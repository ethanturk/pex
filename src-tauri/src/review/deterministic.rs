//! Deterministic, AST-based review checks (powered by ast-grep).
//!
//! Unlike the rule-based *preflight* in `rules.rs` (which only decides which
//! files to review and what checklist text to hand the LLM), this layer
//! produces actual findings with no model involvement: it parses the new file
//! content and runs structural rules, emitting a finding whenever a match
//! overlaps a line the diff added. Same input → same findings, every run, on
//! every platform.
//!
//! Rules are ast-grep YAML, from two sources merged at review time:
//!   1. **Built-in stock rules** — always loaded, embedded below, compiled once
//!      (`BUILTIN`).
//!   2. **Repo rules** — optional `.pex/ast-rules.yml` in the reviewed repo,
//!      fetched at the PR commit and compiled per review (`compile_repo_rules`).
//!
//! Matching uses `ast-grep-core`/`ast-grep-config` wrapping our own tree-sitter
//! grammars (no `ast-grep-language` bundle). Findings carry exact line numbers,
//! so they bypass the LLM anchoring/relocation step.

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use ast_grep_config::{DeserializeEnv, RuleCore, SerializableRuleCore};
use ast_grep_core::matcher::{PatternBuilder, PatternError};
use ast_grep_core::tree_sitter::{LanguageExt, StrDoc, TSLanguage};
use ast_grep_core::{AstGrep, Language, Node};
use serde::Deserialize;

use crate::diff::engine::DiffHunk;
use crate::review::engine::{FileAggregateFinding, Severity};

/// Cap on repo-provided rules, to bound parse/compile/run cost from an
/// untrusted manifest.
const MAX_REPO_RULES: usize = 500;

/// Languages with a compiled-in grammar. New languages require a new grammar
/// dependency; repo rules can only target these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Lang {
    Rust,
    JavaScript,
    TypeScript,
    Tsx,
    Python,
    CSharp,
}

impl Lang {
    fn detect(path: &str) -> Option<Lang> {
        let lower = path.to_ascii_lowercase();
        let ext = lower.rsplit('.').next().unwrap_or("");
        match ext {
            "rs" => Some(Lang::Rust),
            "js" | "mjs" | "cjs" | "jsx" => Some(Lang::JavaScript),
            "tsx" => Some(Lang::Tsx),
            "ts" | "mts" | "cts" => Some(Lang::TypeScript),
            "py" | "pyi" => Some(Lang::Python),
            "cs" => Some(Lang::CSharp),
            _ => None,
        }
    }

    /// Parse a `language` field from a rule manifest.
    fn from_name(name: &str) -> Option<Lang> {
        match name.to_ascii_lowercase().as_str() {
            "rust" | "rs" => Some(Lang::Rust),
            "javascript" | "js" | "jsx" => Some(Lang::JavaScript),
            "typescript" | "ts" => Some(Lang::TypeScript),
            "tsx" => Some(Lang::Tsx),
            "python" | "py" => Some(Lang::Python),
            "csharp" | "c#" | "cs" => Some(Lang::CSharp),
            _ => None,
        }
    }

    fn ts_language(self) -> TSLanguage {
        match self {
            Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
            Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
            Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
            Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
            Lang::Python => tree_sitter_python::LANGUAGE.into(),
            Lang::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        }
    }
}

/// A manifest `language` maps to one or more grammars to compile against.
/// TypeScript rules also apply to TSX (a separate grammar), so they compile for
/// both. JS/JSX share one grammar, so no expansion is needed there.
fn expand_lang(lang: Lang) -> &'static [Lang] {
    match lang {
        Lang::TypeScript => &[Lang::TypeScript, Lang::Tsx],
        Lang::Rust => &[Lang::Rust],
        Lang::JavaScript => &[Lang::JavaScript],
        Lang::Tsx => &[Lang::Tsx],
        Lang::Python => &[Lang::Python],
        Lang::CSharp => &[Lang::CSharp],
    }
}

