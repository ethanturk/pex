#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRuleMatch {
    pub source: String,
    pub pattern: Option<String>,
    pub title: String,
    pub rule: String,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ReviewRuleConfig {
    #[serde(default)]
    pub include: Vec<String>,
    #[serde(default)]
    pub exclude: Vec<String>,
    #[serde(default)]
    pub default_rule: Option<RuleText>,
    #[serde(default)]
    pub rules: Vec<RepoRule>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(untagged)]
pub enum RuleText {
    Text(String),
    Object { title: Option<String>, rule: String },
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RepoRule {
    pub path: String,
    pub title: String,
    pub rule: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RuleDecision {
    Review(ReviewRuleMatch),
    Skip { reason: String },
}

#[derive(Debug, Clone)]
pub struct ReviewRuleResolver {
    repo_config: Option<ReviewRuleConfig>,
}

impl ReviewRuleResolver {
    pub fn new(repo_config: Option<ReviewRuleConfig>) -> Self {
        Self { repo_config }
    }

    pub fn from_json(raw: &str) -> Result<Self, String> {
        let config = serde_json::from_str::<ReviewRuleConfig>(raw)
            .map_err(|e| format!("Invalid .pex/review-rules.json: {}", e))?;
        Ok(Self::new(Some(config)))
    }

    pub fn resolve(&self, path: &str, status: &str) -> RuleDecision {
        let path = normalize_path(path);
        let status = status.to_ascii_lowercase();
        if status == "delete" {
            return RuleDecision::Skip {
                reason: "deleted".to_string(),
            };
        }
        if is_binary_path(&path) {
            return RuleDecision::Skip {
                reason: "binary".to_string(),
            };
        }

        if let Some(config) = &self.repo_config {
            if config
                .exclude
                .iter()
                .any(|pattern| glob_match(pattern, &path))
            {
                return RuleDecision::Skip {
                    reason: "excluded".to_string(),
                };
            }
            if !config.include.is_empty()
                && !config
                    .include
                    .iter()
                    .any(|pattern| glob_match(pattern, &path))
            {
                return RuleDecision::Skip {
                    reason: "notIncluded".to_string(),
                };
            }
            for rule in &config.rules {
                if glob_match(&rule.path, &path) {
                    return RuleDecision::Review(ReviewRuleMatch {
                        source: "repo".to_string(),
                        pattern: Some(rule.path.clone()),
                        title: rule.title.clone(),
                        rule: rule.rule.clone(),
                    });
                }
            }
            if let Some(default_rule) = &config.default_rule {
                let (title, rule) = match default_rule {
                    RuleText::Text(rule) => ("Repository checklist".to_string(), rule.clone()),
                    RuleText::Object { title, rule } => (
                        title
                            .clone()
                            .unwrap_or_else(|| "Repository checklist".to_string()),
                        rule.clone(),
                    ),
                };
                return RuleDecision::Review(ReviewRuleMatch {
                    source: "repo".to_string(),
                    pattern: None,
                    title,
                    rule,
                });
            }
        }

        if builtin_excludes()
            .iter()
            .any(|pattern| glob_match(pattern, &path))
        {
            return RuleDecision::Skip {
                reason: "excluded".to_string(),
            };
        }
        for (pattern, title, rule) in builtin_rules() {
            if glob_match(pattern, &path) {
                return RuleDecision::Review(ReviewRuleMatch {
                    source: "builtin".to_string(),
                    pattern: Some(pattern.to_string()),
                    title: title.to_string(),
                    rule: rule.to_string(),
                });
            }
        }
        if is_text_reviewable_path(&path) {
            return RuleDecision::Review(ReviewRuleMatch {
                source: "builtin".to_string(),
                pattern: None,
                title: "General code review checklist".to_string(),
                rule: "Check changed behavior, error handling, security and data validation, integration boundaries, maintainability, and whether tests or deployment updates are needed.".to_string(),
            });
        }
        RuleDecision::Skip {
            reason: "unsupportedPath".to_string(),
        }
    }
}

impl Default for ReviewRuleResolver {
    fn default() -> Self {
        Self::new(None)
    }
}

pub fn normalize_path(path: &str) -> String {
    path.trim_start_matches('/').replace('\\', "/")
}

fn builtin_excludes() -> &'static [&'static str] {
    &[
        "dist/**",
        "target/**",
        "node_modules/**",
        "src-tauri/gen/**",
        "**/*.lock",
        "**/package-lock.json",
        "**/Cargo.lock",
        "**/*.min.js",
        "**/*.snap",
    ]
}

fn builtin_rules() -> &'static [(&'static str, &'static str, &'static str)] {
    &[
        (
            "src-tauri/src/**/*.rs",
            "Rust/Tauri backend checklist",
            "Check async error propagation, provider abstraction boundaries, cancellation, cache consistency, serde compatibility, platform cfg gates, and ADO/GitHub API behavior.",
        ),
        (
            "**/*.py",
            "Python checklist",
            "Check runtime errors, exception handling, async/resource lifetimes, data validation, security-sensitive input handling, type assumptions, dependency behavior, and missing tests.",
        ),
        (
            "**/*.pyi",
            "Python typing checklist",
            "Check type signatures, optional/nullability semantics, overload consistency, API compatibility, and whether implementation and stub behavior stay aligned.",
        ),
        (
            "**/*.cs",
            "C# checklist",
            "Check nullability, async/await and cancellation, disposal/lifetime ownership, LINQ/query behavior, exception handling, authorization/data validation, dependency injection boundaries, and test coverage.",
        ),
        (
            "**/*.csproj",
            ".NET project checklist",
            "Check target frameworks, package versions, build properties, analyzers, generated assets, publish settings, and compatibility with CI and deployment.",
        ),
        (
            "**/*.sln",
            ".NET solution checklist",
            "Check project membership, configuration/platform mappings, dependency ordering, and whether solution changes match the intended build/test scope.",
        ),
        (
            "**/*.props",
            ".NET MSBuild checklist",
            "Check shared build properties, package/version propagation, conditional logic, target framework compatibility, and CI/build side effects.",
        ),
        (
            "**/*.targets",
            ".NET MSBuild checklist",
            "Check target ordering, incremental build behavior, generated outputs, packaging side effects, and platform-specific conditions.",
        ),
        (
            "**/azuredeploy*.json",
            "Azure ARM template checklist",
            "Check resource API versions, parameter defaults, secureString/secret handling, dependencies, idempotency, naming/location expressions, RBAC/network exposure, and deployment-mode compatibility.",
        ),
        (
            "**/*.parameters.json",
            "Azure ARM parameters checklist",
            "Check parameter names/types match templates, secret values are not hard-coded, environment-specific values are intentional, and deployment defaults are safe.",
        ),
        (
            "**/*.template.json",
            "Azure ARM template checklist",
            "Check resource API versions, parameter defaults, secureString/secret handling, dependencies, idempotency, naming/location expressions, RBAC/network exposure, and deployment-mode compatibility.",
        ),
        (
            "**/*.bicep",
            "Azure Bicep checklist",
            "Check resource API versions, module/parameter contracts, secure parameters, dependencies, scopes, RBAC/network exposure, and generated ARM behavior.",
        ),
        (
            "**/*.bicepparam",
            "Azure Bicep parameters checklist",
            "Check parameter names/types match the Bicep template, secret handling, environment-specific values, and deployment safety.",
        ),
        (
            "src/**/*.tsx",
            "Preact UI checklist",
            "Check state/signal ownership, async loading and cancellation, mobile touch ergonomics, accessibility labels, text overflow, and dark-mode Tailwind classes.",
        ),
        (
            "src/**/*.ts",
            "TypeScript frontend checklist",
            "Check API type parity with Tauri commands, signal updates, error handling, local-storage compatibility, and mobile/desktop platform branching.",
        ),
        (
            "src/styles/**/*.css",
            "Responsive CSS checklist",
            "Check mobile safe areas, touch targets, text overflow, dark mode, and that visual changes remain consistent with the app's utilitarian review workflow.",
        ),
        (
            "src-tauri/tauri.conf.json",
            "Tauri configuration checklist",
            "Check bundle identifiers, platform config, permissions, updater settings, and that desktop/mobile targets remain compatible.",
        ),
        (
            "src-tauri/android/**/*.kt",
            "Android shell checklist",
            "Check WebView lifecycle, asset loading, IPC bridge assumptions, keyboard resizing, API level constraints, and Rust library loading.",
        ),
        (
            "src-tauri/ios/**/*.swift",
            "iOS shell checklist",
            "Check WKWebView lifecycle, safe areas, Tauri IPC bridge assumptions, bundle paths, keychain behavior, and iOS deployment target compatibility.",
        ),
        (
            "**/*.toml",
            "Manifest checklist",
            "Check dependency feature flags, platform-specific dependencies, package metadata, and CI/build compatibility.",
        ),
        (
            "**/*.json",
            "Configuration checklist",
            "Check schema compatibility, generated-vs-source ownership, platform-specific fields, and whether the change affects release or build behavior.",
        ),
        (
            "**/*.yml",
            "Workflow checklist",
            "Check secret handling, reproducibility, platform setup, artifact paths, and release trigger behavior.",
        ),
        (
            "**/*.yaml",
            "Workflow checklist",
            "Check secret handling, reproducibility, platform setup, artifact paths, and release trigger behavior.",
        ),
        (
            "**/*.md",
            "Documentation checklist",
            "Check that instructions are actionable, current with the code paths they reference, and do not conflict with platform-specific setup steps.",
        ),
        (
            "**/*.sh",
            "Shell script checklist",
            "Check idempotency, quoting, platform assumptions, exit behavior, and generated-file ownership.",
        ),
        (
            "**/*.ps1",
            "PowerShell script checklist",
            "Check idempotency, quoting/escaping, error action behavior, platform assumptions, secret handling, and deployment side effects.",
        ),
    ]
}

