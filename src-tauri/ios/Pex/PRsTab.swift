import SwiftUI

/// ── PRs Tab — Review Workflow ────────────────────────────────────────────
///
/// This tab hosts the main PR review workflow in a WKWebView. The Preact
/// frontend handles all navigation internally (Auth → Org Select → PR List →
/// PR Detail) via its `currentView` signal. SwiftUI does NOT manage the
/// view stack — it's a single, persistent webview.
///
/// ## Design rationale
///
/// We use a single webview (not NavigationStack with multiple webviews)
/// because:
///   - The Preact SPA already handles view transitions (currentView signal)
///   - State (active org, selected PR, loaded diffs) is shared across views
///   - Multiple webviews would each have their own JS context and would
///     need to re-fetch all data on navigation
///   - Tauri's `window.__TAURI__` bridge is per-webview — splitting views
///     would require IPC between webviews
///
/// ## Tauri webview loading
///
/// After `tauri ios init`, Tauri sets up a WKWebView that loads the
/// bundled frontend from the app bundle. The exact loading mechanism
/// depends on Tauri's generated code. Typically:
///   - Tauri creates a WKWebViewConfiguration with a custom message handler
///   - The webview loads `index.html` from the app bundle
///   - The Rust backend is initialized before the webview loads
///
/// ## What SwiftUI provides
///
///   - The tab bar item label and icon
///   - Containment (the webview fills the available space)
///   - Safe area insets are handled by SwiftUI automatically
///
/// ## Future enhancement (v2)
///
/// If we want native push transitions (swipe-back gesture, etc.), we
/// could use NavigationStack with programmatic navigation and communicate
/// view changes to the webview via JavaScript evaluation. This is deferred
/// — the SPA-based navigation works well and feels native enough.

struct PRsTabView: View {
    var body: some View {
        // ── Replace this with the Tauri WKWebView ──────────────────────
        //
        // After `tauri ios init`:
        //   1. Find the generated WKWebView wrapper (likely in
        //      `ios/Pex/WebView.swift` or similar)
        //   2. Replace `PlaceholderWebView()` below with it:
        //
        //      TauriWebView(url: /* bundled index.html */)
        //
        //   3. If Tauri's generated code uses UIKit (UIViewController),
        //      wrap it with UIViewControllerRepresentable.
        //
        // See WebView.swift in this directory for the wrapper template.

        PlaceholderWebView()
            .ignoresSafeArea(.keyboard) // Preact handles keyboard avoidance
    }
}

// ── Placeholder — replace with real Tauri webview ──────────────────────────

/// Temporary placeholder. DELETE this after hooking up the real Tauri webview.
/// Shows a brief instruction card in the simulator while setting up.
struct PlaceholderWebView: View {
    var body: some View {
        VStack(spacing: 24) {
            Image(systemName: "doc.text.magnifyingglass")
                .font(.system(size: 48))
                .foregroundColor(.accentColor)

            Text("Pex iOS")
                .font(.title)
                .fontWeight(.bold)

            Text("PR Review for Azure DevOps")
                .font(.subheadline)
                .foregroundColor(.secondary)

            VStack(alignment: .leading, spacing: 8) {
                Label("Load the Tauri WebView", systemImage: "1.circle.fill")
                    .font(.caption)
                Label("Replace this placeholder", systemImage: "2.circle.fill")
                    .font(.caption)
                Label("See WebView.swift for the wrapper", systemImage: "3.circle.fill")
                    .font(.caption)
            }
            .padding()
            .background(.regularMaterial)
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }
}

#Preview {
    PRsTabView()
}
