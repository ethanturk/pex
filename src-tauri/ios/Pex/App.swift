import SwiftUI

/// ── App Entry Point ──────────────────────────────────────────────────────
/// Pex iOS — Azure DevOps PR Reviewer
///
/// ## Integration with Tauri
///
/// After running `tauri ios init` in src-tauri/, Tauri generates an Xcode
/// project with its own App/SceneDelegate and a WKWebView that hosts the
/// Preact frontend. This SwiftUI app REPLACES the default Tauri-generated
/// Swift entry point.
///
/// ## What Tauri provides
///
/// `tauri ios init` generates:
///   - `Pex.xcodeproj` / `Pex.xcworkspace`
///   - A bridging header for Rust FFI
///   - `libpex_lib.a` — the Rust backend compiled as a static library for
///     aarch64-apple-ios, linked into the Xcode target
///   - The Rust `run()` function callable from Swift via the bridging header
///
/// ## What this file does
///
/// This is the @main entry point. Instead of UIKit's AppDelegate lifecycle,
/// we use the SwiftUI App protocol (iOS 16+). The app presents a two-tab
/// interface:
///   - Tab 1: PRs — NavigationStack for the review workflow
///   - Tab 2: Settings — AI config, accounts, theme
///
/// Each tab hosts a WKWebView (via UIViewRepresentable) that loads the
/// Preact frontend from the Tauri bundle. The webview communicates with
/// the Rust backend via Tauri's `window.__TAURI__` bridge — no Swift ↔ JS
/// bridging needed for Tauri invoke() calls.
///
/// ## Setup checklist (on your Mac)
///
///   1. Install Xcode 15+ and iOS 16.0 simulator
///   2. `cd src-tauri && cargo install tauri-cli`
///   3. `tauri ios init` — generates the Xcode project
///   4. Replace the generated AppDelegate/SceneDelegate with this file
///      (or copy its contents into the generated structure)
///   5. Add `import SwiftUI` and target iOS 16.0 in project settings
///   6. Build & run: `tauri ios dev` (simulator) or open .xcodeproj in Xcode
///
/// ## Files to create (in this directory)
///
///   App.swift          ← this file
///   PRsTab.swift       ← NavigationStack for PR workflow
///   SettingsTab.swift   ← Settings view
///   WebView.swift      ← WKWebView wrapper (UIViewRepresentable)
///
/// ## Notes
///
///   - The tab bar is SwiftUI-native. The web frontend does NOT render
///     its own tab bar when running inside Tauri mobile — see
///     src/lib/platform.ts: isTauri() + isMobile() for detection.
///   - Settings shown in the Settings tab use the AiSettings component
///     in standalone mode (no modal backdrop).
///   - iPad gets the same two-tab layout — the web frontend handles
///     adaptive multi-panel layout via CSS breakpoints (768px+).
///   - Scene-based multi-window (Split View) is deferred to v2.
///     This app uses WindowGroup with a single window.
///
/// ## After copying into Xcode project
///
///   1. Remove any Tauri-generated AppDelegate/SceneDelegate .swift files
///      that conflict with the @main attribute.
///   2. Ensure the bridging header imports the Tauri/pex_lib symbols.
///   3. Call the Rust `run()` function during app init (Tauri's generated
///      code handles this — check the generated main.m / AppDelegate for
///      the exact symbol name, typically `run_pex` or similar).

@main
struct PexApp: App {
    /// Track which tab is selected. The webview in each tab is notified
    /// of tab switches so the Preact frontend can update its state.
    @State private var selectedTab = 0

    var body: some Scene {
        WindowGroup {
            TabView(selection: $selectedTab) {
                // ── Tab 1: PRs ────────────────────────────────────────
                PRsTabView()
                    .tabItem {
                        Label("PRs", systemImage: "doc.text.magnifyingglass")
                    }
                    .tag(0)

                // ── Tab 2: Settings ───────────────────────────────────
                SettingsTabView()
                    .tabItem {
                        Label("Settings", systemImage: "gearshape")
                    }
                    .tag(1)
            }
            // iOS 16+ TabView customization
            .tint(.accentColor)
        }
    }
}
