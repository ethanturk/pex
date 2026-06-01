# Pex — Agent Instructions

> Build and contribution guide for AI coding agents and human contributors.

## Project overview

Pex is a Tauri v2 desktop (and soon iOS) app for reviewing Azure DevOps pull requests.
- **Backend:** Rust (`src-tauri/src/`) — ADO API, AI review engine, diff rendering, SQLite cache
- **Frontend:** Preact + TypeScript + Tailwind CSS v4 (`src/`)
- **Desktop:** Tauri v2 (Linux, macOS, Windows) — `.deb`, `.rpm`, `.dmg`, `.msi`
- **iOS:** in progress on `iOS` branch (Swift + WKWebView, iOS 16.0+)

## Quick dev

```bash
cd ~/projects/pex
npm install
npm run tauri dev     # desktop
npm run build         # frontend only (for CI / verifying)
cd src-tauri && cargo build && cargo test --lib
```

## iOS development

The `iOS` branch adds an iOS target. The frontend is shared with desktop — only
the Swift shell and responsive CSS are iOS-specific.

### What's already done (on `iOS` branch)

- **Platform detection:** `src/lib/platform.ts` — `isTauri()`, `isMobile()`, `isIPad()`
- **Mobile CSS:** safe area insets, 44pt touch targets, iOS scroll, `viewport-fit=cover` in `index.html`
- **Adaptive app shell:** `MobileShell` in `src/app.tsx` — two-tab layout (PRs / Settings), bottom tab bar on mobile
- **File tree overlay:** sheet on iPhone (`< 768px`), permanent sidebar on iPad/desktop
- **Touch diff selection:** `DiffViewer.tsx` handles `touchstart/move/end` for line-range selection
- **Keyboard avoidance:** `visualViewport` API in `DiffViewer.tsx` scrolls the comment textarea into view
- **Swift templates:** `src-tauri/ios/Pex/` — heavily commented `.swift` files ready to copy into Xcode

### Swift templates

```
src-tauri/ios/Pex/
├── App.swift          # @main entry, two-tab TabView, iOS 16+
├── PRsTab.swift       # Single persistent webview for review workflow
├── SettingsTab.swift  # Settings tab (shared webview, tab communication notes)
└── WebView.swift      # UIViewRepresentable WKWebView wrapper with full setup checklist
```

All four files are design documents in comments with placeholder implementations.
They compile as-is for preview but need Tauri-specific symbols filled in.

### Setup checklist (on a Mac)

1. **Install prerequisites**
   ```bash
   # Xcode 15+ (App Store)
   # Rust iOS target
   rustup target add aarch64-apple-ios
   ```

2. **Generate Xcode project**
   ```bash
   cd src-tauri
   cargo install tauri-cli    # if not already installed
   tauri ios init
   ```
   This creates `src-tauri/ios/Pex.xcodeproj` (or `.xcworkspace`) with:
   - Bridging header for Rust FFI (`libpex_lib.a` linked as a static library)
   - Default AppDelegate/SceneDelegate (UIKit lifecycle)
   - Build phases for compiling the Rust lib and bundling the frontend

3. **Replace the generated Swift entry point**
   - Open the generated Xcode project
   - Remove any auto-generated AppDelegate / SceneDelegate Swift files
   - Copy the four Swift files from `src-tauri/ios/Pex/` into the Xcode project
   - Ensure `PexApp.swift` has the `@main` attribute (only one `@main` per target)

4. **Fill in Tauri-specific symbols**
   Open `WebView.swift` and fill in every `// CHECK:` comment:
   - **Message handler name:** Tauri's JS bridge posts messages via
     `window.webkit.messageHandlers.<NAME>.postMessage()`. Check Tauri v2
     mobile docs or the generated code for the exact name (likely `"ipc"`).
   - **Custom URL scheme:** Tauri uses a custom scheme (e.g., `tauri://`)
     for Rust ↔ JS IPC. Register it in the WKNavigationDelegate handler.
   - **Bundle path:** The frontend is bundled at `dist/index.html`. Verify the
     `Bundle.main.url(forResource:subdirectory:)` path matches the Xcode
     build phase output.
   - **Rust `run()` symbol:** Tauri's generated `main.m` calls the Rust entry
     point. The symbol name is typically in the bridging header. Make sure
     the Rust lib initializes before the webview loads.

