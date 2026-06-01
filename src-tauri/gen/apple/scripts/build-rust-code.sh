#!/bin/sh
set -eu

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if [ -z "${CI_XCODE_CLOUD:-}" ]; then
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

REPO_ROOT="$(cd "${SRCROOT:?}/../../.." && pwd)"
TAURI_DIR="$REPO_ROOT/src-tauri"
mkdir -p "${SRCROOT:?}/assets"

if [ -n "${CI_BUILD_NUMBER:-}" ]; then
  echo "==> Xcode Cloud: setting CFBundleVersion to ${CI_BUILD_NUMBER}"
  /usr/libexec/PlistBuddy -c "Set :CFBundleVersion ${CI_BUILD_NUMBER}" \
    "${SRCROOT:?}/pex_iOS/Info.plist"
fi

case "${PLATFORM_DISPLAY_NAME:?}:${ARCHS:?}" in
  "iOS:"*"arm64"*)
    RUST_TARGET="aarch64-apple-ios"
    EXTERNALS_ARCH="arm64"
    ;;
  "iOS Simulator:"*"arm64"*)
    RUST_TARGET="aarch64-apple-ios-sim"
    EXTERNALS_ARCH="arm64"
    ;;
  "iOS Simulator:"*"x86_64"*)
    RUST_TARGET="x86_64-apple-ios"
    EXTERNALS_ARCH="x86_64"
    ;;
  *)
    echo "Unsupported Xcode platform/architecture: ${PLATFORM_DISPLAY_NAME}:${ARCHS}" >&2
    exit 1
    ;;
esac

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
rustup target add "$RUST_TARGET"
cargo build --lib --target "$RUST_TARGET" $CARGO_FEATURE_FLAG $CARGO_PROFILE_FLAG

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
