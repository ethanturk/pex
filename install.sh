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
# By default, downloads the latest signed release binary for your platform.
# Falls back to a from-source build if no matching binary is available.
#
# Flags:
#   --from-source     force a from-source build (skip the binary download)
#   --help            show this help
#
# Environment overrides:
#   PEX_REPO_SLUG    GitHub owner/repo (default: ethanturk/pex)
#   PEX_REPO         git URL for source builds (default: derived from slug)
#   PEX_REF          branch/tag/commit for source builds (default: master)
#   PEX_FROM_SOURCE  set to 1 to force a from-source build
# ──────────────────────────────────────────────

PEX_REPO_SLUG="${PEX_REPO_SLUG:-ethanturk/pex}"
PEX_REPO="${PEX_REPO:-https://github.com/${PEX_REPO_SLUG}.git}"
PEX_REF="${PEX_REF:-master}"
FROM_SOURCE="${PEX_FROM_SOURCE:-0}"

for arg in "${@:-}"; do
  case "$arg" in
    --from-source|--source) FROM_SOURCE=1 ;;
    --help|-h)
      sed -n '4,22p' "$0" 2>/dev/null | sed 's/^# \{0,1\}//'
      exit 0
      ;;
    "") ;;
    *)
      echo "Unknown argument: $arg" >&2
      exit 2
      ;;
  esac
done

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

header() { printf "\n%s── %s%s\n" "${BOLD}" "$1" "${RESET}"; }
step()   { printf "  %s  %s\n" "${DIM}→${RESET}" "$1"; }
ok()     { printf "  %s  %s\n" "${CHECK}" "$1"; }
warn()   { printf "  %s  %s\n" "${YELLOW}⚠${RESET}" "$1"; }
fail()   { printf "  %s  %s\n" "${CROSS}" "$1"; }

# ── OS / arch ─────────────────────────────────
OS="$(uname -s)"
ARCH="$(uname -m)"
case "$OS" in
  Linux|Darwin) ;;
  *)
    echo "Pex does not support $OS. Use install.ps1 on Windows."
    exit 1
    ;;
esac

# ── Shared install steps for the macOS .app bundle ──
# Used by both the binary-download path and the source-build path.
install_macos_app() {
  local app_src="$1"
  local app_dest="/Applications/Pex.app"

  if [ ! -d "$app_src" ]; then
    fail "Expected $app_src but it doesn't exist."
    return 1
  fi

  # Quit any running Pex so the rm-then-cp doesn't leave the user on a stale
  # binary held by the old process.
  if pgrep -x Pex >/dev/null 2>&1; then
    step "Pex is running — quitting it…"
    osascript -e 'tell application "Pex" to quit' >/dev/null 2>&1 || true
    sleep 1
    pgrep -x Pex >/dev/null 2>&1 && pkill -x Pex >/dev/null 2>&1 || true
  fi

  step "Installing to $app_dest…"
  if ! { rm -rf "$app_dest" 2>/dev/null && cp -R "$app_src" "$app_dest" 2>/dev/null; }; then
    warn "Need elevated permissions to write to /Applications"
    sudo rm -rf "$app_dest"
    sudo cp -R "$app_src" "$app_dest"
  fi

  # Strip the quarantine flag the OS applies to a freshly-downloaded unsigned
  # bundle (avoids the "Pex can't be opened, unidentified developer" block).
  xattr -dr com.apple.quarantine "$app_dest" 2>/dev/null || true
  ok "Pex installed at $app_dest"

  if [ -n "$INPUT_FD" ]; then
    prompt_yn "      Launch Pex now? [Y/n] "
    if [ -z "${REPLY:-}" ] || [ "$REPLY" = Y ] || [ "$REPLY" = y ]; then
      open "$app_dest"
    fi
  fi
}

