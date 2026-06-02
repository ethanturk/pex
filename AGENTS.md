# Pex — Agent Instructions

> Build and contribution guide for AI coding agents and human contributors.

## Project overview

Pex is a Tauri v2 desktop (and soon mobile) app for reviewing Azure DevOps pull requests.
- **Backend:** Rust (`src-tauri/src/`) — ADO API, AI review engine, diff rendering, SQLite cache
- **Frontend:** Preact + TypeScript + Tailwind CSS v4 (`src/`)
- **Desktop:** Tauri v2 (Linux, macOS, Windows) — `.deb`, `.rpm`, `.dmg`, `.msi`
- **iOS:** in progress (Swift + WKWebView, iOS 16.0+)
- **Android:** in progress (Kotlin + Jetpack Compose + WebView, API 24+)

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

## Android development

Android mirrors the iOS target: the frontend is shared with desktop/iOS — only
the Kotlin shell and the responsive CSS (already in place) are platform-specific.
The Rust backend already builds for Android via the `mobile` cfg (the same
`#[cfg_attr(mobile, tauri::mobile_entry_point)]` covers iOS and Android).

### What's already done

- **Platform detection:** `src/lib/platform.ts` — `isMobile()` is generic
  (touch + narrow viewport), so it already returns `mobile` on Android phones.
  No iOS-only assumptions; the same `MobileShell` renders.
- **Mobile CSS:** safe-area insets, 44pt/48dp touch targets, momentum scroll,
  `viewport-fit=cover` — all platform-neutral.
- **Launcher icons:** `src-tauri/icons/android/` (mipmap densities) already exist.
- **Kotlin templates:** `src-tauri/android/Pex/` — heavily commented `.kt` files
  ready to copy into the generated Android Studio project.

### Kotlin templates

```
src-tauri/android/Pex/
├── MainActivity.kt   # ComponentActivity entry, Compose two-tab bottom nav, API 24+
├── PRsTab.kt         # Single persistent WebView for the review workflow
├── SettingsTab.kt    # Settings tab (shared webview, tab communication notes)
└── WebView.kt        # AndroidView WebView wrapper with full wry/Tauri setup checklist
```

All four files are design documents in comments with placeholder
implementations — the Kotlin analogue of the Swift templates. They compile
as-is but need wry/Tauri-specific symbols filled in (IPC interface name,
asset-loader origin, library name).

### Setup checklist

1. **Install prerequisites**
   ```bash
   # Android Studio + SDK Platform 34 + NDK + an emulator image
   export ANDROID_HOME=$HOME/Android/Sdk
   export NDK_HOME=$ANDROID_HOME/ndk/<version>
   # Rust Android targets
   rustup target add aarch64-linux-android armv7-linux-androideabi \
       i686-linux-android x86_64-linux-android
   ```

2. **Generate the Gradle project**
   ```bash
   cd src-tauri
   cargo install tauri-cli    # if not already installed
   tauri android init
   ```
   This creates `src-tauri/gen/android/` (a Gradle/Android Studio project) with:
   - A generated `MainActivity.kt` extending Tauri's `TauriActivity` (wry's
     `WryActivity`), in package `com.pex.pr_reviewer` (derived from the
     `identifier` `com.pex.pr-reviewer`, `-` → `_`)
   - `libpex_lib.so` built per-ABI via `cargo-ndk` and packaged into `jniLibs/`
   - Gradle tasks that compile the Rust lib and bundle the frontend assets

3. **Apply Pex's Android customizations** (icons + manifest)
   ```bash
   npm run android:setup        # = src-tauri/scripts/android-setup.sh
   ```
   `gen/android` is **.gitignored** (it's generated and ~1 GB with build
   artifacts), so its customizations must be reproducible from source. This
   idempotent script:
   - Installs the branded launcher icons from `src-tauri/icons/android/` (the
     committed source of truth — a full adaptive-icon set). To refresh that set
     from a new brand image, run `cargo tauri icon <1024px.png>` and re-commit
     `icons/android/`. (`cargo tauri icon` also writes mipmaps straight into
     `gen/android`, but without the adaptive `mipmap-anydpi-v26` XML, so we keep
     `icons/android/` as the canonical set and copy it in.)
   - Sets `android:windowSoftInputMode="adjustResize"` on the launcher
     `<activity>` so the WebView resizes for the soft keyboard (paired with the
     `visualViewport` handling in `DiffViewer.tsx`). Tauri has no
     tauri.conf.json field for this, hence the manifest patch.

   > **Entry point:** keep the generated `MainActivity : TauriActivity`. wry
   > already wires the WebView, the `__TAURI_IPC__` bridge, the
   > `WebViewAssetLoader` origin (`https://tauri.localhost/`), and
   > `System.loadLibrary("pex_lib")`. The Compose templates in
   > `src-tauri/android/Pex/` are **superseded** by the shared JS `MobileShell`
   > (two-tab PRs/Settings layout) and are not copied in — doing so would
   > replace the working webview with placeholder code.

4. **Min SDK** — `tauri android init` already sets `minSdk = 24` in
   `app/build.gradle.kts` (Android 7.0; covers `WebViewAssetLoader` and ~98% of
   devices). No action needed.

5. **Build & run**
   ```bash
   source .android-env.sh   # ANDROID_HOME / NDK_HOME / JDK 17 (NOT system JDK 25)
   tauri android dev        # builds Rust .so + frontend, launches emulator/device
   # Or open src-tauri/gen/android in Android Studio and press Run
   ```
   The credential store, SQLite path, and JavaVM access are all handled in the
   Rust backend (see `auth::android_keystore`, `cache::dirs_data_dir`, and the
   `JNI_OnLoad` hook) — no manual wiring required.

### Known integration points (things that need wiring)

| Component | iOS | Android |
|-----------|-----|---------|
| **Tauri shell** | WKWebView via SwiftUI | Android `WebView` via Compose `AndroidView` |
| **Entry point** | SwiftUI `@main App` | Compose `ComponentActivity` / generated `TauriActivity` |
| **Tab bar** | SwiftUI `TabView` | Compose `NavigationBar` (bottom nav) |
| **Asset loading** | `loadFileURL` from bundle | `WebViewAssetLoader` @ `https://tauri.localhost` |
| **IPC bridge** | `WKScriptMessageHandler` | `@JavascriptInterface __TAURI_IPC__` |
| **Credential storage** | iOS Keychain (`keyring` crate) | needs an Android keystore path — the `keyring` crate has no Android backend; wire `tauri-plugin-stronghold` or the Android Keystore (deferred) |
| **Keyboard avoidance** | `ignoresSafeArea(.keyboard)` + `visualViewport` | `windowSoftInputMode=adjustResize` + `visualViewport` |
| **Rust lib** | `libpex_lib.a` (static, aarch64-apple-ios) | `libpex_lib.so` (per-ABI, via cargo-ndk) |

> **Note on credentials:** the desktop/iOS `keyring` features
> (`apple-native`, `windows-native`, `sync-secret-service`) do not include an
> Android backend. Before shipping Android, swap to the Android Keystore (e.g.
> via a Tauri plugin) or guard the keyring calls behind `#[cfg(not(target_os =
> "android"))]` and use an encrypted SQLite/Stronghold fallback.

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
│   ├── ios/Pex/                  # Swift shell templates (iOS)
│   ├── android/Pex/              # Kotlin shell templates (Android)
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

# Android app (needs Android SDK + NDK)
tauri android build    # produces .apk / .aab for Play Store
```
