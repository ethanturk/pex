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
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
}

if ! command -v npm >/dev/null 2>&1; then
  echo "==> Installing Node.js with Homebrew"
  retry brew install node
fi

echo "==> Installing npm dependencies"
retry npm ci

if ! command -v rustup >/dev/null 2>&1; then
  echo "==> Installing Rust toolchain"
  retry install_rustup
fi

# shellcheck disable=SC1091
. "$HOME/.cargo/env"

echo "==> Installing Rust iOS targets"
retry rustup target add aarch64-apple-ios aarch64-apple-ios-sim

echo "==> Xcode Cloud bootstrap complete"
