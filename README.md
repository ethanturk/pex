# Pex

**Azure DevOps PR Reviewer** — a fast, native desktop app for code reviews.

Built with [Tauri](https://tauri.app) (Rust backend) and Preact.

<p align="center">
  <img src="src-tauri/icons/128x128.png" alt="Pex icon" width="96" />
</p>

## Features

- **View PRs** — org/project/repo picker; list PRs by status and author
- **Diff viewer** — syntax-highlighted, side-by-side diffs with virtual scrolling
- **File tracking** — mark files as viewed; `j`/`k` keyboard navigation
- **Inline comments** — post and reply to comment threads on specific diff lines
- **PR actions** — approve (+10), approve with suggestions (+5), wait for author (−5), reject (−10)
- **Multi-org** — quick switcher; credentials per org stored in OS keyring
- **Auth** — PAT token entry or browser-based OAuth 2.0 (AAD device code)
- **Dark + light theme** — respects system preference, toggleable
- **Iteration support** — iteration dropdown recalculates diffs per base commit
- **Conflict markers** — inline `<<<<<<<`/`>>>>>>>` highlighted in red

## Install

### Quick install (recommended)

```bash
# Linux / macOS
./install.sh

# Windows (PowerShell)
.\install.ps1
```

The scripts check prerequisites, build from source, and install the platform package.

### Manual build

```bash
# 1. Install prerequisites
#    - Rust  (https://rustup.rs)
#    - Node.js >= 18  (https://nodejs.org)
#    - Linux: webkit2gtk-4.1, gtk4, libsoup-3.0 (see below)

# 2. Build
npm install
npm run tauri build

# 3. Install
#    Linux:   sudo dpkg -i src-tauri/target/release/bundle/deb/*.deb
#    macOS:   open src-tauri/target/release/bundle/dmg/*.dmg
#    Windows: src-tauri/target/release/bundle/msi/*.msi
```

### Linux system dependencies

| Distro | Command |
|--------|---------|
| **Fedora** | `sudo dnf install webkit2gtk4.1-devel gtk4-devel libsoup3-devel libappindicator-gtk3-devel` |
| **Ubuntu/Debian** | `sudo apt install libwebkit2gtk-4.1-dev libgtk-4-dev libsoup-3.0-dev libappindicator3-dev` |
| **Arch** | `sudo pacman -S webkit2gtk-4.1 gtk4 libsoup3 libappindicator-gtk3 base-devel` |

## Auth setup

Pex connects to Azure DevOps using:

- **PAT** — paste a Personal Access Token with `Code (Read & Write)` scope
- **OAuth 2.0** — "Sign in with browser" opens the Azure AD device code flow

Credentials are stored in your OS keyring (Keychain on macOS, Secret Service on Linux, Credential Manager on Windows).

### Registering a Microsoft Entra ID app (for OAuth)

1. Go to https://app.vsaex.visualstudio.com/app/register
2. Register a new app with:
   - **Redirect URL:** `http://localhost:14820/callback`
   - **Scopes:** `Code (Read & Write)`, `Work Items (Read)`
3. Copy the client ID into `~/.config/pex/client_id`

## Keyboard shortcuts

| Key | Action |
|-----|--------|
| `j` | Next file in diff |
| `k` | Previous file in diff |
| `v` | Toggle file as viewed |
| `a` | Approve PR |

## Tech stack

- **Rust** — Tauri v2, reqwest, similarity (diffs), syntect (highlighting), rusqlite (cache), keyring
- **Frontend** — Preact, TypeScript, Tailwind CSS v4, Signals
- **Packaging** — `.deb` / `.rpm` / `.AppImage` (Linux), `.dmg` (macOS), `.msi` (Windows)
- **Auto-updater** — tauri-plugin-updater via GitHub Releases

## License

MIT — see [LICENSE](LICENSE).
