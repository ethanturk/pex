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
| 16 | `**/*.tsx` | React/JSX UI | component state and effect cleanup, async loading and cancellation, prop and key correctness, accessibility labels, escaping of untrusted data, type parity with backend contracts |
| 17 | `**/*.ts` | TypeScript | type soundness and any/unknown usage, async error handling, module import/export correctness, API/contract compatibility, data validation, build/tooling config impact |
| 18 | `**/*.jsx` | JavaScript/JSX UI | component state and effect cleanup, async loading and cancellation, prop and key correctness, accessibility labels, event handler wiring, escaping of untrusted data |
| 19 | `**/*.js` | JavaScript | runtime errors and null/undefined handling, async/promise error propagation, module format and import/export correctness, input validation and security-sensitive handling, dependency behavior, browser-vs-Node assumptions, missing tests |
| 20 | `**/*.mjs` | JavaScript | (same as #19, ESM import/export) |
| 21 | `**/*.cjs` | JavaScript | (same as #19, CommonJS require/export) |
| 22 | `src/styles/**/*.css` | Responsive CSS | mobile safe areas, touch targets, text overflow, dark mode, consistency with the app's utilitarian review workflow |
| 23 | `src-tauri/tauri.conf.json` | Tauri configuration | bundle identifiers, platform config, permissions, updater settings, desktop/mobile target compatibility |
| 24 | `src-tauri/android/**/*.kt` | Android shell | WebView lifecycle, asset loading, IPC bridge assumptions, keyboard resizing, API level constraints, Rust library loading |
| 25 | `src-tauri/ios/**/*.swift` | iOS shell | WKWebView lifecycle, safe areas, Tauri IPC bridge assumptions, bundle paths, keychain behavior, iOS deployment target compatibility |
| 26 | `**/*.toml` | Manifest | dependency feature flags, platform-specific dependencies, package metadata, CI/build compatibility |
| 27 | `**/*.json` | Configuration | schema compatibility, generated-vs-source ownership, platform-specific fields, release/build impact |
| 28 | `**/*.yml` | Workflow | secret handling, reproducibility, platform setup, artifact paths, release trigger behavior |
| 29 | `**/*.yaml` | Workflow | (same as #28) |
| 30 | `**/*.md` | Documentation | instructions actionable, current with referenced code paths, no conflict with platform-specific setup |
| 31 | `**/*.sh` | Shell script | idempotency, quoting, platform assumptions, exit behavior, generated-file ownership |
| 32 | `**/*.ps1` | PowerShell script | idempotency, quoting/escaping, error action behavior, platform assumptions, secret handling, deployment side effects |

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

## Two distinct layers (don't conflate them)

1. **Deterministic preflight** (`rules.rs`, `related.rs`, `anchoring.rs`) — the
   rules above. These are deterministic *scoping and guidance*: they pick which
   files to review and what checklist text to hand the LLM. They do **not**
   produce findings; the findings come from the LLM.
2. **Deterministic findings** (`deterministic.rs`) — tree-sitter AST checks that
   produce real findings with **no LLM**, scoped to lines the diff added. These
   are reproducible run-to-run and run in both Fast and Thorough modes. They
   merge into the same pipeline as the LLM findings and are tagged
   `sources: ["deterministic:<rule-id>"]` (shown with a "rule" badge in the UI).

### Deterministic AST rules (`deterministic.rs`) — v1

| Lang | Rule id | Severity | Flags |
|------|---------|----------|-------|
| Rust | `rust-unwrap` | minor | `.unwrap()`/`.expect()` (skipped in tests) |
| Rust | `rust-dbg` | minor | `dbg!` |
| Rust | `rust-todo` | moderate | `todo!`/`unimplemented!` |
| TS/TSX | `ts-console` | minor | `console.log`/`console.debug` (skipped in tests) |
| TS/TSX | `ts-debugger` | moderate | `debugger` |
| TS/TSX | `ts-any` | minor | `any` type |
| TS/TSX | `ts-ignore` | moderate | `@ts-ignore`/`@ts-nocheck` |
| Python | `py-bare-except` | moderate | bare `except:` |
| Python | `py-eval-exec` | critical | `eval()`/`exec()` |
| Python | `py-print` | minor | `print()` (skipped in tests) |

Findings only fire when the matched AST node overlaps an **added** line, so
pre-existing issues aren't flagged. New languages/rules are added by extending
the per-language rule tables.

## Coverage gaps / observations (for discussion)

- JS/TS gap **closed**: `.ts`/`.tsx` now covered everywhere (rules #16–#17,
  after the `src/**` frontend rules #14–#15 which keep precedence), and
  `.js`/`.jsx`/`.mjs`/`.cjs` are now first-class (#18–#21) instead of being
  skipped as `unsupportedPath`.
- **Still no dedicated rule** for: `.go`, `.java`/`.kt`/`.rb`/`.php`/`.cpp`/`.c`
  (generic fallback only), `.sql`, `.html`, `.scss`/`.less`, `.proto`,
  `.graphql`, Dockerfile (generic).
- `**/*.json` (#27) is broad and sits after the Azure-specific JSON rules, so
  ordering matters if new JSON rules are added.
- CSS rule is scoped to `src/styles/**` only; CSS elsewhere falls through.
- `.kt` matches both the Android shell rule (#24, path-scoped) and the generic
  fallback; outside `src-tauri/android/**` it gets the generic checklist.