fn is_binary_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let ext = lower.rsplit('.').next().unwrap_or("");
    matches!(
        ext,
        "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "webp"
            | "ico"
            | "icns"
            | "pdf"
            | "zip"
            | "gz"
            | "tgz"
            | "jks"
            | "aab"
            | "apk"
            | "dmg"
            | "msi"
            | "dll"
            | "so"
            | "dylib"
            | "a"
            | "wasm"
            | "woff"
            | "woff2"
            | "ttf"
            | "otf"
    )
}

fn is_text_reviewable_path(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "dockerfile" | "makefile" | "rakefile" | "gemfile" | "procfile"
    ) {
        return true;
    }
    let ext = lower.rsplit('.').next().unwrap_or("");
    matches!(
        ext,
        "txt"
            | "config"
            | "conf"
            | "ini"
            | "env"
            | "xml"
            | "props"
            | "targets"
            | "cshtml"
            | "razor"
            | "sql"
            | "ps1"
            | "psm1"
            | "psd1"
            | "bat"
            | "cmd"
            | "java"
            | "kt"
            | "kts"
            | "go"
            | "rb"
            | "php"
            | "cpp"
            | "cc"
            | "cxx"
            | "c"
            | "h"
            | "hpp"
            | "fs"
            | "fsx"
            | "vb"
            | "gradle"
    )
}