5. **Set iOS deployment target**
   - Xcode → Project → Info → Deployment Target → iOS 16.0
   - This gives us `NavigationStack` (avoids the deprecated `NavigationView`)
     and covers ~90% of devices.

6. **Build & run**
   ```bash
   tauri ios dev          # builds Rust lib + frontend, launches Simulator
   # Or open .xcodeproj in Xcode and press ⌘R
   ```

### Known integration points (things that need wiring)

| Component | Desktop (Linux/macOS/Windows) | iOS |
|-----------|------------------------------|-----|
| **Tauri shell** | System webview (WebKitGTK / WKWebView / WebView2) | WKWebView via SwiftUI |
| **Entry point** | `src-tauri/src/main.rs` → `lib.rs::run()` | `#[cfg_attr(mobile, tauri::mobile_entry_point)]` in `lib.rs` |
| **Frontend entry** | `src/main.tsx` renders `<App />` | Same `main.tsx` — platform detection switches shell |
| **Tab bar** | None (header nav) | SwiftUI `TabView` → tells webview which tab is active |
| **Keyboard avoidance** | Not needed (physical keyboard) | `visualViewport` in JS + Swift `ignoresSafeArea(.keyboard)` |
| **Credential storage** | OS keyring (`keyring` crate) | iOS Keychain (same crate, uses Security.framework) |
| **Auth** | PAT text input + OAuth browser window | PAT text input only (OAuth deferred) |
| **File system** | `~/.config/pex/` (config), `~/.local/share/pex/` (cache) | App sandbox (`Library/Application Support/`) |
| **Network** | reqwest (native TLS) | reqwest (same, uses Security.framework TLS) |
| **Diff cache** | SQLite at `~/.local/share/pex/pex.db` | SQLite at sandboxed Library path |

### Package structure

```
pex/
├── src/                          # Shared Preact frontend
│   ├── app.tsx                   # DesktopShell + MobileShell (platform-aware)
│   ├── components/               # Auth, PR list, diff viewer, AI review, etc.
│   ├── lib/
│   │   ├── api.ts                # Tauri invoke() wrappers for all Rust commands
│   │   ├── signals.ts            # Preact signals (currentView, activeOrg, etc.)
│   │   ├── platform.ts           # isTauri(), isMobile(), isIPad()
│   │   └── hooks.ts             # useEffectOnce helper
│   └── styles/global.css        # Tailwind v4 + diff styling + mobile CSS
├── src-tauri/
│   ├── src/
│   │   ├── lib.rs                # AppState, error types, run()
│   │   ├── main.rs               # Desktop entry point
│   │   ├── commands/             # Tauri commands (auth, ai, review, etc.)
│   │   ├── ado/                  # Azure DevOps REST API client
│   │   ├── ai/                   # AI provider trait + OpenAI/Anthropic impls
│   │   ├── review/               # Multi-pass PR review engine
│   │   ├── diff/                 # Diff engine (similar + syntect)
│   │   ├── auth/                 # PAT + OAuth login
│   │   └── cache/                # SQLite diff cache
│   ├── ios/Pex/                  # Swift shell templates (iOS branch)
│   ├── tauri.conf.json           # Tauri config (all platforms)
│   └── Cargo.toml                # Rust dependencies
├── .hermes/plans/                # Implementation plans
├── index.html                    # Vite entry point
├── vite.config.ts
├── package.json
└── tsconfig.json
```

### Build verification

```bash
# Frontend
npm run build          # tsc + vite → dist/

# Rust
cd src-tauri && cargo build && cargo test --lib   # 18 tests

# iOS app (macOS only)
tauri ios build        # produces .ipa for App Store / TestFlight
```
