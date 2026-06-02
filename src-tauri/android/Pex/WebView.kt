package com.pex.pr_reviewer

import android.annotation.SuppressLint
import android.graphics.Color
import android.webkit.WebView
import android.webkit.WebViewClient
import androidx.compose.runtime.Composable
import androidx.compose.ui.Modifier
import androidx.compose.ui.viewinterop.AndroidView

/// ── WebView wrapper for the Tauri frontend ────────────────────────────────
///
/// Bridges Android's `WebView` into Compose via `AndroidView`. The webview
/// loads the bundled Preact frontend (index.html, served from app assets)
/// and communicates with the Rust backend through Tauri's JS bridge
/// (`window.__TAURI__`).
///
/// ## Integration with Tauri
///
/// After `tauri android init`, Tauri (via wry) generates its own `WebView`
/// setup — a `RustWebView` configured inside `TauriActivity` that:
///   1. Registers a `WebViewAssetLoader` mapping `https://tauri.localhost/*`
///      to the bundled frontend assets
///   2. Loads the Rust shared library and calls `run()` before navigating
///   3. Loads `https://tauri.localhost/index.html`
///   4. Installs the IPC bridge: a `@JavascriptInterface` object named
///      `__TAURI_IPC__` (wry's `ipc.postMessage`) that forwards messages to
///      the Rust side
///
/// If Tauri's generated code exposes a ready-to-use `WebView` (the
/// `RustWebView`), prefer reusing it rather than building a fresh one — wry
/// has already wired up the asset loader, IPC interface, and custom scheme.
/// Hand that instance to `AndroidView(factory = { rustWebView })`.
///
/// If you need to build the `WebView` from scratch (not using wry's
/// `RustWebView`), follow the setup checklist below. Note: a hand-rolled
/// WebView will NOT have the Tauri `invoke()` bridge unless you replicate
/// wry's asset loader + `__TAURI_IPC__` interface, so this path is only for
/// experimentation.

// MARK: - Compose wrapper

/// Hosts a Tauri `WebView` inside Compose.
///
/// @param webView      Optional existing wry `RustWebView` to reuse. When
///                     null, a bare placeholder WebView is created from
///                     scratch (see the checklist — it lacks the IPC bridge).
/// @param initialRoute Optional initial route (e.g. "settings") loaded as a
///                     query/hash so the SPA opens directly on that view.
@Composable
fun TauriWebView(
    modifier: Modifier = Modifier,
    webView: WebView? = null,
    initialRoute: String? = null,
) {
    AndroidView(
        modifier = modifier,
        factory = { context ->
            // Reuse wry's RustWebView when provided — it already has the
            // asset loader + IPC bridge wired up.
            webView ?: createBareWebView(context, initialRoute)
        },
        update = { view ->
            // Called when Compose state changes. Use this to:
            //   - Switch tabs via evaluateJavascript
            //   - Inject theme changes
            //   - Handle Tauri events from the Rust side
        },
    )
}

// MARK: - Full custom setup (if NOT reusing wry's RustWebView)

@SuppressLint("SetJavaScriptEnabled")
private fun createBareWebView(
    context: android.content.Context,
    initialRoute: String?,
): WebView {
    val webView = WebView(context)

    // ── 1. Settings ────────────────────────────────────────────────────
    webView.settings.apply {
        javaScriptEnabled = true
        domStorageEnabled = true
        // The frontend is served from assets via WebViewAssetLoader, so
        // file access from file URLs should stay OFF in production.
        allowFileAccess = false
        allowContentAccess = false
        mediaPlaybackRequiresUserGesture = false
        // Match the OS dark/light setting; the Preact CSS also reads
        // prefers-color-scheme.
        // (Use WebSettingsCompat.setForceDark on older API levels.)
    }

    // ── 2. Transparent background (app-like feel) ──────────────────────
    webView.setBackgroundColor(Color.TRANSPARENT)
    // Disable overscroll glow / bounce for an app-like feel.
    webView.overScrollMode = WebView.OVER_SCROLL_NEVER

    // ── 3. IPC bridge ───────────────────────────────────────────────────
    //
    // Tauri/wry communicates JS → Rust via a @JavascriptInterface object.
    // wry's RustWebView injects this automatically. If you build the
    // WebView yourself you must replicate it:
    //
    //   webView.addJavascriptInterface(object {
    //       @JavascriptInterface
    //       fun postMessage(message: String) { /* forward to Rust */ }
    //   }, "__TAURI_IPC__")
    //
    // CHECK: confirm the interface name wry v2 expects from the generated
    // code (it has historically been "__TAURI_IPC__").

    // ── 4. Asset loader + navigation ───────────────────────────────────
    //
    // wry serves the bundled frontend from `https://tauri.localhost` using
    // an `androidx.webkit.WebViewAssetLoader`. Replicating it here is what
    // makes `loadUrl("https://tauri.localhost/index.html")` resolve to the
    // bundled `dist/` assets. See wry's generated RustWebViewClient.
    webView.webViewClient = object : WebViewClient() {
        override fun onPageFinished(view: WebView?, url: String?) {
            // Frontend loaded. You can inject initial state here, e.g.
            // notify which tab is active, or apply a theme class:
            //   view?.evaluateJavascript(
            //     "if (matchMedia('(prefers-color-scheme: dark)').matches)" +
            //     " document.documentElement.classList.add('dark');", null)
        }
    }

    // ── 5. Load the frontend ───────────────────────────────────────────
    val suffix = initialRoute?.let { "?tab=$it" } ?: ""
    webView.loadUrl("https://tauri.localhost/index.html$suffix")

    return webView
}

// MARK: - Keyboard avoidance helper
//
// Android resizes the WebView when the soft keyboard appears if the
// generated Activity uses `android:windowSoftInputMode="adjustResize"`
// (set this in the generated AndroidManifest.xml). The Preact frontend
// also scrolls the comment input into view via the `visualViewport` API
// (see DiffViewer.tsx). Both layers work together — the native layer
// resizes around the bottom nav, the JS layer scrolls the input into view.
