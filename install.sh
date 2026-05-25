#!/usr/bin/env bash
set -euo pipefail

# ──────────────────────────────────────────────
# Pex — Azure DevOps PR Reviewer
# Linux / macOS install script
#
# Run either:
#   • from a cloned repo:   ./install.sh
#   • piped from curl:      curl -fsSL https://raw.githubusercontent.com/ethanturk/pex/master/install.sh | bash
#
# Environment overrides:
#   PEX_REPO   git URL to clone (default: https://github.com/ethanturk/pex.git)
#   PEX_REF    branch/tag/commit (default: master)
# ──────────────────────────────────────────────

PEX_REPO="${PEX_REPO:-https://github.com/ethanturk/pex.git}"
PEX_REF="${PEX_REF:-master}"

# When piped from curl, BASH_SOURCE[0] is empty/"bash" and no script file exists.
# Detect that and clone the repo into a temp dir before continuing.
_src="${BASH_SOURCE[0]:-}"
if [ -n "$_src" ] && [ -f "$_src" ]; then
  SCRIPT_DIR="$(cd "$(dirname "$_src")" && pwd)"
else
  SCRIPT_DIR=""
fi

if [ -z "$SCRIPT_DIR" ] || [ ! -f "$SCRIPT_DIR/src-tauri/tauri.conf.json" ]; then
  if ! command -v git >/dev/null 2>&1; then
    echo "git is required to bootstrap the Pex source. Install git and re-run."
    exit 1
  fi
  TMP_DIR="$(mktemp -d -t pex-install.XXXXXX)"
  trap 'rm -rf "$TMP_DIR"' EXIT
  echo "Fetching Pex source ($PEX_REPO @ $PEX_REF)…"
  git clone --depth 1 --branch "$PEX_REF" "$PEX_REPO" "$TMP_DIR" >/dev/null 2>&1 \
    || git clone "$PEX_REPO" "$TMP_DIR" >/dev/null
  SCRIPT_DIR="$TMP_DIR"
fi

# When piped from curl, stdin is the pipe and `read` would EOF immediately.
# Read interactive prompts from /dev/tty when available.
if [ -t 0 ]; then
  INPUT_FD=/dev/stdin
elif [ -e /dev/tty ]; then
  INPUT_FD=/dev/tty
else
  INPUT_FD=""
fi
prompt_yn() {
  # $1 = prompt; sets REPLY (defaults to "Y" if no input source)
  if [ -n "$INPUT_FD" ]; then
    printf "%s" "$1"
    read -r REPLY <"$INPUT_FD" || REPLY="Y"
  else
    REPLY="Y"
  fi
}
BOLD="$(tput bold 2>/dev/null || printf '')"
DIM="$(tput dim 2>/dev/null || printf '')"
GREEN="$(tput setaf 2 2>/dev/null || printf '')"
YELLOW="$(tput setaf 3 2>/dev/null || printf '')"
RED="$(tput setaf 1 2>/dev/null || printf '')"
RESET="$(tput sgr0 2>/dev/null || printf '')"
CHECK="${GREEN}✔${RESET}"
CROSS="${RED}✘${RESET}"

header()  { printf "\n%s── %s%s\n" "${BOLD}" "$1" "${RESET}"; }
step()   { printf "  %s  %s\n" "${DIM}→${RESET}" "$1"; }
ok()     { printf "  %s  %s\n" "${CHECK}" "$1"; }
warn()   { printf "  %s  %s\n" "${YELLOW}⚠${RESET}"  "$1"; }
fail()   { printf "  %s  %s\n" "${CROSS}" "$1"; }

# ── OS guard ──────────────────────────────────
OS="$(uname -s)"
case "$OS" in
  Linux|Darwin) ;;
  *)
    echo "Pex does not support $OS. Use install.ps1 on Windows."
    exit 1
    ;;
esac

# ── Dependency checks ─────────────────────────
header "Checking prerequisites"

# -- Rust
if command -v rustc &>/dev/null; then
  ok "Rust $(rustc --version | awk '{print $2}')"
else
  fail "Rust not found"
  echo "       Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
  exit 1
fi

# -- Node.js
if command -v node &>/dev/null; then
  ok "Node.js $(node --version)"
else
  fail "Node.js not found"
  echo "       Install: https://nodejs.org (LTS recommended)"
  exit 1
fi

# -- npm
if command -v npm &>/dev/null; then
  ok "npm $(npm --version)"
else
  fail "npm not found — reinstall Node.js"
  exit 1
fi

# ── System dependencies (Linux only) ──────────
if [ "$OS" = Linux ]; then
  header "Checking system libraries"

  MISSING_DEPS=""

  check_lib() {
    local label="$1" pkgconf="$2"
    if pkg-config --exists "$pkgconf" 2>/dev/null; then
      ok "$label"
    else
      fail "$label"
      MISSING_DEPS="$MISSING_DEPS $label"
    fi
  }

  check_lib "webkit2gtk-4.1" "webkit2gtk-4.1"
  check_lib "gtk4"              "gtk4"
  check_lib "libsoup-3.0"       "libsoup-3.0"

  if [ -n "$MISSING_DEPS" ]; then
    echo ""
    warn "Missing libraries:${MISSING_DEPS}"

    if command -v dnf &>/dev/null; then
      # Fedora / RHEL family
      step "Detected dnf — run the following to install:"
      echo ""
      echo "      sudo dnf install webkit2gtk4.1-devel gtk4-devel \\"
      echo "                       libsoup3-devel libappindicator-gtk3-devel \\"
      echo "                       openssl-devel curl wget file"
      echo ""
    elif command -v apt &>/dev/null; then
      # Debian / Ubuntu family
      step "Detected apt — run the following to install:"
      echo ""
      echo "      sudo apt install libwebkit2gtk-4.1-dev libgtk-4-dev \\"
      echo "                       libsoup-3.0-dev libappindicator3-dev \\"
      echo "                       libssl-dev curl wget file"
      echo ""
    elif command -v pacman &>/dev/null; then
      step "Detected pacman — run the following to install:"
      echo ""
      echo "      sudo pacman -S webkit2gtk-4.1 gtk4 libsoup3 \\"
      echo "                       libappindicator-gtk3 base-devel curl wget file"
      echo ""
    else
      warn "Unknown package manager — you'll need to install GTK+WebKit deps manually"
    fi

    printf "\nRerun %s after installing the dependencies.\n" "$0"
    exit 1
  fi