/// ast-grep `Language` adapter over our tree-sitter grammars. Lets us use
/// ast-grep's matcher with only our 5 grammars (no `ast-grep-language` bundle).
#[derive(Clone)]
struct AgLang(Lang);

impl Language for AgLang {
    fn kind_to_id(&self, kind: &str) -> u16 {
        self.get_ts_language().id_for_node_kind(kind, true)
    }
    fn field_to_id(&self, field: &str) -> Option<u16> {
        self.get_ts_language()
            .field_id_for_name(field)
            .map(|f| f.get())
    }
    fn build_pattern(
        &self,
        builder: &PatternBuilder,
    ) -> Result<ast_grep_core::Pattern, PatternError> {
        builder.build(|src| StrDoc::try_new(src, self.clone()))
    }
}

impl LanguageExt for AgLang {
    fn get_ts_language(&self) -> TSLanguage {
        self.0.ts_language()
    }
}

type Doc = StrDoc<AgLang>;

/// A rule's metadata plus its compiled matcher (against one grammar).
struct CompiledRule {
    id: String,
    severity: Severity,
    message: String,
    skip_in_tests: bool,
    matcher: RuleCore,
}

/// Compiled rules grouped by the grammar they were compiled against.
pub struct CompiledRuleSet {
    by_lang: HashMap<Lang, Vec<CompiledRule>>,
}

impl std::fmt::Debug for CompiledRuleSet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // RuleCore matchers aren't Debug; summarize by count.
        f.debug_struct("CompiledRuleSet")
            .field("rules", &self.len())
            .finish()
    }
}

impl CompiledRuleSet {
    fn empty() -> Self {
        Self {
            by_lang: HashMap::new(),
        }
    }

    fn rules(&self, lang: Lang) -> &[CompiledRule] {
        self.by_lang.get(&lang).map(Vec::as_slice).unwrap_or(&[])
    }

