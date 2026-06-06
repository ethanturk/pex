# Deterministic Review Rules — Working Reference

Source: `src-tauri/src/review/rules.rs` (deterministic preflight, merged Jun 5 2026).
This is a hand-maintained mirror for planning/editing. The compiled-in source of
truth is `builtin_rules()`, `builtin_excludes()`, `is_binary_path()`, and
`is_text_reviewable_path()`.

## Resolution order (`ReviewRuleResolver::resolve`, rules.rs:60)

1. `status == "delete"` → **Skip** `deleted`
2. binary extension → **Skip** `binary`
3. **repo config** (`.pex/review-rules.json`), if present:
   - matches `exclude` glob → Skip `excluded`
   - `include` non-empty and no match → Skip `notIncluded`
   - first matching `rules[].path` → Review (source `repo`)
   - else `defaultRule` → Review (source `repo`)
4. built-in excludes → Skip `excluded`
5. first matching built-in rule → Review (source `builtin`)
6. text-reviewable path → Review, "General code review checklist"
7. otherwise → Skip `unsupportedPath`

First match wins within each list; repo config always beats built-ins.

## Built-in rules (`builtin_rules()`, rules.rs:179) — first match wins

| # | Glob | Title | Checklist |
|---|------|-------|-----------|
| 1 | `src-tauri/src/**/*.rs` | Rust/Tauri backend | async error propagation, provider abstraction boundaries, cancellation, cache consistency, serde compatibility, platform cfg gates, ADO/GitHub API behavior |
| 2 | `**/*.py` | Python | runtime errors, exception handling, async/resource lifetimes, data validation, security-sensitive input handling, type assumptions, dependency behavior, missing tests |
| 3 | `**/*.pyi` | Python typing | type signatures, optional/nullability semantics, overload consistency, API compatibility, impl↔stub alignment |
| 4 | `**/*.cs` | C# | nullability, async/await and cancellation, disposal/lifetime ownership, LINQ/query behavior, exception handling, authorization/data validation, DI boundaries, test coverage |
| 5 | `**/*.csproj` | .NET project | target frameworks, package versions, build properties, analyzers, generated assets, publish settings, CI/deployment compatibility |
| 6 | `**/*.sln` | .NET solution | project membership, configuration/platform mappings, dependency ordering, intended build/test scope |
| 7 | `**/*.props` | .NET MSBuild | shared build properties, package/version propagation, conditional logic, target framework compatibility, CI/build side effects |
| 8 | `**/*.targets` | .NET MSBuild | target ordering, incremental build behavior, generated outputs, packaging side effects, platform-specific conditions |
| 9 | `**/azuredeploy*.json` | Azure ARM template | resource API versions, parameter defaults, secureString/secret handling, dependencies, idempotency, naming/location expressions, RBAC/network exposure, deployment-mode compatibility |
| 10 | `**/*.parameters.json` | Azure ARM parameters | parameter names/types match templates, no hard-coded secrets, intentional env-specific values, safe deployment defaults |
| 11 | `**/*.template.json` | Azure ARM template | (same as #9) |
| 12 | `**/*.bicep` | Azure Bicep | resource API versions, module/parameter contracts, secure parameters, dependencies, scopes, RBAC/network exposure, generated ARM behavior |
| 13 | `**/*.bicepparam` | Azure Bicep parameters | parameter names/types match the Bicep template, secret handling, env-specific values, deployment safety |
| 14 | `src/**/*.tsx` | Preact UI | state/signal ownership, async loading and cancellation, mobile touch ergonomics, accessibility labels, text overflow, dark-mode Tailwind classes |
| 15 | `src/**/*.ts` | TypeScript frontend | API type parity with Tauri commands, signal updates, error handling, local-storage compatibility, mobile/desktop platform branching |
| 16 | `src/styles/**/*.css` | Responsive CSS | mobile safe areas, touch targets, text overflow, dark mode, consistency with the app's utilitarian review workflow |
| 17 | `src-tauri/tauri.conf.json` | Tauri configuration | bundle identifiers, platform config, permissions, updater settings, desktop/mobile target compatibility |
| 18 | `src-tauri/android/**/*.kt` | Android shell | WebView lifecycle, asset loading, IPC bridge assumptions, keyboard resizing, API level constraints, Rust library loading |
| 19 | `src-tauri/ios/**/*.swift` | iOS shell | WKWebView lifecycle, safe areas, Tauri IPC bridge assumptions, bundle paths, keychain behavior, iOS deployment target compatibility |
| 20 | `**/*.toml` | Manifest | dependency feature flags, platform-specific dependencies, package metadata, CI/build compatibility |
| 21 | `**/*.json` | Configuration | schema compatibility, generated-vs-source ownership, platform-specific fields, release/build impact |
| 22 | `**/*.yml` | Workflow | secret handling, reproducibility, platform setup, artifact paths, release trigger behavior |
| 23 | `**/*.yaml` | Workflow | (same as #22) |
| 24 | `**/*.md` | Documentation | instructions actionable, current with referenced code paths, no conflict with platform-specific setup |
| 25 | `**/*.sh` | Shell script | idempotency, quoting, platform assumptions, exit behavior, generated-file ownership |
| 26 | `**/*.ps1` | PowerShell script | idempotency, quoting/escaping, error action behavior, platform assumptions, secret handling, deployment side effects |

## Generic fallback (rules.rs:141)

If no built-in rule matches but the path is "text-reviewable":

- **Title:** General code review checklist
- **Checklist:** changed behavior, error handling, security and data validation, integration boundaries, maintainability, whether tests or deployment updates are needed

## Built-in excludes (`builtin_excludes()`, rules.rs:165) → Skip `excluded`

`dist/**`, `target/**`, `node_modules/**`, `src-tauri/gen/**`, `**/*.lock`,
`**/package-lock.json`, `**/Cargo.lock`, `**/*.min.js`, `**/*.snap`

## Binary extensions (`is_binary_path()`, rules.rs:314) → Skip `binary`

png, jpg, jpeg, gif, webp, ico, icns, pdf, zip, gz, tgz, jks, aab, apk, dmg,
msi, dll, so, dylib, a, wasm, woff, woff2, ttf, otf

## Text-reviewable fallback set (`is_text_reviewable_path()`, rules.rs:347)

- **Exact filenames:** Dockerfile, Makefile, Rakefile, Gemfile, Procfile
- **Extensions:** txt, config, conf, ini, env, xml, props, targets, cshtml,
  razor, sql, ps1, psm1, psd1, bat, cmd, java, kt, kts, go, rb, php, cpp, cc,
  cxx, c, h, hpp, fs, fsx, vb, gradle

## Skip reasons (enum-ish strings)

`deleted` · `binary` · `excluded` · `notIncluded` · `unsupportedPath`

## Repo override schema (`.pex/review-rules.json`, ReviewRuleConfig rules.rs:12)

```json
{
  "include": ["src/**"],
  "exclude": ["src/generated/**"],
  "defaultRule": { "title": "Repo default", "rule": "..." },
  "rules": [
    { "path": "src/**/*.ts", "title": "...", "rule": "..." }
  ]
}
```

`defaultRule` may also be a bare string. Repo `rules` are first-match-wins and
take precedence over all built-ins.

## Coverage gaps / observations (for discussion)

- **No dedicated rule** for: `.js`/`.jsx`/`.mjs`/`.cjs`, `.go` (only via generic
  fallback), `.java`/`.kt`/`.rb`/`.php`/`.cpp`/`.c` (generic fallback only),
  `.sql`, `.html`, `.scss`/`.less`, `.proto`, `.graphql`, Dockerfile (generic).
- `**/*.json` (#21) is broad and sits after the Azure-specific JSON rules, so
  ordering matters if new JSON rules are added.
- CSS rule is scoped to `src/styles/**` only; CSS elsewhere falls through.
- `.kt` matches both the Android shell rule (#18, path-scoped) and the generic
  fallback; outside `src-tauri/android/**` it gets the generic checklist.
