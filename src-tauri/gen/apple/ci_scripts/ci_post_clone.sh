#!/bin/sh
set -eu

echo "==> Bootstrapping Pex dependencies for Xcode Cloud"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
cd "$REPO_ROOT"

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

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

install_rustup() {
  rustup_script="${TMPDIR:-/tmp}/rustup-init.sh"
  curl --proto '=https' --tlsv1.2 -sSfL https://sh.rustup.rs -o "$rustup_script"
  sh "$rustup_script" -y --profile minimal --default-toolchain stable
}

if ! command -v npm >/dev/null 2>&1; then
  echo "==> Installing Node.js with Homebrew"
  retry brew install node
fi

# libsql-ffi builds bundled SQLite via cmake. Ensure a modern cmake is present:
# older/absent cmake omits -arch for apple-ios targets, so the iOS SDK headers
# fail to parse ("unknown type name '__int64_t'"). build-rust-code.sh also pins
# an explicit iOS toolchain file so arch/sysroot are correct regardless.
if ! command -v cmake >/dev/null 2>&1; then
  echo "==> Installing CMake with Homebrew"
  retry brew install cmake
fi

echo "==> Installing npm dependencies"
retry npm ci

if ! command -v rustup >/dev/null 2>&1; then
  echo "==> Installing Rust toolchain"
  retry install_rustup
fi

if [ -f "$HOME/.cargo/env" ]; then
  # shellcheck disable=SC1091
  . "$HOME/.cargo/env"
elif command -v cargo >/dev/null 2>&1; then
  echo "==> Using Rust toolchain already available on PATH"
else
  echo "Rust toolchain bootstrap did not create $HOME/.cargo/env and cargo is not on PATH." >&2
  echo "Check Xcode Cloud network/DNS access to https://sh.rustup.rs and retry the build." >&2
  exit 1
fi

echo "==> Installing Rust iOS targets"
retry rustup target add aarch64-apple-ios aarch64-apple-ios-sim

echo "==> Xcode Cloud bootstrap complete"
