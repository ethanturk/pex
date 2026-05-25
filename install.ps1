# ──────────────────────────────────────────────
# Pex — Azure DevOps PR Reviewer
# Windows install script (PowerShell)
# ──────────────────────────────────────────────

$ErrorActionPreference = "Stop"
Set-StrictMode -Version Latest

$ScriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path

function Write-Step  { Write-Host "  → " -NoNewline -ForegroundColor DarkGray; Write-Host $args }
function Write-OK    { Write-Host "  ✔ " -NoNewline -ForegroundColor Green;   Write-Host $args }
function Write-Warn  { Write-Host "  ⚠ " -NoNewline -ForegroundColor Yellow;  Write-Host $args }
function Write-Fail  { Write-Host "  ✘ " -NoNewline -ForegroundColor Red;     Write-Host $args }

Write-Host ""
Write-Host "── Checking prerequisites" -ForegroundColor White

# ── Rust ──────────────────────────────────────
try {
  $rustVer = (rustc --version)
  Write-OK "Rust $rustVer"
} catch {
  Write-Fail "Rust not found"
  Write-Host "       Install: https://rustup.rs"
  exit 1
}

# ── Node.js ───────────────────────────────────
try {
  $nodeVer = (node --version)
  Write-OK "Node.js $nodeVer"
} catch {
  Write-Fail "Node.js not found"
  Write-Host "       Install: https://nodejs.org (LTS recommended)"
  exit 1
}

try {
  $npmVer = (npm --version)
  Write-OK "npm $npmVer"
} catch {
  Write-Fail "npm not found — reinstall Node.js"
  exit 1
}

# ── WebView2 ──────────────────────────────────
$webView2Path = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\EdgeUpdate\Clients\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}" -ErrorAction SilentlyContinue
if ($webView2Path) {
  Write-OK "WebView2 Runtime found"
} else {
  # Tauri bundles WebView2 in the MSI, or the user installs it separately
  Write-Warn "WebView2 Runtime not detected — the Pex installer bundles it if needed"
}

# ── Visual C++ Redistributable ────────────────
# Tauri on Windows requires the VC++ runtime for the MSVC-built Rust binary
$vcRedist = Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\VisualStudio\14.0\VC\Runtimes\x64" -ErrorAction SilentlyContinue
if (-not $vcRedist) {
  Write-Warn "Visual C++ Redistributable not found"
  Write-Host "       Download: https://aka.ms/vs/17/release/vc_redist.x64.exe"
  Write-Host "       (The Pex MSI may bundle this — continuing anyway)"
}

# ── Build ─────────────────────────────────────
Write-Host ""
Write-Host "── Building Pex" -ForegroundColor White

Set-Location $ScriptDir

Write-Step "Installing npm dependencies…"
npm install --silent

Write-Step "Building for Windows (cargo tauri build)…"
npm run tauri build

Write-Host ""
Write-Host "── Build complete" -ForegroundColor White

# ── Install ───────────────────────────────────
$bundleDir = Join-Path $ScriptDir "src-tauri\target\release\bundle"

# Prefer the MSI installer
$msi = Get-ChildItem -Path "$bundleDir\msi" -Filter "*.msi" -ErrorAction SilentlyContinue | Select-Object -First 1
if ($msi) {
  Write-Step "Found MSI installer"
  Write-Host ""
  Write-Host "      Install $($msi.Name) ? [Y/n]" -NoNewline
  $answer = Read-Host
  if ($answer -eq "" -or $answer -eq "Y" -or $answer -eq "y") {
    Write-Step "Running MSI installer…"
    Start-Process msiexec.exe -ArgumentList "/i `"$($msi.FullName)`"" -Wait -Verb RunAs
    Write-OK "Pex installed"
  } else {
    Write-Step "Skipped. MSI at: $($msi.FullName)"
  }
} else {
  # Fallback — NSIS installer
  $nsis = Get-ChildItem -Path "$bundleDir\nsis" -Filter "*.exe" -ErrorAction SilentlyContinue | Select-Object -First 1
  if ($nsis) {
    Write-Step "Found NSIS installer"
    Write-Host ""
    Write-Host "      Install $($nsis.Name) ? [Y/n]" -NoNewline
    $answer = Read-Host
    if ($answer -eq "" -or $answer -eq "Y" -or $answer -eq "y") {
      Write-Step "Running NSIS installer…"
      Start-Process -FilePath $nsis.FullName -Wait -Verb RunAs
      Write-OK "Pex installed"
    } else {
      Write-Step "Skipped. Installer at: $($nsis.FullName)"
    }
  } else {
    Write-Warn "No installer found — check $bundleDir"
    Get-ChildItem $bundleDir -Recurse -Filter "*.msi" -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "       $_" }
    Get-ChildItem $bundleDir -Recurse -Filter "*.exe" -ErrorAction SilentlyContinue | ForEach-Object { Write-Host "       $_" }
  }
}

Write-Host ""
Write-Host "Done. Launch Pex from the Start Menu or run 'pex' in a terminal." -ForegroundColor Green
