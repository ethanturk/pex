#!/bin/sh
set -eu

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

# libsql-ffi builds its bundled SQLite (SQLite3MultipleCiphers) through cmake.
# cmake-rs only auto-sets CMAKE_OSX_ARCHITECTURES for macOS ("darwin") targets,
# never for apple-ios — so with an older cmake the SQLite sources compile against
# the iOS SDK with no -arch and clang fails to resolve __int64_t in
# <sys/_types.h>. Hand cmake an explicit iOS toolchain file (cmake-rs honors
# CMAKE_TOOLCHAIN_FILE from the env) so the arch/sysroot are always pinned.
# `arch` = arm64|x86_64, `sysroot` = iphoneos|iphonesimulator.
setup_ios_cmake_toolchain() {
  _arch="$1"
  _sysroot="$2"
  _deployment="${3:-16.0}"
  _file="${TMPDIR:-/tmp}/pex-ios-cmake-toolchain.cmake"
  cat > "$_file" <<EOF
set(CMAKE_SYSTEM_NAME iOS)
set(CMAKE_OSX_ARCHITECTURES ${_arch})
set(CMAKE_OSX_SYSROOT ${_sysroot})
set(CMAKE_OSX_DEPLOYMENT_TARGET ${_deployment})
EOF
  export CMAKE_TOOLCHAIN_FILE="$_file"
  echo "==> cmake iOS toolchain: arch=${_arch} sysroot=${_sysroot} min=${_deployment} ($_file)"
}

if [ -z "${CI_XCODE_CLOUD:-}" ]; then
  # Local/Xcode build phase: derive arch + sysroot from the Xcode-provided env.
  case "${PLATFORM_DISPLAY_NAME:?}" in
    *Simulator*) IOS_SDK="iphonesimulator" ;;
    *) IOS_SDK="iphoneos" ;;
  esac
  # ARCHS may be a space-separated list; cmake takes one arch per Rust target.
  IOS_ARCH="$(printf '%s' "${ARCHS:?}" | awk '{print $1}')"
  setup_ios_cmake_toolchain "$IOS_ARCH" "$IOS_SDK" "${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"

  npm run -- tauri ios xcode-script -v \
    --platform "${PLATFORM_DISPLAY_NAME:?}" \
    --sdk-root "${SDKROOT:?}" \
    --framework-search-paths "${FRAMEWORK_SEARCH_PATHS:?}" \
    --header-search-paths "${HEADER_SEARCH_PATHS:?}" \
    --gcc-preprocessor-definitions "${GCC_PREPROCESSOR_DEFINITIONS:-}" \
    --configuration "${CONFIGURATION:?}" \
    ${FORCE_COLOR:-} \
    ${ARCHS:?}
  exit 0
fi

echo "==> Xcode Cloud: building Rust static library directly"

# Xcode exports SDKROOT pointing at the iOS SDK. cargo build scripts that compile
# a HOST tool (e.g. libsql-sqlite3-parser builds `lemon` for the build machine)
# pick up that iOS sysroot via the cc crate and try to build a macOS-host binary
# against it — clang fails with "using sysroot for 'iPhoneOS' but targeting
# 'MacOSX'" → "Unsupported architecture" / unknown type '__int64_t'. Drop the
# inherited SDK env so the iOS --target build resolves the iOS SDK via xcrun.
# (This branch never uses SDKROOT itself — it derives the Rust target from
# PLATFORM_DISPLAY_NAME/ARCHS.)
unset SDKROOT

# With SDKROOT gone, host build-script tools have no sysroot and fail with
# "'stdio.h' file not found" (CI's host clang has no default SDK). Point the HOST
# compiler at the macOS SDK explicitly via cc's per-target CFLAGS (cc reads the
# underscored host-triple form, e.g. CFLAGS_x86_64_apple_darwin). The iOS
# --target build is unaffected — it still resolves the iOS SDK through xcrun.
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
HOST_ENV_KEY="$(printf '%s' "$HOST_TRIPLE" | tr '-' '_')"
MACOS_SDK_PATH="$(xcrun --sdk macosx --show-sdk-path)"
export "CFLAGS_${HOST_ENV_KEY}=-isysroot ${MACOS_SDK_PATH}"
export "CXXFLAGS_${HOST_ENV_KEY}=-isysroot ${MACOS_SDK_PATH}"
echo "==> host (${HOST_TRIPLE}) build-script SDK: ${MACOS_SDK_PATH}"

