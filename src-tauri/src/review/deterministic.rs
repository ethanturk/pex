//! Deterministic, AST-based review checks.
//!
//! Unlike the rule-based *preflight* in `rules.rs` (which only decides which
//! files to review and what checklist text to hand the LLM), this layer
//! produces actual findings with no model involvement: it parses the new file
//! content with tree-sitter, runs a fixed set of structural queries, and emits
//! a finding whenever a match overlaps a line the diff added. Same input →
//! same findings, every run, on every platform.
//!
//! These findings are merged into the same pipeline as the LLM's
//! (`FileAggregateFinding`), but because they already carry exact line numbers
//! they bypass the LLM anchoring/relocation step entirely.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;
use tree_sitter::{Language, Node, Parser, Query, QueryCursor, StreamingIterator};

use crate::diff::engine::DiffHunk;
use crate::review::engine::{FileAggregateFinding, Severity};

/// Languages with an AST checker. Scoped intentionally small for v1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Lang {
    Rust,
    TypeScript,
    Tsx,
    Python,
}

impl Lang {
    fn detect(path: &str) -> Option<Lang> {
        let lower = path.to_ascii_lowercase();
        let ext = lower.rsplit('.').next().unwrap_or("");
        match ext {
            "rs" => Some(Lang::Rust),
            "tsx" => Some(Lang::Tsx),
            "ts" | "mts" | "cts" => Some(Lang::TypeScript),
            "py" | "pyi" => Some(Lang::Python),
            _ => None,
        }
    }

    fn ts_language(self) -> Language {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
        }
    }
}

/// Extra matching applied on top of `requires`, for cases a plain capture-text
/// equality can't express.
#[derive(Debug, Clone, Copy)]
enum Special {
    None,
    /// The reported node is a comment whose text must contain one of these.
    CommentContains(&'static [&'static str]),
    /// The reported node is a Python `except_clause` with no exception type.
    BareExcept,
}

/// A single deterministic rule: a tree-sitter query plus the constraints that
/// decide whether a match is a real finding.
struct DetRule {
    id: &'static str,
    severity: Severity,
    message: &'static str,
    /// Query text. Must capture the node named by `report`; may capture others
    /// referenced in `requires`.
    query: &'static str,
    /// Capture whose line range the finding is reported on.
    report: &'static str,
    /// Each `(capture, allowed)` pair must hold: the capture's text ∈ allowed.
    requires: &'static [(&'static str, &'static [&'static str])],
    special: Special,
    /// Suppress in test files (for rules that are legitimate in tests).
    skip_in_tests: bool,
}

fn rules_for(lang: Lang) -> &'static [DetRule] {
    match lang {
        Lang::Rust => RUST_RULES,
        Lang::TypeScript | Lang::Tsx => TS_RULES,
        Lang::Python => PYTHON_RULES,
    }
}

const RUST_RULES: &[DetRule] = &[
    DetRule {
        id: "rust-unwrap",
        severity: Severity::Minor,
        message: "Avoid .unwrap()/.expect() on Result/Option in non-test code — propagate the error with `?` or handle it explicitly.",
        query: "(call_expression function: (field_expression field: (field_identifier) @flag))",
        report: "flag",
        requires: &[("flag", &["unwrap", "expect"])],
        special: Special::None,
        skip_in_tests: true,
    },
    DetRule {
        id: "rust-dbg",
        severity: Severity::Minor,
        message: "Remove `dbg!` before merging.",
        query: "(macro_invocation macro: (identifier) @flag)",
        report: "flag",
        requires: &[("flag", &["dbg"])],
        special: Special::None,
        skip_in_tests: false,
    },
    DetRule {
        id: "rust-todo",
        severity: Severity::Moderate,
        message: "Unfinished code: `todo!`/`unimplemented!` will panic if reached.",
        query: "(macro_invocation macro: (identifier) @flag)",
        report: "flag",
        requires: &[("flag", &["todo", "unimplemented"])],
        special: Special::None,
        skip_in_tests: false,
    },
];

