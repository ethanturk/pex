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

One-liner — no clone required. The script fetches the source, checks prerequisites, builds, and installs.

**Linux / macOS**

```bash
curl -fsSL https://raw.githubusercontent.com/ethanturk/pex/master/install.sh | bash
```

**Windows (PowerShell)**

```powershell
iwr -useb https://raw.githubusercontent.com/ethanturk/pex/master/install.ps1 | iex
```

Or, from a cloned repo:

```bash
./install.sh        # Linux / macOS
.\install.ps1       # Windows
```

Pin a specific tag or branch with `PEX_REF` (e.g. `PEX_REF=v0.2.0 curl … | bash`).

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

## Releases

Releases are built automatically via GitHub Actions when a `v*` tag is pushed.

### One-time setup

1. **Generate a signing key pair** — the updater uses this to verify that updates are authentic:

   ```bash
   cargo tauri signer generate -w ~/.tauri/pex.key
   ```

   This prints a public key (starts with `dW50cnVzdGVk...`) and saves the private key to `~/.tauri/pex.key`.

2. **Set the public key** in `src-tauri/tauri.conf.json` → `plugins.updater.pubkey`.

3. **Add the private key as a GitHub Secret:**

   - Go to your repo → Settings → Secrets and variables → Actions
   - Add `TAURI_SIGNING_PRIVATE_KEY` with the content of `~/.tauri/pex.key`
   - If you set a password, add `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` as well

### Cutting a release

```bash
git tag v0.1.0
git push origin v0.1.0
```

The GitHub Action builds for Linux, macOS, and Windows, signs everything, and creates a draft GitHub Release with all platform installers + updater artifacts.

Existing installs will detect the new version via the updater plugin and prompt to update.

## License

MIT — see [LICENSE](LICENSE).
