package com.pex.pr_reviewer

import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.padding
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.Surface
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp

/// ── PRs Tab — Review Workflow ────────────────────────────────────────────
///
/// This tab hosts the main PR review workflow in an Android `WebView`. The
/// Preact frontend handles all navigation internally (Auth → Org Select →
/// PR List → PR Detail) via its `currentView` signal. Compose does NOT
/// manage the view stack — it's a single, persistent webview.
///
/// ## Design rationale
///
/// We use a single webview (not a Navigation graph with multiple webviews)
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
/// After `tauri android init`, Tauri (via wry) sets up an Android `WebView`
/// that serves the bundled frontend through a `WebViewAssetLoader` mapped to
/// the `https://tauri.localhost` origin. The exact loading mechanism lives
/// in the generated `RustWebView` / `TauriActivity`. Typically:
///   - wry registers a custom `WebViewAssetLoader` for the app assets
///   - the webview loads `https://tauri.localhost/index.html`
///   - the Rust backend is initialized before the webview loads
///
/// ## What Compose provides
///
///   - The bottom-nav item label and icon (in MainActivity)
///   - Containment (the webview fills the available space)
///   - Window insets are handled via the Scaffold's `innerPadding`
///
/// ## Future enhancement (v2)
///
/// If we want native push transitions (predictive back gesture, etc.), we
/// could use Navigation-Compose with programmatic navigation and notify the
/// webview of view changes via `evaluateJavascript`. This is deferred — the
/// SPA-based navigation works well and feels native enough.

@Composable
fun PRsTab(modifier: Modifier = Modifier) {
    // ── Replace this with the Tauri WebView ────────────────────────────
    //
    // After `tauri android init`:
    //   1. Find the generated WebView setup (the `RustWebView` created by
    //      wry inside `TauriActivity`).
    //   2. Replace `PlaceholderPRsTab()` below with the wrapper:
    //
    //         TauriWebView(modifier = modifier)
    //
    //   3. If you reuse the generated `RustWebView` instance, hand it to
    //      `AndroidView(factory = { existingWebView })` instead of creating
    //      a fresh one. See WebView.kt in this directory for the wrapper.

    PlaceholderPRsTab(modifier = modifier)
}

// ── Placeholder — replace with real Tauri webview ──────────────────────────

/// Temporary placeholder. DELETE this after hooking up the real Tauri
/// webview. Shows a brief instruction card in the emulator while setting up.
@Composable
private fun PlaceholderPRsTab(modifier: Modifier = Modifier) {
    Surface(modifier = modifier.fillMaxSize()) {
        Column(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Text("Pex Android", style = MaterialTheme.typography.headlineMedium)
            Text(
                "PR Review for Azure DevOps",
                style = MaterialTheme.typography.bodyMedium,
                color = MaterialTheme.colorScheme.onSurfaceVariant,
            )
            Text(
                "\n1. Load the Tauri WebView" +
                    "\n2. Replace this placeholder" +
                    "\n3. See WebView.kt for the wrapper",
                style = MaterialTheme.typography.bodySmall,
                textAlign = TextAlign.Start,
            )
        }
    }
}