# ── Binary-install path ───────────────────────
# Returns 0 on success; non-zero if no matching asset was found or any
# download/extract step failed. The caller falls back to source-build on
# non-zero.
try_binary_install() {
  if ! command -v curl >/dev/null 2>&1; then
    warn "curl not found — cannot download release binaries."
    return 1
  fi
  if ! command -v python3 >/dev/null 2>&1; then
    warn "python3 not found — cannot parse GitHub release JSON."
    return 1
  fi

  header "Fetching latest Pex release"
  local api_url="https://api.github.com/repos/${PEX_REPO_SLUG}/releases/latest"

  local release_json
  if ! release_json="$(curl -fsSL -H 'Accept: application/vnd.github+json' "$api_url" 2>/dev/null)"; then
    warn "No published release at $api_url"
    return 1
  fi

  # Pick the right asset for our OS/arch. We let Python do the matching so
  # callers don't need jq. Asset name patterns come from Tauri's bundler.
  local picker
  picker=$(cat <<'PYEOF'
import json, sys, re, os
data = json.load(sys.stdin)
os_name = os.environ["OS"]
arch    = os.environ["ARCH"]

# Tauri arch aliases — what shows up in filenames.
arch_aliases = {
    "x86_64":  ["x86_64", "amd64", "x64"],
    "amd64":   ["x86_64", "amd64", "x64"],
    "arm64":   ["aarch64", "arm64"],
    "aarch64": ["aarch64", "arm64"],
}.get(arch, [arch])

# Preference order per OS. First match wins.
if os_name == "Darwin":
    suffixes = [".app.tar.gz", ".dmg"]
elif os_name == "Linux":
    # Try .deb / .rpm / .AppImage in that order.
    suffixes = [".deb", ".rpm", ".AppImage"]
else:
    print("", end="")
    sys.exit(0)

def matches(name, suffix):
    if not name.endswith(suffix):
        return False
    # Updater sidecar files end .app.tar.gz.sig — exclude.
    if name.endswith(".sig"):
        return False
    return any(a in name for a in arch_aliases)

for suffix in suffixes:
    for asset in data.get("assets", []):
        if matches(asset["name"], suffix):
            print(asset["browser_download_url"])
            print(asset["name"])
            sys.exit(0)

sys.exit(1)
PYEOF
)

  local picked
  if ! picked="$(OS="$OS" ARCH="$ARCH" printf '%s' "$release_json" | python3 -c "$picker" 2>/dev/null)"; then
    warn "No release asset matched OS=$OS arch=$ARCH."
    warn "Available assets:"
    printf '%s' "$release_json" | python3 -c "import json,sys; [print('   - ' + a['name']) for a in json.load(sys.stdin).get('assets',[])]" 2>/dev/null || true
    return 1
  fi

  local url asset_name
  url="$(printf '%s' "$picked" | sed -n '1p')"
  asset_name="$(printf '%s' "$picked" | sed -n '2p')"

  local release_tag
  release_tag="$(printf '%s' "$release_json" | python3 -c "import json,sys; print(json.load(sys.stdin).get('tag_name',''))" 2>/dev/null || true)"
  ok "Found release ${release_tag:-?} → $asset_name"

  local tmp
  tmp="$(mktemp -d -t pex-bin.XXXXXX)"
  trap 'rm -rf "$tmp"' RETURN

  step "Downloading $asset_name…"
  if ! curl -fL --progress-bar -o "$tmp/$asset_name" "$url"; then
    warn "Download failed."
    return 1
  fi

  case "$OS" in
    Darwin)
      header "Installing Pex"
      case "$asset_name" in
        *.app.tar.gz)
          step "Extracting…"
          mkdir -p "$tmp/extract"
          if ! tar -xzf "$tmp/$asset_name" -C "$tmp/extract"; then
            warn "Extract failed."
            return 1
          fi
          local app
          app="$(find "$tmp/extract" -maxdepth 2 -name 'Pex.app' -print -quit)"
          if [ -z "$app" ]; then
            warn "No Pex.app inside $asset_name."
            return 1
          fi
          install_macos_app "$app"
          ;;
        *.dmg)
          step "Mounting .dmg…"
          local mountpoint="$tmp/mount"
          mkdir -p "$mountpoint"
          if ! hdiutil attach -nobrowse -quiet -mountpoint "$mountpoint" "$tmp/$asset_name"; then
            warn "Failed to mount $asset_name."
            return 1
          fi
          trap 'hdiutil detach -quiet "'"$mountpoint"'" 2>/dev/null || true; rm -rf "'"$tmp"'"' RETURN
          local app
          app="$(find "$mountpoint" -maxdepth 2 -name 'Pex.app' -print -quit)"
          if [ -z "$app" ]; then
            warn "No Pex.app inside the .dmg."
            return 1
          fi
          install_macos_app "$app"
          hdiutil detach -quiet "$mountpoint" 2>/dev/null || true
          ;;
        *)
          warn "Don't know how to install $asset_name on macOS."
          return 1
          ;;
      esac
      ;;

    Linux)
      header "Installing Pex"
      case "$asset_name" in
        *.deb)
          if ! command -v dpkg >/dev/null 2>&1; then
            warn "dpkg not found — can't install $asset_name."
            return 1
          fi
          step "Installing .deb (requires sudo)…"
          sudo dpkg -i "$tmp/$asset_name" || sudo apt-get install -f -y
          ok "Pex installed"
          ;;
        *.rpm)
          if ! command -v rpm >/dev/null 2>&1; then
            warn "rpm not found — can't install $asset_name."
            return 1
          fi
          step "Installing .rpm (requires sudo)…"
          sudo rpm -i --replacepkgs "$tmp/$asset_name"
          ok "Pex installed"
          ;;
        *.AppImage)
          local dest="${HOME}/.local/bin/Pex.AppImage"
          mkdir -p "$(dirname "$dest")"
          cp "$tmp/$asset_name" "$dest"
          chmod +x "$dest"
          ok "AppImage installed at $dest"
          warn "Add ${HOME}/.local/bin to PATH if it isn't already."
          ;;
        *)
          warn "Don't know how to install $asset_name on Linux."
          return 1
          ;;
      esac
      ;;
  esac

  return 0
}