fi

# ── Build ─────────────────────────────────────
header "Building Pex"

cd "$SCRIPT_DIR"

step "Installing npm dependencies…"
npm install --silent

# Disable updater artifact generation for local installs. The .app.tar.gz
# updater bundle requires minisign signing — without the env var
# TAURI_SIGNING_PRIVATE_KEY set, Tauri prompts for the key path + password
# once per artifact. End-user installs don't auto-update from a locally
# built bundle, so skip the whole signing step.
TAURI_BUILD_ARGS=( -- --config '{"bundle":{"createUpdaterArtifacts":false}}' )

if [ "$OS" = Darwin ]; then
  step "Building for macOS (cargo tauri build)…"
  npm run tauri build "${TAURI_BUILD_ARGS[@]}"
else
  step "Building for Linux (cargo tauri build)…"
  npm run tauri build "${TAURI_BUILD_ARGS[@]}"
fi

header "Build complete"

# ── Install ───────────────────────────────────
if [ "$OS" = Linux ]; then
  # Try to detect and install the built package
  BUNDLE_DIR="$SCRIPT_DIR/src-tauri/target/release/bundle"

  if [ -d "$BUNDLE_DIR/deb" ] && command -v dpkg &>/dev/null; then
    DEB="$(echo "$BUNDLE_DIR"/deb/*.deb | head -1)"
    if [ -f "$DEB" ]; then
      step "Installing .deb package…"
      printf "\n      Requires sudo. Install %s?\n" "$(basename "$DEB")"
      prompt_yn "      [Y/n] "
      if [ -z "${REPLY:-}" ] || [ "$REPLY" = Y ] || [ "$REPLY" = y ]; then
        sudo dpkg -i "$DEB"
        ok "Pex installed"
      else
        step "Skipped. Package at: $DEB"
      fi
      exit 0
    fi
  fi

  if [ -d "$BUNDLE_DIR/rpm" ] && command -v rpm &>/dev/null; then
    RPM="$(echo "$BUNDLE_DIR"/rpm/*.rpm | head -1)"
    if [ -f "$RPM" ]; then
      step "Installing .rpm package…"
      printf "\n      Requires sudo. Install %s?\n" "$(basename "$RPM")"
      prompt_yn "      [Y/n] "
      if [ -z "${REPLY:-}" ] || [ "$REPLY" = Y ] || [ "$REPLY" = y ]; then
        sudo rpm -i "$RPM"
        ok "Pex installed"
      else
        step "Skipped. Package at: $RPM"
      fi
      exit 0
    fi
  fi

  # Fallback — AppImage
  if [ -d "$BUNDLE_DIR/appimage" ]; then
    APPIMAGE="$(echo "$BUNDLE_DIR"/appimage/*.AppImage | head -1)"
    if [ -f "$APPIMAGE" ]; then
      chmod +x "$APPIMAGE"
      ok "AppImage ready: $APPIMAGE"
      exit 0
    fi
  fi

  warn "No installable package found — check $BUNDLE_DIR"

elif [ "$OS" = Darwin ]; then
  APP_SRC="$SCRIPT_DIR/src-tauri/target/release/bundle/macos/Pex.app"
  APP_DEST="/Applications/Pex.app"

  if [ ! -d "$APP_SRC" ]; then
    warn "No Pex.app found at $APP_SRC"
    fail "Build did not produce a macOS app bundle."
    exit 1
  fi

  # Quit any running Pex so we can overwrite /Applications/Pex.app cleanly —
  # otherwise the rm-then-cp leaves the user running the stale binary.
  if pgrep -x Pex >/dev/null 2>&1; then
    step "Pex is running — quitting it…"
    osascript -e 'tell application "Pex" to quit' >/dev/null 2>&1 || true
    sleep 1
    pgrep -x Pex >/dev/null 2>&1 && pkill -x Pex >/dev/null 2>&1 || true
  fi

  # Replace any existing install. Try without sudo first; if /Applications
  # is locked down (multi-user mac), retry with sudo.
  copy_app() {
    rm -rf "$APP_DEST" 2>/dev/null && cp -R "$APP_SRC" "$APP_DEST" 2>/dev/null
  }

  step "Installing to $APP_DEST…"
  if ! copy_app; then
    warn "Need elevated permissions to write to /Applications"
    sudo rm -rf "$APP_DEST"
    sudo cp -R "$APP_SRC" "$APP_DEST"
  fi

  # Strip the quarantine flag the OS applies to a freshly-copied unsigned
  # bundle (avoids the "Pex can't be opened, unidentified developer" block
  # on first launch).
  xattr -dr com.apple.quarantine "$APP_DEST" 2>/dev/null || true

  ok "Pex installed at $APP_DEST"

  if [ -n "$INPUT_FD" ]; then
    prompt_yn "      Launch Pex now? [Y/n] "
    if [ -z "${REPLY:-}" ] || [ "$REPLY" = Y ] || [ "$REPLY" = y ]; then
      open "$APP_DEST"
    fi
  fi
fi