const TS_RULES: &[DetRule] = &[
    DetRule {
        id: "ts-console",
        severity: Severity::Minor,
        message: "Remove `console.log`/`console.debug` debugging statement.",
        query: "(call_expression function: (member_expression object: (identifier) @obj property: (property_identifier) @flag))",
        report: "flag",
        requires: &[("obj", &["console"]), ("flag", &["log", "debug"])],
        special: Special::None,
        skip_in_tests: true,
    },
    DetRule {
        id: "ts-debugger",
        severity: Severity::Moderate,
        message: "Remove `debugger` statement.",
        query: "(debugger_statement) @flag",
        report: "flag",
        requires: &[],
        special: Special::None,
        skip_in_tests: false,
    },
    DetRule {
        id: "ts-any",
        severity: Severity::Minor,
        message: "Avoid the `any` type — use a specific type or `unknown`.",
        query: "(predefined_type) @flag",
        report: "flag",
        requires: &[("flag", &["any"])],
        special: Special::None,
        skip_in_tests: false,
    },
    DetRule {
        id: "ts-ignore",
        severity: Severity::Moderate,
        message: "Avoid `@ts-ignore`/`@ts-nocheck` — fix the type error or use `@ts-expect-error` with a reason.",
        query: "(comment) @flag",
        report: "flag",
        requires: &[],
        special: Special::CommentContains(&["@ts-ignore", "@ts-nocheck"]),
        skip_in_tests: false,
    },
];

const PYTHON_RULES: &[DetRule] = &[
    DetRule {
        id: "py-bare-except",
        severity: Severity::Moderate,
        message: "Bare `except:` swallows everything (including KeyboardInterrupt/SystemExit) — catch a specific exception type.",
        query: "(except_clause) @flag",
        report: "flag",
        requires: &[],
        special: Special::BareExcept,
        skip_in_tests: false,
    },
    DetRule {
        id: "py-eval-exec",
        severity: Severity::Critical,
        message: "Avoid `eval()`/`exec()` — arbitrary code execution risk.",
        query: "(call function: (identifier) @flag)",
        report: "flag",
        requires: &[("flag", &["eval", "exec"])],
        special: Special::None,
        skip_in_tests: false,
    },
    DetRule {
        id: "py-print",
        severity: Severity::Minor,
        message: "Remove `print()` or use the logging module.",
        query: "(call function: (identifier) @flag)",
        report: "flag",
        requires: &[("flag", &["print"])],
        special: Special::None,
        skip_in_tests: true,
    },
];

/// A rule with its query compiled once, against a specific grammar.
struct CompiledRule {
    rule: &'static DetRule,
    query: Query,
}

/// A grammar plus its compiled rule queries.
struct CompiledLang {
    language: Language,
    rules: Vec<CompiledRule>,
}

/// Tree-sitter `Query` compilation is more expensive than running the query, so
/// compile every rule once at first use and reuse across files. `Query` is
/// `Send + Sync`, so a process-wide cache is safe.
static COMPILED: LazyLock<HashMap<Lang, CompiledLang>> = LazyLock::new(|| {
    let mut map = HashMap::new();
    for lang in [Lang::Rust, Lang::TypeScript, Lang::Tsx, Lang::Python] {
        let language = lang.ts_language();
        let mut rules = Vec::new();
        for rule in rules_for(lang) {
            match Query::new(&language, rule.query) {
                Ok(query) => rules.push(CompiledRule { rule, query }),
                // A malformed built-in query is a programming error: make it
                // loud in tests/debug, but never abort a real review for it.
                Err(e) => debug_assert!(false, "invalid query for {}: {e}", rule.id),
            }
        }
        map.insert(lang, CompiledLang { language, rules });
    }
    map
});