fn glob_match(pattern: &str, path: &str) -> bool {
    let pattern = normalize_path(pattern);
    let path = normalize_path(path);
    if pattern == path {
        return true;
    }
    let pattern_segments: Vec<&str> = pattern.split('/').collect();
    let path_segments: Vec<&str> = path.split('/').collect();
    match_segments(&pattern_segments, &path_segments)
}

fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
    if pattern.is_empty() {
        return path.is_empty();
    }
    if pattern[0] == "**" {
        return (0..=path.len()).any(|skip| match_segments(&pattern[1..], &path[skip..]));
    }
    if path.is_empty() {
        return false;
    }
    segment_match(pattern[0], path[0]) && match_segments(&pattern[1..], &path[1..])
}

fn segment_match(pattern: &str, text: &str) -> bool {
    let p = pattern.as_bytes();
    let s = text.as_bytes();
    let (mut pi, mut si) = (0usize, 0usize);
    let mut star: Option<usize> = None;
    let mut star_match = 0usize;

    while si < s.len() {
        if pi < p.len() && (p[pi] == b'?' || p[pi] == s[si]) {
            pi += 1;
            si += 1;
        } else if pi < p.len() && p[pi] == b'*' {
            star = Some(pi);
            pi += 1;
            star_match = si;
        } else if let Some(star_idx) = star {
            star_match += 1;
            si = star_match;
            pi = star_idx + 1;
        } else {
            return false;
        }
    }
    while pi < p.len() && p[pi] == b'*' {
        pi += 1;
    }
    pi == p.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_fallback_matches_rust() {
        let resolver = ReviewRuleResolver::default();
        let RuleDecision::Review(rule) = resolver.resolve("src-tauri/src/review/engine.rs", "edit")
        else {
            panic!("expected review");
        };
        assert_eq!(rule.source, "builtin");
        assert_eq!(rule.title, "Rust/Tauri backend checklist");
    }

    #[test]
    fn first_repo_match_wins() {
        let resolver = ReviewRuleResolver::new(Some(ReviewRuleConfig {
            include: vec![],
            exclude: vec![],
            default_rule: None,
            rules: vec![
                RepoRule {
                    path: "src/**/*.ts".into(),
                    title: "first".into(),
                    rule: "a".into(),
                },
                RepoRule {
                    path: "src/lib/*.ts".into(),
                    title: "second".into(),
                    rule: "b".into(),
                },
            ],
        }));
        let RuleDecision::Review(rule) = resolver.resolve("src/lib/api.ts", "edit") else {
            panic!("expected review");
        };
        assert_eq!(rule.title, "first");
    }

    #[test]
    fn repo_config_override_default_rule() {
        let resolver = ReviewRuleResolver::from_json(
            r#"{"defaultRule":{"title":"Repo default","rule":"repo-specific"}}"#,
        )
        .unwrap();
        let RuleDecision::Review(rule) = resolver.resolve("src-tauri/src/lib.rs", "edit") else {
            panic!("expected review");
        };
        assert_eq!(rule.source, "repo");
        assert_eq!(rule.title, "Repo default");
        assert_eq!(rule.rule, "repo-specific");
    }

    #[test]
    fn builtin_matches_python_csharp_and_arm_templates() {
        let resolver = ReviewRuleResolver::default();
        let cases = [
            ("src/service/app.py", "Python checklist"),
            ("src/Domain/OrderService.cs", "C# checklist"),
            ("src/App/App.csproj", ".NET project checklist"),
            ("infra/azuredeploy.json", "Azure ARM template checklist"),
            (
                "infra/prod.parameters.json",
                "Azure ARM parameters checklist",
            ),
            ("infra/main.bicep", "Azure Bicep checklist"),
        ];
        for (path, expected_title) in cases {
            let RuleDecision::Review(rule) = resolver.resolve(path, "edit") else {
                panic!("expected review for {path}");
            };
            assert_eq!(rule.title, expected_title);
        }
    }

    #[test]
    fn unknown_text_paths_get_generic_review() {
        let resolver = ReviewRuleResolver::default();
        let RuleDecision::Review(rule) = resolver.resolve("deploy/web.config", "edit") else {
            panic!("expected generic review");
        };
        assert_eq!(rule.title, "General code review checklist");
    }

    #[test]
    fn include_and_exclude_are_applied() {
        let resolver = ReviewRuleResolver::from_json(
            r#"{"include":["src/**"],"exclude":["src/generated/**"],"defaultRule":"review"}"#,
        )
        .unwrap();
        assert!(matches!(
            resolver.resolve("docs/readme.md", "edit"),
            RuleDecision::Skip { reason } if reason == "notIncluded"
        ));
        assert!(matches!(
            resolver.resolve("src/generated/client.ts", "edit"),
            RuleDecision::Skip { reason } if reason == "excluded"
        ));
    }

    #[test]
    fn deleted_binary_and_default_skips() {
        let resolver = ReviewRuleResolver::default();
        assert!(matches!(
            resolver.resolve("src/lib/api.ts", "delete"),
            RuleDecision::Skip { reason } if reason == "deleted"
        ));
        assert!(matches!(
            resolver.resolve("docs/image.png", "edit"),
            RuleDecision::Skip { reason } if reason == "binary"
        ));
        assert!(matches!(
            resolver.resolve("release.jks", "edit"),
            RuleDecision::Skip { reason } if reason == "binary"
        ));
    }
}
