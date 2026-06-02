#!/usr/bin/env bash
#
# Reproducibly apply Pex's Android customizations to the generated Gradle
# project (`src-tauri/gen/android`), which is .gitignored and recreated by
# `npx tauri android init`. Run this after a fresh init (or any time the generated
# project is regenerated):
#
#     source .android-env.sh        # ANDROID_HOME / NDK_HOME / JDK 17 on PATH
#     npm run android:setup         # (wraps this script)
#
# It is idempotent — safe to run repeatedly.
#
# What it does:
#   1. Generates the Gradle project with `npx tauri android init` if it's missing.
#   2. Installs the branded launcher icons. The committed source of truth is
#      `src-tauri/icons/android/` (a full adaptive-icon set: per-density PNGs,
#      the `mipmap-anydpi-v26` adaptive XML, and the background color). To
#      refresh that set from a new brand image, run
#      `cargo tauri icon <path-to-1024px.png>` and re-commit `icons/android/`.
#   3. Sets `android:windowSoftInputMode="adjustResize"` on the launcher
#      activity so the WebView resizes for the soft keyboard (paired with the
#      `visualViewport` handling in the frontend). Tauri exposes no
#      tauri.conf.json field for this, so we patch the generated manifest.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SRC_TAURI="$(cd "$SCRIPT_DIR/.." && pwd)"
REPO_ROOT="$(cd "$SRC_TAURI/.." && pwd)"
GEN="$SRC_TAURI/gen/android"
RES="$GEN/app/src/main/res"
ICONS="$SRC_TAURI/icons/android"
MANIFEST="$GEN/app/src/main/AndroidManifest.xml"

# 1. Generate the project if it doesn't exist yet.
if [ ! -d "$GEN" ]; then
  echo "==> gen/android missing — running 'npx tauri android init'"
  ( cd "$REPO_ROOT" && npx --no-install tauri android init )
fi

# 2. Branded launcher icons.
echo "==> Installing branded launcher icons from icons/android/"
for d in mdpi hdpi xhdpi xxhdpi xxxhdpi; do
  for f in ic_launcher.png ic_launcher_foreground.png ic_launcher_round.png; do
    src="$ICONS/mipmap-$d/$f"
    [ -f "$src" ] && cp -f "$src" "$RES/mipmap-$d/$f"
  done
done
# Adaptive icon (API 26+) + the background color it references.
mkdir -p "$RES/mipmap-anydpi-v26"
cp -f "$ICONS/mipmap-anydpi-v26/ic_launcher.xml" "$RES/mipmap-anydpi-v26/ic_launcher.xml"
cp -f "$ICONS/values/ic_launcher_background.xml" "$RES/values/ic_launcher_background.xml"

# 3. windowSoftInputMode=adjustResize on the launcher activity (idempotent).
if grep -q 'android:windowSoftInputMode' "$MANIFEST"; then
  echo "==> Manifest already has windowSoftInputMode — skipping"
else
  echo "==> Patching manifest: windowSoftInputMode=\"adjustResize\""
  perl -0pi -e 's{(android:name="\.MainActivity")}{$1\n            android:windowSoftInputMode="adjustResize"}' "$MANIFEST"
fi

# 4. Release signing config. The signing material lives in
#    gen/android/keystore.properties (git-ignored; written from secrets in CI or
#    by the developer locally). When that file is absent the release build is
#    simply unsigned, so this is safe to apply unconditionally.
GRADLE="$GEN/app/build.gradle.kts"
if grep -q 'pex-signing' "$GRADLE"; then
  echo "==> Signing config already present — skipping"
else
  echo "==> Injecting release signing config into build.gradle.kts"
  # Load keystore.properties (uses the `import java.util.Properties` already at
  # the top of the generated file).
  perl -0pi -e 's~\nandroid \{\n~\n// pex-signing: release signing pulled from keystore.properties (git-ignored)\nval keystorePropertiesFile = rootProject.file("keystore.properties")\nval keystoreProperties = Properties().apply {\n    if (keystorePropertiesFile.exists()) keystorePropertiesFile.inputStream().use { load(it) }\n}\n\nandroid {\n~' "$GRADLE"
  # Declare the release signingConfig (populated only when the keystore exists).
  perl -0pi -e 's~\n    buildTypes \{\n~\n    signingConfigs {\n        create("release") {\n            if (keystorePropertiesFile.exists()) {\n                storeFile = file(keystoreProperties.getProperty("storeFile"))\n                storePassword = keystoreProperties.getProperty("storePassword")\n                keyAlias = keystoreProperties.getProperty("keyAlias")\n                keyPassword = keystoreProperties.getProperty("keyPassword")\n            }\n        }\n    }\n    buildTypes {\n~' "$GRADLE"
  # Apply it to the release build type (only when a keystore is configured).
  perl -0pi -e 's~(        getByName\("release"\) \{\n)~${1}            if (keystorePropertiesFile.exists()) signingConfig = signingConfigs.getByName("release")\n~' "$GRADLE"
fi

echo "==> Android customizations applied."
