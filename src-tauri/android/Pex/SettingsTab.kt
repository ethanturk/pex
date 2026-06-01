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
///   A) **Single shared webview** — both tabs share one `WebView` instance.
///      Compose just shows the same view. The frontend reads which tab is
///      active and switches its internal state accordingly. Simpler, less
///      memory, but the webview state persists across tabs (Settings form
///      stays filled in when switching back to PRs).
///
///   B) **Separate webviews per tab** — each tab has its own `WebView`.
///      More memory (~2×), but complete state isolation. The Settings tab
///      loads a different route or passes a query param.
///
///   RECOMMENDED: Start with (A) — single webview. The web frontend's
///   MobileShell already handles tab switching (src/app.tsx). The Compose
///   bottom nav and the Preact tab bar are separate — when the user taps a
///   Compose tab, we tell the webview to switch. When the webview's internal
///   state changes, we update the Compose tab selection.
///
/// ## Implementation for approach A
///
///   1. Create a shared `WebView` in MainActivity (or a ViewModel) and
///      remember it across recompositions (`remember { WebView(context) }`).
///   2. Pass it to both PRsTab and SettingsTab.
///   3. On tab change, call `webView.evaluateJavascript(...)` to notify the
///      frontend.
///   4. The frontend's platform.ts detects Tauri + mobile and enables
///      tab-aware routing.
///
///   The JS to switch to the Settings tab:
///     window.__tauri_mobile_tab__ = "settings";
///     window.dispatchEvent(new Event("tauri-tab-change"));
///
///   See the mobile app shell (src/app.tsx: MobileShell) for the
///   corresponding frontend handler. This is identical to the iOS app —
///   the same JS contract drives both platforms.
///
/// ## For now
///
/// This view is a PLACEHOLDER. After setting up the Tauri webview:
///   1. Choose approach A or B above
///   2. Replace the placeholder with the webview
///   3. Wire up tab change communication

@Composable
fun SettingsTab(modifier: Modifier = Modifier) {
    // ── Placeholder — replace with webview ─────────────────────────────
    //
    // Option A (shared webview):
    //   TauriWebView(webView = sharedWebView, modifier = modifier)
    //   // then, on entering this tab:
    //   sharedWebView.evaluateJavascript(
    //       "window.__tauri_mobile_tab__ = 'settings';" +
    //       "window.dispatchEvent(new Event('tauri-tab-change'))",
    //       null,
    //   )
    //
    // Option B (separate webview):
    //   TauriWebView(initialRoute = "settings", modifier = modifier)

    PlaceholderSettingsTab(modifier = modifier)
}

// ── Placeholder — delete after hooking up webview ─────────────────────────

@Composable
private fun PlaceholderSettingsTab(modifier: Modifier = Modifier) {
    Surface(modifier = modifier.fillMaxSize()) {
        Column(
            modifier = Modifier.fillMaxSize().padding(24.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center,
        ) {
            Text("Settings", style = MaterialTheme.typography.headlineMedium)
            Text(
                "\n• AI Provider Configuration" +
                    "\n• Customize Review Prompts" +
                    "\n• Account Management",
                style = MaterialTheme.typography.bodyMedium,
                textAlign = TextAlign.Start,
            )
        }
    }
}
