#!/bin/sh
set -eu

echo "==> Bootstrapping Pex dependencies for Xcode Cloud"

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/../../../.." && pwd)"
cd "$REPO_ROOT"

export PATH="$HOME/.cargo/bin:/opt/homebrew/bin:/usr/local/bin:$PATH"

if ! command -v npm >/dev/null 2>&1; then
  echo "==> Installing Node.js with Homebrew"
  brew install node
fi

echo "==> Installing npm dependencies"
npm ci

if ! command -v rustup >/dev/null 2>&1; then
  echo "==> Installing Rust toolchain"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs \
    | sh -s -- -y --profile minimal --default-toolchain stable
fi

# shellcheck disable=SC1091
. "$HOME/.cargo/env"

echo "==> Installing Rust iOS targets"
rustup target add aarch64-apple-ios aarch64-apple-ios-sim

echo "==> Xcode Cloud bootstrap complete"
