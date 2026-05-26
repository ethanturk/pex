import SwiftUI

/// ── Settings Tab ──────────────────────────────────────────────────────────
///
/// The Settings tab shows AI configuration. On desktop, this is a modal
/// dialog. On mobile, it's a dedicated tab — no backdrop, no close button.
///
/// ## How it works
///
/// The Preact AiSettings component is rendered in the webview. The frontend
/// detects that it's running in the Settings tab context and renders in
/// "standalone" mode (no modal overlay).
///
/// The tab communicates the active state to the webview via JavaScript.
/// When the user switches to the Settings tab, we evaluate JS in the
/// webview to tell the frontend to show the AI settings view.
///
/// ## Tab communication
///
/// Two approaches (pick one):
///
///   A) **Single shared webview** — both tabs share one WKWebView instance.
///      SwiftUI just shows the same view. The frontend reads which tab is
///      active and switches its internal state accordingly. Simpler, less
///      memory, but the webview state persists across tabs (Settings form
///      stays filled in when switching back to PRs).
///
///   B) **Separate webviews per tab** — each tab has its own WKWebView.
///      More memory (~2×), but complete state isolation. The Settings tab
///      loads a different route or passes a query param.
///
///   RECOMMENDED: Start with (A) — single webview. The web frontend's
///   MobileShell already handles tab switching (src/app.tsx). The Swift
///   tab bar and the Preact tab bar are separate — when the user taps a
///   Swift tab, we tell the webview to switch. When the webview's internal
///   state changes, we update the Swift tab selection.
///
/// ## Implementation for approach A
///
///   1. Create a shared WKWebView in App.swift (or a ViewModel)
///   2. Pass it to both PRsTabView and SettingsTabView
///   3. On tab change, call webView.evaluateJavaScript(...) to notify
///      the frontend
///   4. The frontend's platform.ts detects Tauri + mobile and enables
///      tab-aware routing
///
///   The JS to switch to Settings tab:
///     window.__tauri_mobile_tab__ = "settings"
///     window.dispatchEvent(new Event("tauri-tab-change"))
///
///   See the mobile app shell (src/app.tsx: MobileShell) for the
///   corresponding frontend handler.
///
/// ## For now
///
/// This view is a PLACEHOLDER. After setting up the Tauri webview:
///   1. Choose approach A or B above
///   2. Replace the placeholder with the webview
///   3. Wire up tab change communication

struct SettingsTabView: View {
    var body: some View {
        // ── Placeholder — replace with webview ─────────────────────────
        //
        // Option A (shared webview):
        //   let webView: WKWebView
        //   WebViewRepresentable(webView: webView)
        //     .onAppear {
        //         webView.evaluateJavaScript(
        //             "window.__tauri_mobile_tab__ = 'settings';" +
        //             "window.dispatchEvent(new Event('tauri-tab-change'))"
        //         )
        //     }
        //
        // Option B (separate webview):
        //   TauriWebView(url: bundledURL, initialRoute: "/settings")

        PlaceholderSettingsView()
            .ignoresSafeArea(.keyboard)
    }
}

// ── Placeholder — delete after hooking up webview ─────────────────────────

struct PlaceholderSettingsView: View {
    var body: some View {
        VStack(spacing: 24) {
            Image(systemName: "gearshape")
                .font(.system(size: 48))
                .foregroundColor(.accentColor)

            Text("Settings")
                .font(.title)
                .fontWeight(.bold)

            VStack(alignment: .leading, spacing: 8) {
                Label("AI Provider Configuration", systemImage: "brain.head.profile")
                Label("Customize Review Prompts", systemImage: "text.alignleft")
                Label("Account Management", systemImage: "person.circle")
            }
            .font(.caption)
            .padding()
            .background(.regularMaterial)
            .clipShape(RoundedRectangle(cornerRadius: 12))
        }
    }
}

#Preview {
    SettingsTabView()
}
