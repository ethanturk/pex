import SwiftUI
import WebKit

/// ── WKWebView wrapper for the Tauri frontend ──────────────────────────────
///
/// Bridges UIKit's WKWebView into SwiftUI via UIViewRepresentable.
/// The webview loads the bundled Preact frontend (index.html) and
/// communicates with the Rust backend through Tauri's JS bridge
/// (`window.__TAURI__`).
///
/// ## Integration with Tauri
///
/// After `tauri ios init`, Tauri generates its own WKWebView setup —
/// typically a UIViewController that:
///   1. Creates a WKWebViewConfiguration with Tauri's message handler
///   2. Calls the Rust `run()` function before loading
///   3. Loads `index.html` from the app bundle
///   4. Registers custom URL scheme handlers for Tauri IPC
///
/// If Tauri's generated code provides a ready-to-use WKWebView instance,
/// wrap it with:
///
///     struct TauriWebView: UIViewRepresentable {
///         let webView: WKWebView  // from Tauri's setup
///         func makeUIView(context: Context) -> WKWebView { webView }
///         func updateUIView(_ uiView: WKWebView, context: Context) {}
///     }
///
/// If you need to create the WKWebView from scratch (not using Tauri's
/// generated controller), follow the setup checklist below.

// MARK: - Full Custom Setup (if not using Tauri's generated controller)

struct TauriWebView: UIViewRepresentable {
    /// URL to the bundled index.html
    let bundleURL: URL

    /// Optional: initial route to load (e.g., "#/settings" for Settings tab)
    var initialRoute: String? = nil

    func makeCoordinator() -> Coordinator {
        Coordinator()
    }

    func makeUIView(context: Context) -> WKWebView {
        // ── 1. Configuration ───────────────────────────────────────────
        let config = WKWebViewConfiguration()

        // Allow file:// access from file:// origin (frontend is bundled)
        config.preferences.setValue(true, forKey: "allowFileAccessFromFileURLs")

        // Enable inline media playback (no fullscreen takeover)
        config.allowsInlineMediaPlayback = true

        // Disable user text selection for a more app-like feel
        // (optional — the Preact CSS already handles this for diff lines)

        // ── 2. Tauri message handler ───────────────────────────────────
        //
        // Tauri communicates via window.webkit.messageHandlers.<name>.postMessage().
        // Register the handler that Tauri's JS bridge expects.
        //
        // CHECK: what name does Tauri v2 use? Typically "tauri" or "ipc".
        // Look in the generated iOS code or Tauri's mobile docs.
        //
        // config.userContentController.add(context.coordinator, name: "ipc")

        // ── 3. Create webview ──────────────────────────────────────────
        let webView = WKWebView(frame: .zero, configuration: config)

        // iOS appearance
        webView.isOpaque = false
        webView.backgroundColor = .clear
        webView.scrollView.backgroundColor = .clear

        // Disable zoom / bounce for app-like feel
        webView.scrollView.bounces = false
        webView.scrollView.alwaysBounceVertical = false
        webView.scrollView.contentInsetAdjustmentBehavior = .never

        // Allow Tauri's JS bridge
        webView.configuration.preferences.setValue(true, forKey: "developerExtrasEnabled")

        // ── 4. Navigation delegate ─────────────────────────────────────
        webView.navigationDelegate = context.coordinator

        // ── 5. Load the frontend ───────────────────────────────────────
        var url = bundleURL
        if let route = initialRoute {
            // Append route as fragment: index.html#/settings
            url = bundleURL.appendingPathComponent("")
            // Note: file:// URLs don't support fragments the same way.
            // Use a query parameter instead if needed:
            // url = URL(string: bundleURL.absoluteString + "?" + route) ?? bundleURL
        }
        webView.loadFileURL(url, allowingReadAccessTo: url.deletingLastPathComponent())

        return webView
    }

    func updateUIView(_ webView: WKWebView, context: Context) {
        // Called when SwiftUI state changes. Use this to:
        //   - Switch tabs via evaluateJavaScript
        //   - Inject theme changes
        //   - Handle Tauri events from the Rust side
    }

    // ── Optional: dismantle (cleanup) ──────────────────────────────────
    static func dismantleUIView(_ webView: WKWebView, coordinator: Coordinator) {
        // Remove message handlers to prevent leaks
        // webView.configuration.userContentController.removeScriptMessageHandler(forName: "ipc")
    }
}

// MARK: - Coordinator (WKNavigationDelegate + WKScriptMessageHandler)

extension TauriWebView {
    class Coordinator: NSObject, WKNavigationDelegate {
        // ── WKNavigationDelegate ───────────────────────────────────────

        func webView(_ webView: WKWebView, didFinish navigation: WKNavigation!) {
            // Frontend loaded. Now you can:
            //   1. Inject any initial state (theme, active org, etc.)
            //   2. Notify the frontend which tab is active
            //
            // Example: inject dark mode preference
            // let js = """
            //   if (window.matchMedia('(prefers-color-scheme: dark)').matches) {
            //     document.documentElement.classList.add('dark');
            //   }
            // """
            // webView.evaluateJavaScript(js)
        }

        func webView(
            _ webView: WKWebView,
            decidePolicyFor navigationAction: WKNavigationAction,
            decisionHandler: @escaping (WKNavigationActionPolicy) -> Void
        ) {
            // Allow the Tauri IPC requests (custom scheme handlers)
            // Tauri uses a custom URL scheme (e.g., tauri:// or ipc://)
            // for Rust ↔ JS communication.
            //
            // CHECK: what scheme does Tauri v2 mobile use?
            // Look at the generated code or Tauri mobile docs.
            //
            // For now, allow everything:
            decisionHandler(.allow)
        }

        // ── WKScriptMessageHandler (if using Tauri IPC) ────────────────
        //
        // func userContentController(
        //     _ userContentController: WKUserContentController,
        //     didReceive message: WKScriptMessage
        // ) {
        //     // Forward to Tauri's Rust bridge
        //     // The exact handler depends on Tauri's generated code.
        // }
    }
}

// MARK: - Convenience initializer for bundled frontend

extension TauriWebView {
    /// Creates a webview that loads the frontend from the app bundle.
    ///
    /// After `tauri ios init`, the frontend is typically at:
    ///   `Bundle.main.url(forResource: "index", withExtension: "html",
    ///                    subdirectory: "dist")`
    ///
    /// Usage:
    ///     TauriWebView()
    init() {
        // Default: look for index.html in the bundle's "dist" directory
        // Adjust the subdirectory based on Tauri's bundle structure.
        let url = Bundle.main.url(
            forResource: "index",
            withExtension: "html",
            subdirectory: "dist"
        ) ?? Bundle.main.url(
            forResource: "index",
            withExtension: "html"
        )!
        self.init(bundleURL: url)
    }
}

// MARK: - Keyboard avoidance helper

extension TauriWebView {
    /// Call this from the tab view to enable keyboard avoidance.
    /// The webview respects safe areas and the keyboard inset automatically
    /// when `ignoresSafeArea(.keyboard)` is NOT applied.
    ///
    /// The Preact frontend also handles keyboard avoidance via
    /// `visualViewport` API (see DiffViewer.tsx). Both layers should work
    /// together — the native layer handles the tab bar, the JS layer
    /// scrolls the comment input into view.
}
