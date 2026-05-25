#!/usr/bin/env bash
set -euo pipefail

# ============================================================
# Pex Install Script
# Installs all prerequisites for building and running Pex:
#   - Rust (via rustup)
#   - .NET 10 SDK (via dotnet-install.sh)
#   - Purist (AI PR review engine)
# ============================================================

RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BOLD='\033[1m'
NC='\033[0m' # No Color

DOTNET_INSTALL_DIR="${DOTNET_INSTALL_DIR:-$HOME/.dotnet}"
PURIST_DIR="${PURIST_DIR:-$HOME/repos/purist}"
PURIST_REPO="https://github.com/ethanturk/purist.git"

section() { echo -e "\n${BOLD}==>${NC} ${BOLD}$*${NC}"; }
ok()     { echo -e "  ${GREEN}✓${NC} $*"; }
info()   { echo -e "  ${YELLOW}→${NC} $*"; }
fail()   { echo -e "  ${RED}✗${NC} $*"; }

# ---- Rust ----
section "Checking Rust..."

if command -v rustup &>/dev/null && command -v cargo &>/dev/null; then
    RUST_VERSION=$(rustc --version 2>/dev/null || echo "unknown")
    ok "Rust found: $RUST_VERSION"
else
    info "Rust not found — installing via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
    ok "Rust installed: $(rustc --version)"
fi

# ---- .NET 10 SDK ----
section "Checking .NET SDK..."

# Try to resolve dotnet (look in PATH and ~/.dotnet)
find_dotnet() {
    for candidate in dotnet "$DOTNET_INSTALL_DIR/dotnet"; do
        if command -v "$candidate" &>/dev/null || [ -x "$candidate" ]; then
            echo "$candidate"
            return 0
        fi
    done
    return 1
}

DOTNET_BIN=$(find_dotnet || true)

if [ -n "$DOTNET_BIN" ]; then
    DOTNET_VERSION=$("$DOTNET_BIN" --version 2>/dev/null || echo "0")
    DOTNET_MAJOR=$(echo "$DOTNET_VERSION" | cut -d. -f1)
    if [ "$DOTNET_MAJOR" -ge 10 ]; then
        ok ".NET SDK $DOTNET_VERSION found (≥ 10)"
    else
        info ".NET SDK $DOTNET_VERSION found — but need ≥ 10. Upgrading..."
        NEED_INSTALL=true
    fi
else
    info ".NET SDK not found — installing..."
    NEED_INSTALL=true
fi

if [ "${NEED_INSTALL:-false}" = true ]; then
    info "Downloading dotnet-install.sh..."
    curl -sL https://dot.net/v1/dotnet-install.sh -o /tmp/dotnet-install.sh
    chmod +x /tmp/dotnet-install.sh
    /tmp/dotnet-install.sh --channel 10.0 --install-dir "$DOTNET_INSTALL_DIR"
    rm /tmp/dotnet-install.sh

    DOTNET_BIN="$DOTNET_INSTALL_DIR/dotnet"
    DOTNET_VERSION=$("$DOTNET_BIN" --version)
    ok ".NET SDK $DOTNET_VERSION installed to $DOTNET_INSTALL_DIR"

    # Add to PATH for current session
    export PATH="$DOTNET_INSTALL_DIR:$PATH"
fi

# ---- Purist ----
section "Checking Purist..."

if [ -f "$PURIST_DIR/src/Purist/Purist.csproj" ]; then
    ok "Purist found at $PURIST_DIR"
else
    info "Cloning Purist to $PURIST_DIR..."
    mkdir -p "$(dirname "$PURIST_DIR")"
    git clone "$PURIST_REPO" "$PURIST_DIR" --depth 1
    ok "Purist cloned to $PURIST_DIR"
fi

# Build Purist to verify .NET 10 works
info "Building Purist (verifying .NET SDK)..."
DOTNET_ROOT="$DOTNET_INSTALL_DIR" "$DOTNET_BIN" build "$PURIST_DIR/src/Purist/Purist.csproj" -c Release --nologo -v q 2>&1 | tail -1
ok "Purist builds successfully"

# ---- PATH setup hint ----
section "PATH Setup"

NEED_PATH=false
if ! command -v cargo &>/dev/null; then
    echo "  export PATH=\"\$HOME/.cargo/bin:\$PATH\""
    NEED_PATH=true
fi
if ! command -v dotnet &>/dev/null; then
    echo "  export PATH=\"$DOTNET_INSTALL_DIR:\$PATH\""
    echo "  export DOTNET_ROOT=\"$DOTNET_INSTALL_DIR\""
    NEED_PATH=true
fi

if [ "$NEED_PATH" = true ]; then
    echo ""
    echo -e "${YELLOW}Add the above lines to your ~/.bashrc or ~/.zshrc for permanent PATH setup.${NC}"
else
    ok "rustup and dotnet are already on PATH"
fi

# ---- Summary ----
echo ""
echo -e "${BOLD}══════════════════════════════════════════${NC}"
echo -e "${GREEN}${BOLD}Pex prerequisites are ready.${NC}"
echo ""
echo -e "  Rust:     $(rustc --version 2>/dev/null || echo 'check PATH')"
echo -e "  .NET:     $("$DOTNET_BIN" --version 2>/dev/null)"
echo -e "  Purist:   $PURIST_DIR"
echo ""
echo -e "  Next:     cd ~/projects/pex && npm install && npm run tauri dev"
echo -e "${BOLD}══════════════════════════════════════════${NC}"