    pub fn len(&self) -> usize {
        self.by_lang.values().map(Vec::len).sum()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

// ---- Rule manifest (YAML) ----

#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    rules: Vec<RuleSpec>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuleSpec {
    id: String,
    language: String,
    severity: Severity,
    message: String,
    #[serde(default)]
    skip_in_tests: bool,
    /// The ast-grep rule body: `rule`, plus optional `constraints`, `utils`,
    /// `transform`, `fix`.
    #[serde(flatten)]
    core: SerializableRuleCore,
}

/// Parse and compile a YAML rule manifest. Never fails: a bad manifest or rule
/// yields an empty/partial set plus human-readable warnings. `cap` bounds the
/// number of rules (used for untrusted repo manifests).
fn compile_manifest(yaml: &str, cap: Option<usize>) -> (CompiledRuleSet, Vec<String>) {
    let mut warnings = Vec::new();
    let manifest: Manifest = match ast_grep_config::from_str(yaml) {
        Ok(manifest) => manifest,
        Err(e) => {
            warnings.push(format!("invalid AST rules manifest: {e}"));
            return (CompiledRuleSet::empty(), warnings);
        }
    };

    let mut by_lang: HashMap<Lang, Vec<CompiledRule>> = HashMap::new();
    let mut count = 0usize;
    for spec in manifest.rules {
        if cap.is_some_and(|c| count >= c) {
            warnings.push(format!(
                "too many AST rules; ignoring beyond {}",
                cap.unwrap()
            ));
            break;
        }
        count += 1;

        let Some(base) = Lang::from_name(&spec.language) else {
            warnings.push(format!(
                "rule '{}': unsupported language '{}'",
                spec.id, spec.language
            ));
            continue;
        };

        for &lang in expand_lang(base) {
            let env = DeserializeEnv::new(AgLang(lang));
            match spec.core.get_matcher(env) {
                Ok(matcher) => by_lang.entry(lang).or_default().push(CompiledRule {
                    id: spec.id.clone(),
                    severity: spec.severity,
                    message: spec.message.clone(),
                    skip_in_tests: spec.skip_in_tests,
                    matcher,
                }),
                Err(e) => warnings.push(format!("rule '{}' ({lang:?}): {e}", spec.id)),
            }
        }
    }
    (CompiledRuleSet { by_lang }, warnings)
}

/// Parse and compile a repo `.pex/ast-rules.yml` manifest (rule count capped).
pub fn compile_repo_rules(yaml: &str) -> (CompiledRuleSet, Vec<String>) {
    compile_manifest(yaml, Some(MAX_REPO_RULES))
}

/// Built-in stock rules, always loaded, compiled once at first use.
static BUILTIN: LazyLock<CompiledRuleSet> = LazyLock::new(|| {
    let (set, warnings) = compile_manifest(BUILTIN_YAML, None);
    debug_assert!(warnings.is_empty(), "built-in rule errors: {warnings:?}");
    set
});

/// Embedded stock rules covering Rust, JavaScript/JSX, TypeScript/TSX, Python,
/// and C#. Authored in the same format repo rules use.
const BUILTIN_YAML: &str = r#"
rules:
  - id: rust-unwrap
    language: rust
    severity: minor
    message: "Avoid .unwrap()/.expect() on Result/Option in non-test code — propagate the error with `?` or handle it explicitly."
    skipInTests: true
    rule:
      any:
        - pattern: $A.unwrap()
        - pattern: $A.expect($$$)
  - id: rust-dbg
    language: rust
    severity: minor
    message: "Remove `dbg!` before merging."
    rule:
      kind: macro_invocation
      regex: '^dbg!'
  - id: rust-todo
    language: rust
    severity: moderate
    message: "Unfinished code: `todo!`/`unimplemented!` will panic if reached."
    rule:
      kind: macro_invocation
      regex: '^(todo|unimplemented)!'
  - id: js-console
    language: javascript
    severity: minor
    message: "Remove `console.log`/`console.debug` debugging statement."
    skipInTests: true
    rule:
      any:
        - pattern: console.log($$$)
        - pattern: console.debug($$$)
  - id: js-debugger
    language: javascript
    severity: moderate
    message: "Remove `debugger` statement."
    rule:
      kind: debugger_statement
  - id: js-eval
    language: javascript
    severity: moderate
    message: "Avoid `eval()` — code injection risk and deopt."
    rule:
      pattern: eval($$$)
  - id: ts-console
    language: typescript
    severity: minor
    message: "Remove `console.log`/`console.debug` debugging statement."
    skipInTests: true
    rule:
      any:
        - pattern: console.log($$$)
        - pattern: console.debug($$$)
  - id: ts-debugger
    language: typescript
    severity: moderate
    message: "Remove `debugger` statement."
    rule:
      kind: debugger_statement
  - id: ts-any
    language: typescript
    severity: minor
    message: "Avoid the `any` type — use a specific type or `unknown`."
    rule:
      kind: predefined_type
      regex: '^any$'
  - id: ts-ignore
    language: typescript
    severity: moderate
    message: "Avoid `@ts-ignore`/`@ts-nocheck` — fix the type error or use `@ts-expect-error` with a reason."
    rule:
      kind: comment
      regex: '@ts-ignore|@ts-nocheck'
  - id: ts-eval
    language: typescript
    severity: moderate
    message: "Avoid `eval()` — code injection risk and deopt."
    rule:
      pattern: eval($$$)
  - id: py-bare-except
    language: python
    severity: moderate
    message: "Bare `except:` swallows everything (including KeyboardInterrupt/SystemExit) — catch a specific exception type."
    rule:
      kind: except_clause
      regex: '^except\s*:'
  - id: py-eval-exec
    language: python
    severity: critical
    message: "Avoid `eval()`/`exec()` — arbitrary code execution risk."
    rule:
      any:
        - pattern: eval($$$)
        - pattern: exec($$$)
  - id: py-print
    language: python
    severity: minor
    message: "Remove `print()` or use the logging module."
    skipInTests: true
    rule:
      pattern: print($$$)
  - id: cs-console-writeline
    language: csharp
    severity: minor
    message: "Remove `Console.WriteLine`/`Console.Write` debugging output or use a logger."
    skipInTests: true
    rule:
      any:
        - pattern: Console.WriteLine($$$)
        - pattern: Console.Write($$$)
  - id: cs-throw-base-exception
    language: csharp
    severity: moderate
    message: "Throw a specific exception type rather than the base `Exception`/`ApplicationException`/`SystemException`."
    rule:
      any:
        - pattern: throw new Exception($$$)
        - pattern: throw new ApplicationException($$$)
        - pattern: throw new SystemException($$$)
  - id: cs-debugger-break
    language: csharp
    severity: moderate
    message: "Remove `Debugger.Break()`/`Debugger.Launch()`."
    rule:
      any:
        - pattern: Debugger.Break()
        - pattern: Debugger.Launch()
"#;

/// Run the deterministic AST checks for `path` against its new content, scoped
/// to the lines the diff added. Runs the built-in stock rules plus any repo
/// rules supplied. Never panics: an unsupported language or empty diff yields
/// no findings.
pub fn check_file(
    path: &str,
    new_content: &str,
    hunks: &[DiffHunk],
    repo: Option<&CompiledRuleSet>,
) -> Vec<FileAggregateFinding> {
    let Some(lang) = Lang::detect(path) else {
        return Vec::new();
    };
    let added = added_lines(hunks);
    if added.is_empty() {
        return Vec::new();
    }
    let is_test = is_test_file(path);

    let grep = AstGrep::new(new_content, AgLang(lang));
    let root = grep.root();

    let mut findings = Vec::new();
    run_rules(BUILTIN.rules(lang), &root, &added, is_test, &mut findings);
    if let Some(repo) = repo {
        run_rules(repo.rules(lang), &root, &added, is_test, &mut findings);
    }
    findings
}

fn run_rules(
    rules: &[CompiledRule],
    root: &Node<Doc>,
    added: &HashSet<usize>,
    is_test: bool,
    out: &mut Vec<FileAggregateFinding>,
) {
    for cr in rules {
        if cr.skip_in_tests && is_test {
            continue;
        }
        for m in root.find_all(&cr.matcher) {
            let start = m.start_pos().line() + 1;
            let end = m.end_pos().line() + 1;
            if !overlaps_added(start, end, added) {
                continue;
            }
            let snippet = m.text().lines().next().unwrap_or("").trim().to_string();
            out.push(FileAggregateFinding {
                severity: cr.severity,
                confidence: 100,
                line_start: Some(start),
                line_end: Some(end),
                comment: cr.message.clone(),
                existing_code: Some(snippet),
                evidence: None,
                sources: vec![format!("deterministic:{}", cr.id)],
            });
        }
    }
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
        || lower.ends_with("tests.cs")
        || lower.starts_with("test_")
        || lower.contains("/test_")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::diff::engine::extract_hunks;

    fn check(path: &str, old: &str, new: &str) -> Vec<FileAggregateFinding> {
        let hunks = extract_hunks(old, new);
        check_file(path, new, &hunks, None)
    }

    fn ids(findings: &[FileAggregateFinding]) -> Vec<String> {
        findings.iter().flat_map(|f| f.sources.clone()).collect()
    }

    #[test]
    fn builtins_compile_cleanly() {
        // Forces the LazyLock; the debug_assert inside fires on any bad rule.
        // 16 rules, with the 5 typescript rules also compiled for tsx (+5).
        assert!(BUILTIN.len() >= 20, "len = {}", BUILTIN.len());
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
        let new = "fn t() {\n    foo().unwrap();\n}\n";
        assert!(check("src/foo_test.rs", "\n", new).is_empty());
        assert!(check("tests/integration.rs", "\n", new).is_empty());
    }

    #[test]
    fn ignores_unchanged_unwrap() {
        let code = "fn main() {\n    foo().unwrap();\n}\n";
        let new = "fn main() {\n    foo().unwrap();\n    let y = 1;\n}\n";
        assert!(ids(&check("src/main.rs", code, new))
            .iter()
            .all(|id| id != "deterministic:rust-unwrap"));
    }

    #[test]
    fn flags_rust_dbg_and_todo() {
        let new = "fn main() {\n    dbg!(x);\n    todo!();\n}\n";
        let got = ids(&check("src/main.rs", "fn main() {}\n", new));
        assert!(got.contains(&"deterministic:rust-dbg".to_string()));
        assert!(got.contains(&"deterministic:rust-todo".to_string()));
    }

    #[test]
    fn flags_js_console_debugger_eval() {
        let new = "function f() {\n  console.log(1);\n  debugger;\n  eval(s);\n}\n";
        let got = ids(&check("src/app.js", "const x = 1;\n", new));
        assert!(got.contains(&"deterministic:js-console".to_string()));
        assert!(got.contains(&"deterministic:js-debugger".to_string()));
        assert!(got.contains(&"deterministic:js-eval".to_string()));
    }

    #[test]
    fn flags_jsx_console() {
        let new = "const C = () => {\n  console.log('x');\n  return <div/>;\n};\n";
        assert!(ids(&check("src/C.jsx", "const x = 1;\n", new))
            .contains(&"deterministic:js-console".to_string()));
    }

    #[test]
    fn flags_tsx_any_and_console() {
        // Exercises the typescript→tsx grammar expansion.
        let new = "const C = (v: any) => {\n  console.log(v);\n  return <div/>;\n};\n";
        let got = ids(&check("src/C.tsx", "const x = 1;\n", new));
        assert!(
            got.contains(&"deterministic:ts-any".to_string()),
            "got {got:?}"
        );
        assert!(
            got.contains(&"deterministic:ts-console".to_string()),
            "got {got:?}"
        );
    }

    #[test]
    fn flags_ts_console_debugger_any() {
        let new = "export function f(v: any) {\n  console.log(v);\n  debugger;\n}\n";
        let got = ids(&check("src/app.ts", "export const x = 1;\n", new));
        assert!(got.contains(&"deterministic:ts-console".to_string()));
        assert!(got.contains(&"deterministic:ts-debugger".to_string()));
        assert!(got.contains(&"deterministic:ts-any".to_string()));
    }

    #[test]
    fn ts_console_error_not_flagged() {
        let new = "const x = 1;\nconsole.error('boom');\n";
        assert!(ids(&check("src/app.ts", "const x = 1;\n", new))
            .iter()
            .all(|id| id != "deterministic:ts-console"));
    }

    #[test]
    fn flags_ts_ignore_comment() {
        let new = "const x = 1;\n// @ts-ignore\nconst y: number = z;\n";
        assert!(ids(&check("src/app.tsx", "const x = 1;\n", new))
            .contains(&"deterministic:ts-ignore".to_string()));
    }

    #[test]
    fn flags_python_bare_except_but_not_typed() {
        let bare = "x = 1\ntry:\n    pass\nexcept:\n    pass\n";
        assert!(ids(&check("app.py", "x = 1\n", bare))
            .contains(&"deterministic:py-bare-except".to_string()));
        let typed = "x = 1\ntry:\n    pass\nexcept ValueError:\n    pass\n";
        assert!(ids(&check("app.py", "x = 1\n", typed))
            .iter()
            .all(|id| id != "deterministic:py-bare-except"));
    }

    #[test]
    fn flags_python_eval() {
        let new = "x = 1\ny = eval(user_input)\n";
        let f = check("app.py", "x = 1\n", new);
        assert!(ids(&f).contains(&"deterministic:py-eval-exec".to_string()));
        assert_eq!(
            f.iter()
                .find(|x| x.sources[0].contains("eval"))
                .unwrap()
                .severity,
            Severity::Critical
        );
    }

    #[test]
    fn flags_csharp_writeline_throw_debugger() {
        let new = "class C {\n  void M() {\n    Console.WriteLine(\"x\");\n    Debugger.Break();\n    throw new Exception(\"e\");\n  }\n}\n";
        let got = ids(&check("src/C.cs", "class C {}\n", new));
        assert!(
            got.contains(&"deterministic:cs-console-writeline".to_string()),
            "got {got:?}"
        );
        assert!(
            got.contains(&"deterministic:cs-debugger-break".to_string()),
            "got {got:?}"
        );
        assert!(
            got.contains(&"deterministic:cs-throw-base-exception".to_string()),
            "got {got:?}"
        );
    }

    #[test]
    fn no_findings_for_unknown_language() {
        assert!(check("README.md", "a\n", "a\nb\n").is_empty());
    }

    #[test]
    fn repo_rule_compiles_and_flags() {
        let yaml = r#"
rules:
  - id: no-foo
    language: rust
    severity: moderate
    message: "Do not call foo()."
    rule:
      pattern: foo()
"#;
        let (set, warnings) = compile_repo_rules(yaml);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        assert_eq!(set.len(), 1);
        let hunks = extract_hunks("fn m() {}\n", "fn m() {\n    foo();\n}\n");
        let f = check_file("src/x.rs", "fn m() {\n    foo();\n}\n", &hunks, Some(&set));
        assert!(ids(&f).contains(&"deterministic:no-foo".to_string()));
    }

    #[test]
    fn repo_typescript_rule_also_targets_tsx() {
        let yaml = r#"
rules:
  - id: ts-rule
    language: typescript
    severity: minor
    message: "m"
    rule:
      pattern: bad($$$)
"#;
        let (set, warnings) = compile_repo_rules(yaml);
        assert!(warnings.is_empty(), "warnings: {warnings:?}");
        // Compiled for both typescript and tsx grammars.
        assert_eq!(set.len(), 2);
        let hunks = extract_hunks("const x = 1;\n", "const x = 1;\nbad(1);\n");
        let f = check_file("src/c.tsx", "const x = 1;\nbad(1);\n", &hunks, Some(&set));
        assert!(ids(&f).contains(&"deterministic:ts-rule".to_string()));
    }

    #[test]
    fn repo_rule_bad_pattern_warns_not_panics() {
        let yaml = "rules:\n  - id: bad\n    language: rust\n    severity: minor\n    message: m\n    rule:\n      kind: not_a_real_node_kind_xyz\n";
        let (_set, warnings) = compile_repo_rules(yaml);
        // Either compiles to a never-matching rule or warns; must not panic.
        let hunks = extract_hunks("fn m(){}\n", "fn m(){ let y = 1; }\n");
        let _ = check_file("src/x.rs", "fn m(){ let y = 1; }\n", &hunks, Some(&_set));
        let _ = warnings;
    }

    #[test]
    fn repo_rule_unsupported_language_warns() {
        let yaml = "rules:\n  - id: x\n    language: haskell\n    severity: minor\n    message: m\n    rule:\n      pattern: x\n";
        let (set, warnings) = compile_repo_rules(yaml);
        assert!(set.is_empty());
        assert!(warnings.iter().any(|w| w.contains("haskell")));
    }

    #[test]
    fn invalid_manifest_yaml_warns() {
        let (set, warnings) = compile_repo_rules(":\n  not: [valid");
        assert!(set.is_empty());
        assert_eq!(warnings.len(), 1);
    }
}