/// Run the deterministic AST checks for `path` against its new content, scoped
/// to the lines the diff actually added. Never panics: a parse failure for one
/// file yields no findings rather than aborting the review.
pub fn check_file(
    path: &str,
    new_content: &str,
    hunks: &[DiffHunk],
) -> Vec<FileAggregateFinding> {
    let Some(lang) = Lang::detect(path) else {
        return Vec::new();
    };
    let Some(compiled) = COMPILED.get(&lang) else {
        return Vec::new();
    };
    let added = added_lines(hunks);
    if added.is_empty() {
        return Vec::new();
    }
    let is_test = is_test_file(path);

    let mut parser = Parser::new();
    if parser.set_language(&compiled.language).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(new_content, None) else {
        return Vec::new();
    };
    let root = tree.root_node();
    let src = new_content.as_bytes();

    let mut findings = Vec::new();
    for CompiledRule { rule, query } in &compiled.rules {
        if rule.skip_in_tests && is_test {
            continue;
        }
        let names = query.capture_names();
        let mut cursor = QueryCursor::new();
        let mut matches = cursor.matches(query, root, src);
        while let Some(m) = matches.next() {
            let Some(node) = evaluate_match(rule, &names, m.captures, src) else {
                continue;
            };
            let start = node.start_position().row + 1;
            let end = node.end_position().row + 1;
            if !overlaps_added(start, end, &added) {
                continue;
            }
            findings.push(FileAggregateFinding {
                severity: rule.severity,
                confidence: 100,
                line_start: Some(start),
                line_end: Some(end),
                comment: rule.message.to_string(),
                existing_code: Some(first_line(node, new_content)),
                evidence: None,
                sources: vec![format!("deterministic:{}", rule.id)],
            });
        }
    }
    findings
}

/// Returns the reported node if every constraint holds, else `None`.
fn evaluate_match<'a>(
    rule: &DetRule,
    names: &[&str],
    captures: &'a [tree_sitter::QueryCapture<'a>],
    src: &[u8],
) -> Option<Node<'a>> {
    // First capture per name (queries here capture each name at most once).
    let lookup = |want: &str| -> Option<Node<'a>> {
        captures
            .iter()
            .find(|c| names.get(c.index as usize) == Some(&want))
            .map(|c| c.node)
    };

    for (cap, allowed) in rule.requires {
        let node = lookup(cap)?;
        let text = node.utf8_text(src).unwrap_or("");
        if !allowed.contains(&text) {
            return None;
        }
    }

    let report = lookup(rule.report)?;

    match rule.special {
        Special::None => {}
        Special::CommentContains(needles) => {
            let text = report.utf8_text(src).unwrap_or("");
            if !needles.iter().any(|n| text.contains(n)) {
                return None;
            }
        }
        Special::BareExcept => {
            let text = report.utf8_text(src).unwrap_or("");
            if !is_bare_except(text) {
                return None;
            }
        }
    }

    Some(report)
}

/// `except:` with nothing between the keyword and the colon.
fn is_bare_except(text: &str) -> bool {
    let t = text.trim_start();
    let Some(rest) = t.strip_prefix("except") else {
        return false;
    };
    rest.trim_start().starts_with(':')
}

fn first_line(node: Node, content: &str) -> String {
    content
        .lines()
        .nth(node.start_position().row)
        .unwrap_or("")
        .trim()
        .to_string()
}

/// New-side line numbers (1-based) that the diff inserted.
fn added_lines(hunks: &[DiffHunk]) -> HashSet<usize> {
    let mut set = HashSet::new();
    for hunk in hunks {
        for line in &hunk.lines {
            if line.kind == "+" {
                if let Some(n) = line.new_lineno {
                    set.insert(n);
                }
            }
        }
    }
    set
}

fn overlaps_added(start: usize, end: usize, added: &HashSet<usize>) -> bool {
    (start..=end).any(|l| added.contains(&l))
}