# ── Source-build path ─────────────────────────
source_build_install() {
  # Bootstrap a source tree if we don't already have one (e.g. piped from curl).
  local SCRIPT_DIR
  local _src="${BASH_SOURCE[0]:-}"
  if [ -n "$_src" ] && [ -f "$_src" ]; then
    SCRIPT_DIR="$(cd "$(dirname "$_src")" && pwd)"
  else
    SCRIPT_DIR=""
  fi

  if [ -z "$SCRIPT_DIR" ] || [ ! -f "$SCRIPT_DIR/src-tauri/tauri.conf.json" ]; then
    if ! command -v git >/dev/null 2>&1; then
      fail "git is required to bootstrap the Pex source. Install git and re-run."
      exit 1
    fi
    local TMP_DIR
    TMP_DIR="$(mktemp -d -t pex-install.XXXXXX)"
    trap 'rm -rf "$TMP_DIR"' EXIT
    step "Fetching Pex source ($PEX_REPO @ $PEX_REF)…"
    git clone --depth 1 --branch "$PEX_REF" "$PEX_REPO" "$TMP_DIR" >/dev/null 2>&1 \
      || git clone "$PEX_REPO" "$TMP_DIR" >/dev/null
    SCRIPT_DIR="$TMP_DIR"
  fi

  header "Checking prerequisites (source build)"

  if command -v rustc >/dev/null 2>&1; then
    ok "Rust $(rustc --version | awk '{print $2}')"
  else
    fail "Rust not found"
    echo "       Install: curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
    exit 1
  fi
  if command -v node >/dev/null 2>&1; then
    ok "Node.js $(node --version)"
  else
    fail "Node.js not found"
    echo "       Install: https://nodejs.org (LTS recommended)"
    exit 1
  fi
  if command -v npm >/dev/null 2>&1; then
    ok "npm $(npm --version)"
  else
    fail "npm not found — reinstall Node.js"
    exit 1
  fi

  if [ "$OS" = Linux ]; then
    header "Checking system libraries"
    local MISSING_DEPS=""
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
    check_lib "gtk4"           "gtk4"
    check_lib "libsoup-3.0"    "libsoup-3.0"

    if [ -n "$MISSING_DEPS" ]; then
      echo ""
      warn "Missing libraries:${MISSING_DEPS}"
      if command -v dnf >/dev/null 2>&1; then
        step "Detected dnf — run:"
        echo "      sudo dnf install webkit2gtk4.1-devel gtk4-devel libsoup3-devel libappindicator-gtk3-devel openssl-devel curl wget file"
      elif command -v apt >/dev/null 2>&1; then
        step "Detected apt — run:"
        echo "      sudo apt install libwebkit2gtk-4.1-dev libgtk-4-dev libsoup-3.0-dev libappindicator3-dev libssl-dev curl wget file"
      elif command -v pacman >/dev/null 2>&1; then
        step "Detected pacman — run:"
        echo "      sudo pacman -S webkit2gtk-4.1 gtk4 libsoup3 libappindicator-gtk3 base-devel curl wget file"
      else
        warn "Unknown package manager — install GTK+WebKit deps manually."
      fi
      exit 1
    fi
  fi

  header "Building Pex"
  cd "$SCRIPT_DIR"
  step "Installing npm dependencies…"
  npm install --silent

  # Disable updater artifact generation — without TAURI_SIGNING_PRIVATE_KEY,
  # the minisign step prompts for a key path + password per artifact. Local
  # builds don't ship to other users' updaters, so skip it.
  local TAURI_BUILD_ARGS=( -- --config '{"bundle":{"createUpdaterArtifacts":false}}' )

  step "Running tauri build (this may take several minutes)…"
  npm run tauri build "${TAURI_BUILD_ARGS[@]}"

  header "Build complete"

  if [ "$OS" = Linux ]; then
    local BUNDLE_DIR="$SCRIPT_DIR/src-tauri/target/release/bundle"
    if [ -d "$BUNDLE_DIR/deb" ] && command -v dpkg >/dev/null 2>&1; then
      local DEB
      DEB="$(echo "$BUNDLE_DIR"/deb/*.deb | head -1)"
      if [ -f "$DEB" ]; then
        step "Installing .deb package (requires sudo)…"
        sudo dpkg -i "$DEB"
        ok "Pex installed"
        return 0
      fi
    fi
    if [ -d "$BUNDLE_DIR/rpm" ] && command -v rpm >/dev/null 2>&1; then
      local RPM
      RPM="$(echo "$BUNDLE_DIR"/rpm/*.rpm | head -1)"
      if [ -f "$RPM" ]; then
        step "Installing .rpm package (requires sudo)…"
        sudo rpm -i --replacepkgs "$RPM"
        ok "Pex installed"
        return 0
      fi
    fi
    if [ -d "$BUNDLE_DIR/appimage" ]; then
      local APPIMAGE
      APPIMAGE="$(echo "$BUNDLE_DIR"/appimage/*.AppImage | head -1)"
      if [ -f "$APPIMAGE" ]; then
        chmod +x "$APPIMAGE"
        ok "AppImage ready: $APPIMAGE"
        return 0
      fi
    fi
    warn "No installable package found — check $BUNDLE_DIR"
  elif [ "$OS" = Darwin ]; then
    install_macos_app "$SCRIPT_DIR/src-tauri/target/release/bundle/macos/Pex.app"
  fi
}

# ── Dispatch ──────────────────────────────────
if [ "$FROM_SOURCE" = "1" ]; then
  step "Forced source build (--from-source / PEX_FROM_SOURCE=1)"
  source_build_install
  exit 0
fi

if try_binary_install; then
  exit 0
fi

warn "Falling back to from-source build."
echo "  ${DIM}(use --from-source to skip the binary download attempt next time)${RESET}"
source_build_install