retry() {
  attempts=1
  max_attempts=5
  delay_seconds=10

  until "$@"; do
    status=$?
    if [ "$attempts" -ge "$max_attempts" ]; then
      echo "Command failed after $attempts attempts: $*" >&2
      return "$status"
    fi

    echo "Command failed, retrying in ${delay_seconds}s: $*" >&2
    sleep "$delay_seconds"
    attempts=$((attempts + 1))
    delay_seconds=$((delay_seconds * 2))
  done
}

REPO_ROOT="$(cd "${SRCROOT:?}/../../.." && pwd)"
TAURI_DIR="$REPO_ROOT/src-tauri"
mkdir -p "${SRCROOT:?}/assets"

if [ -n "${CI_TAG:-}" ]; then
  APP_VERSION="${CI_TAG#v}"
else
  APP_VERSION="$(node -e 'const fs = require("fs"); console.log(JSON.parse(fs.readFileSync(process.argv[1], "utf8")).version)' "$REPO_ROOT/package.json")"
fi
echo "==> Xcode Cloud: setting CFBundleShortVersionString to ${APP_VERSION}"
/usr/libexec/PlistBuddy -c "Set :CFBundleShortVersionString ${APP_VERSION}" \
  "${SRCROOT:?}/pex_iOS/Info.plist"

if [ -n "${CI_BUILD_NUMBER:-}" ]; then
  echo "==> Xcode Cloud: setting CFBundleVersion to ${CI_BUILD_NUMBER}"
  /usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${CI_BUILD_NUMBER}" \
    "${SRCROOT:?}/pex_iOS/Info.plist"
fi

case "${PLATFORM_DISPLAY_NAME:?}:${ARCHS:?}" in
  "iOS:"*"arm64"*)
    RUST_TARGET="aarch64-apple-ios"
    EXTERNALS_ARCH="arm64"
    IOS_SDK="iphoneos"
    ;;
  "iOS Simulator:"*"arm64"*)
    RUST_TARGET="aarch64-apple-ios-sim"
    EXTERNALS_ARCH="arm64"
    IOS_SDK="iphonesimulator"
    ;;
  "iOS Simulator:"*"x86_64"*)
    RUST_TARGET="x86_64-apple-ios"
    EXTERNALS_ARCH="x86_64"
    IOS_SDK="iphonesimulator"
    ;;
  *)
    echo "Unsupported Xcode platform/architecture: ${PLATFORM_DISPLAY_NAME}:${ARCHS}" >&2
    exit 1
    ;;
esac

# Pin the cmake iOS toolchain (arch + sysroot) for libsql-ffi's SQLite build.
setup_ios_cmake_toolchain "$EXTERNALS_ARCH" "$IOS_SDK" "${IPHONEOS_DEPLOYMENT_TARGET:-16.0}"

if [ "${CONFIGURATION:?}" = "release" ]; then
  CARGO_PROFILE_FLAG="--release"
  CARGO_PROFILE_DIR="release"
else
  CARGO_PROFILE_FLAG=""
  CARGO_PROFILE_DIR="debug"
fi
CARGO_FEATURE_FLAG="--features custom-protocol"

cd "$REPO_ROOT"
npm run build

cd "$TAURI_DIR"
export CARGO_NET_RETRY="${CARGO_NET_RETRY:-10}"
export CARGO_HTTP_TIMEOUT="${CARGO_HTTP_TIMEOUT:-120}"
export CARGO_REGISTRIES_CRATES_IO_PROTOCOL="${CARGO_REGISTRIES_CRATES_IO_PROTOCOL:-git}"
export CARGO_NET_GIT_FETCH_WITH_CLI="${CARGO_NET_GIT_FETCH_WITH_CLI:-true}"

retry rustup target add "$RUST_TARGET"
retry cargo fetch --target "$RUST_TARGET"
retry cargo build --lib --target "$RUST_TARGET" $CARGO_FEATURE_FLAG $CARGO_PROFILE_FLAG

LIB_SOURCE="$TAURI_DIR/target/$RUST_TARGET/$CARGO_PROFILE_DIR/libpex_lib.a"
LIB_DEST_DIR="${SRCROOT:?}/Externals/$EXTERNALS_ARCH/${CONFIGURATION:?}"
LIB_DEST="$LIB_DEST_DIR/libapp.a"

if [ ! -f "$LIB_SOURCE" ]; then
  echo "Rust static library not found at $LIB_SOURCE" >&2
  exit 1
fi

mkdir -p "$LIB_DEST_DIR"
cp "$LIB_SOURCE" "$LIB_DEST"
echo "==> Wrote $LIB_DEST"