fn is_test_file(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.contains("/test/")
        || lower.contains("/tests/")
        || lower.starts_with("tests/")
        || lower.contains("__tests__")
        || lower.contains(".test.")
        || lower.contains(".spec.")
        || lower.ends_with("_test.rs")
        || lower.ends_with("_test.py")
        || lower.starts_with("test_")
        || lower.contains("/test_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::engine::extract_hunks;

    fn check(path: &str, old: &str, new: &str) -> Vec<FileAggregateFinding> {
        let hunks = extract_hunks(old, new);
        check_file(path, new, &hunks)
    }

    fn ids(findings: &[FileAggregateFinding]) -> Vec<String> {
        findings.iter().flat_map(|f| f.sources.clone()).collect()
    }

    #[test]
    fn flags_rust_unwrap_on_added_line() {
        let old = "fn main() {}\n";
        let new = "fn main() {\n    let x = foo().unwrap();\n}\n";
        let f = check("src/main.rs", old, new);
        assert_eq!(ids(&f), vec!["deterministic:rust-unwrap"]);
        assert_eq!(f[0].line_start, Some(2));
        assert_eq!(f[0].confidence, 100);
    }

    #[test]
    fn ignores_unwrap_in_test_files() {
        let old = "\n";
        let new = "fn t() {\n    foo().unwrap();\n}\n";
        assert!(check("src/foo_test.rs", old, new).is_empty());
        assert!(check("tests/integration.rs", old, new).is_empty());
    }

    #[test]
    fn ignores_unchanged_unwrap() {
        // unwrap exists in both old and new (context line, not added) → no finding.
        let code = "fn main() {\n    foo().unwrap();\n}\n";
        let new = "fn main() {\n    foo().unwrap();\n    let y = 1;\n}\n";
        let f = check("src/main.rs", code, new);
        assert!(ids(&f).iter().all(|id| id != "deterministic:rust-unwrap"));
    }

    #[test]
    fn flags_rust_dbg_and_todo() {
        let old = "fn main() {}\n";
        let new = "fn main() {\n    dbg!(x);\n    todo!();\n}\n";
        let f = check("src/main.rs", old, new);
        let got = ids(&f);
        assert!(got.contains(&"deterministic:rust-dbg".to_string()));
        assert!(got.contains(&"deterministic:rust-todo".to_string()));
    }

    #[test]
    fn flags_ts_console_and_debugger_and_any() {
        let old = "export const x = 1;\n";
        let new = "export function f(v: any) {\n  console.log(v);\n  debugger;\n}\n";
        let f = check("src/app.ts", old, new);
        let got = ids(&f);
        assert!(got.contains(&"deterministic:ts-console".to_string()));
        assert!(got.contains(&"deterministic:ts-debugger".to_string()));
        assert!(got.contains(&"deterministic:ts-any".to_string()));
    }

    #[test]
    fn ts_console_error_not_flagged() {
        let old = "const x = 1;\n";
        let new = "const x = 1;\nconsole.error('boom');\n";
        let f = check("src/app.ts", old, new);
        assert!(ids(&f).iter().all(|id| id != "deterministic:ts-console"));
    }

    #[test]
    fn flags_ts_ignore_comment() {
        let old = "const x = 1;\n";
        let new = "const x = 1;\n// @ts-ignore\nconst y: number = z;\n";
        let f = check("src/app.tsx", old, new);
        assert!(ids(&f).contains(&"deterministic:ts-ignore".to_string()));
    }

    #[test]
    fn flags_python_bare_except_but_not_typed() {
        let old = "x = 1\n";
        let new_bare = "x = 1\ntry:\n    pass\nexcept:\n    pass\n";
        assert!(ids(&check("app.py", old, new_bare))
            .contains(&"deterministic:py-bare-except".to_string()));

        let new_typed = "x = 1\ntry:\n    pass\nexcept ValueError:\n    pass\n";
        assert!(ids(&check("app.py", old, new_typed))
            .iter()
            .all(|id| id != "deterministic:py-bare-except"));
    }

    #[test]
    fn flags_python_eval() {
        let old = "x = 1\n";
        let new = "x = 1\ny = eval(user_input)\n";
        let f = check("app.py", old, new);
        assert!(ids(&f).contains(&"deterministic:py-eval-exec".to_string()));
        assert_eq!(f.iter().find(|x| x.sources[0].contains("eval")).unwrap().severity, Severity::Critical);
    }

    #[test]
    fn no_findings_for_unknown_language() {
        let old = "a\n";
        let new = "a\nb\n";
        assert!(check("README.md", old, new).is_empty());
    }
}
