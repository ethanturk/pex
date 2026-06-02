package com.pex.pr_reviewer

import android.os.Bundle
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.padding
import androidx.compose.material.icons.Icons
import androidx.compose.material.icons.outlined.Description
import androidx.compose.material.icons.outlined.Settings
import androidx.compose.material3.Icon
import androidx.compose.material3.MaterialTheme
import androidx.compose.material3.NavigationBar
import androidx.compose.material3.NavigationBarItem
import androidx.compose.material3.Scaffold
import androidx.compose.material3.Text
import androidx.compose.runtime.Composable
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableIntStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier

/// ── App Entry Point ──────────────────────────────────────────────────────
/// Pex Android — Azure DevOps PR Reviewer
///
/// ## Integration with Tauri
///
/// After running `tauri android init` in src-tauri/, Tauri generates a
/// Gradle/Android Studio project with its own `MainActivity` (extending
/// Tauri's `TauriActivity` → wry's `WryActivity`) and an Android `WebView`
/// that hosts the Preact frontend. This Compose `MainActivity` REPLACES the
/// default Tauri-generated entry point (or wraps the generated WebView).
///
/// ## What Tauri provides
///
/// `tauri android init` generates (under `src-tauri/gen/android/`):
///   - A Gradle project: `app/`, `buildSrc/`, `settings.gradle`, wrapper
///   - `app/src/main/java/com/pex/pr_reviewer/MainActivity.kt` extending
///     `TauriActivity` — the default UIKit-less Android entry point
///   - `libpex_lib.so` — the Rust backend compiled as a shared library for
///     each ABI (arm64-v8a, armeabi-v7a, x86, x86_64) via `cargo-ndk`,
///     packaged into `jniLibs/` and loaded with `System.loadLibrary`
///   - The Rust `run()` function invoked from the generated activity's
///     `onCreate` (wry calls into the Rust entry point on startup)
///
/// ## What this file does
///
/// This is the launcher `Activity`. Instead of inflating an XML layout or
/// using the bare generated WebView, we use Jetpack Compose. The app
/// presents a two-tab interface (mirroring the iOS app):
///   - Tab 1: PRs — the review workflow
///   - Tab 2: Settings — AI config, accounts, theme
///
/// Each tab hosts an Android `WebView` (via Compose `AndroidView`) that
/// loads the Preact frontend from the app's assets. The webview talks to
/// the Rust backend through Tauri's `window.__TAURI__` bridge — no Kotlin ↔
/// JS bridging needed for Tauri `invoke()` calls.
///
/// ## Setup checklist (on your machine)
///
///   1. Install Android Studio + SDK Platform 34 + NDK + an emulator image
///   2. Set `ANDROID_HOME` and `NDK_HOME` (Tauri reads these)
///   3. `rustup target add aarch64-linux-android armv7-linux-androideabi \
///         i686-linux-android x86_64-linux-android`
///   4. `cd src-tauri && cargo install tauri-cli`
///   5. `tauri android init` — generates the Gradle project under gen/android
///   6. Replace the generated `MainActivity` with this file (or copy its
///      contents into the generated package), keeping the same package name
///      so the manifest's `<activity>` still resolves.
///   7. Build & run: `tauri android dev` (emulator/device) or open
///      `src-tauri/gen/android` in Android Studio and press Run.
///
/// ## Files to create (in this directory)
///
///   MainActivity.kt    ← this file
///   PRsTab.kt          ← review workflow tab
///   SettingsTab.kt     ← settings tab
///   WebView.kt         ← Android WebView wrapper (Compose AndroidView)
///
/// ## Notes
///
///   - The bottom navigation bar is Compose-native. The web frontend does
///     NOT render its own tab bar when running inside Tauri mobile — see
///     src/lib/platform.ts: isTauri() + isMobile() for detection.
///   - Settings shown in the Settings tab use the AiSettings component in
///     standalone mode (no modal backdrop).
///   - Tablets get the same two-tab layout — the web frontend handles the
///     adaptive multi-panel layout via CSS breakpoints (768px+).
///   - Multi-window / freeform is deferred. This app uses a single Activity
///     with `singleTask` launch mode (set in the generated manifest).
///
/// ## After copying into the generated project
///
///   1. Keep the package name identical to the generated one
///      (`com.pex.pr_reviewer` — Tauri derives it from the
///      `identifier` in tauri.conf.json: `com.pex.pr-reviewer`, with `-`
///      replaced by `_`).
///   2. Ensure the Rust library is loaded before the WebView is created.
///      The generated `TauriActivity` already calls `System.loadLibrary`
///      and the Rust `run()` entry point — check the generated activity
///      for the exact wiring if you fully replace it instead of extending.
///   3. Confirm `AndroidManifest.xml` points its launcher `<activity>` at
///      this class.

class MainActivity : androidx.activity.ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                PexApp()
            }
        }
    }
}

/// Top-level composable: a two-tab scaffold with a bottom navigation bar.
/// The selected tab index is hoisted here so each tab's WebView can be
/// notified of tab switches (the Preact frontend updates its state).
@Composable
fun PexApp() {
    var selectedTab by remember { mutableIntStateOf(0) }

    Scaffold(
        bottomBar = {
            NavigationBar {
                // ── Tab 1: PRs ────────────────────────────────────────
                NavigationBarItem(
                    selected = selectedTab == 0,
                    onClick = { selectedTab = 0 },
                    icon = { Icon(Icons.Outlined.Description, contentDescription = "PRs") },
                    label = { Text("PRs") },
                )
                // ── Tab 2: Settings ───────────────────────────────────
                NavigationBarItem(
                    selected = selectedTab == 1,
                    onClick = { selectedTab = 1 },
                    icon = { Icon(Icons.Outlined.Settings, contentDescription = "Settings") },
                    label = { Text("Settings") },
                )
            }
        },
    ) { innerPadding ->
        when (selectedTab) {
            0 -> PRsTab(modifier = Modifier.padding(innerPadding))
            else -> SettingsTab(modifier = Modifier.padding(innerPadding))
        }
    }
}
