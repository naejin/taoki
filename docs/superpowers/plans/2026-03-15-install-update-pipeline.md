# Install & Update Pipeline Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add one-liner install/update scripts and a GitHub Actions release pipeline so users can install Taoki without a Rust toolchain.

**Architecture:** GitHub Actions cross-compiles the binary for 5 targets on tag push, uploads tarballs to a GitHub Release with SHA256 checksums. Shell/PowerShell install scripts download the correct binary, verify the checksum, and register the plugin with Claude Code.

**Tech Stack:** GitHub Actions, Bash, PowerShell, `cross` (for Linux ARM64 cross-compilation)

---

## Chunk 0: Branch setup

### Task 0: Create the feature branch

- [ ] **Step 1: Create the branch from current master**

```bash
git checkout -b feat/install-update-pipeline
```

All subsequent tasks commit to this branch.

---

## Chunk 1: Binary --version flag and updated run.sh

### Task 1: Add --version flag to the binary

**Files:**
- Modify: `src/main.rs:89-96`

- [ ] **Step 1: Add --version check before MCP loop**

In `src/main.rs`, insert these 5 lines at the very start of `main()` (line 90, before `eprintln!("taoki: MCP server starting")`):

```rust
    let args: Vec<String> = std::env::args().collect();
    if args.len() > 1 && args[1] == "--version" {
        println!("taoki {}", env!("CARGO_PKG_VERSION"));
        return;
    }
```

The existing `eprintln!("taoki: MCP server starting");` and everything after it stays unchanged. `plugin.json` is not modified — no changes needed there.

- [ ] **Step 2: Verify it works**

Run: `cargo run -- --version`
Expected output: `taoki 0.1.0`

Run: `cargo test`
Expected: all 29 tests pass (no regressions)

- [ ] **Step 3: Commit**

```bash
git add src/main.rs
git commit -m "feat: add --version flag for install script health checks"
```

### Task 2: Update scripts/run.sh with fallback logic

**Files:**
- Modify: `scripts/run.sh`

- [ ] **Step 1: Update run.sh**

Replace `scripts/run.sh` with:

```bash
#!/usr/bin/env bash
set -euo pipefail
DIR="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$DIR/target/release/taoki"
if [ -f "$BIN" ]; then
  exec "$BIN" "$@"
elif [ -f "$DIR/Cargo.toml" ]; then
  cargo build --release --manifest-path "$DIR/Cargo.toml" >&2
  exec "$BIN" "$@"
else
  echo "Error: taoki binary not found. Re-run the install script to download it." >&2
  echo "  curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash" >&2
  exit 1
fi
```

- [ ] **Step 2: Verify run.sh still works for source builds**

Run: `rm -f target/release/taoki && bash scripts/run.sh --version`
Expected: builds, then prints `taoki 0.1.0`

- [ ] **Step 3: Commit**

```bash
git add scripts/run.sh
git commit -m "feat: add fallback logic to run.sh for pre-built binary installs"
```

### Task 3: Create scripts/run.cmd for Windows

**Files:**
- Create: `scripts/run.cmd`

- [ ] **Step 1: Create run.cmd**

```cmd
@echo off
set "DIR=%~dp0.."
set "BIN=%DIR%\target\release\taoki.exe"
if exist "%BIN%" (
    "%BIN%" %*
    exit /b %errorlevel%
)
where cargo >nul 2>&1
if %errorlevel% equ 0 (
    cargo build --release --manifest-path "%DIR%\Cargo.toml" 2>&1
    "%BIN%" %*
    exit /b %errorlevel%
)
echo Error: taoki binary not found. Re-run the install script to download it. >&2
echo   irm https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.ps1 ^| iex >&2
exit /b 1
```

- [ ] **Step 2: Commit**

```bash
git add scripts/run.cmd
git commit -m "feat: add Windows entry point script for MCP server"
```

## Chunk 2: GitHub Actions release pipeline

### Task 4: Create the release workflow

**Files:**
- Create: `.github/workflows/release.yml`

- [ ] **Step 1: Create the workflow file**

```yaml
name: Release

on:
  push:
    tags:
      - 'v*'

permissions:
  contents: write

jobs:
  build:
    name: Build ${{ matrix.target }}
    runs-on: ${{ matrix.runner }}
    strategy:
      fail-fast: false
      matrix:
        include:
          - target: x86_64-unknown-linux-gnu
            runner: ubuntu-latest
            artifact: taoki-linux-x86_64
            ext: tar.gz
          - target: aarch64-unknown-linux-gnu
            runner: ubuntu-latest
            artifact: taoki-linux-aarch64
            ext: tar.gz
            cross: true
          - target: x86_64-apple-darwin
            runner: macos-13
            artifact: taoki-macos-x86_64
            ext: tar.gz
          - target: aarch64-apple-darwin
            runner: macos-latest
            artifact: taoki-macos-aarch64
            ext: tar.gz
          - target: x86_64-pc-windows-msvc
            runner: windows-latest
            artifact: taoki-windows-x86_64
            ext: zip

    steps:
      - uses: actions/checkout@v4

      - name: Install Rust toolchain
        uses: dtolnay/rust-toolchain@stable
        with:
          targets: ${{ matrix.target }}

      - name: Install cross (Linux ARM64)
        if: matrix.cross
        run: cargo install cross --locked

      - name: Build
        run: |
          if [ "${{ matrix.cross }}" = "true" ]; then
            cross build --release --target ${{ matrix.target }}
          else
            cargo build --release --target ${{ matrix.target }}
          fi
        shell: bash

      - name: Package (Unix)
        if: matrix.ext == 'tar.gz'
        run: |
          mkdir -p staging/taoki/target/release
          cp target/${{ matrix.target }}/release/taoki staging/taoki/target/release/
          cp -r .claude-plugin staging/taoki/
          cp -r commands staging/taoki/
          cp -r skills staging/taoki/
          mkdir -p staging/taoki/scripts
          cp scripts/run.sh staging/taoki/scripts/
          cp scripts/run.cmd staging/taoki/scripts/
          chmod +x staging/taoki/scripts/run.sh
          chmod +x staging/taoki/target/release/taoki
          cd staging
          tar czf ../${{ matrix.artifact }}.${{ matrix.ext }} taoki
        shell: bash

      - name: Package (Windows)
        if: matrix.ext == 'zip'
        run: |
          mkdir -p staging/taoki/target/release
          cp target/${{ matrix.target }}/release/taoki.exe staging/taoki/target/release/
          cp -r .claude-plugin staging/taoki/
          cp -r commands staging/taoki/
          cp -r skills staging/taoki/
          mkdir -p staging/taoki/scripts
          cp scripts/run.sh staging/taoki/scripts/
          cp scripts/run.cmd staging/taoki/scripts/
          cd staging
          7z a -tzip ../${{ matrix.artifact }}.${{ matrix.ext }} taoki
        shell: bash

      - name: Upload artifact
        uses: actions/upload-artifact@v4
        with:
          name: ${{ matrix.artifact }}
          path: ${{ matrix.artifact }}.${{ matrix.ext }}

  release:
    name: Create Release
    needs: build
    runs-on: ubuntu-latest
    steps:
      - name: Download all artifacts
        uses: actions/download-artifact@v4
        with:
          path: artifacts

      - name: Collect release files
        run: |
          mkdir release
          find artifacts -type f \( -name '*.tar.gz' -o -name '*.zip' \) -exec cp {} release/ \;

      - name: Generate checksums
        run: |
          cd release
          sha256sum * > checksums.txt

      - name: Create GitHub Release
        uses: softprops/action-gh-release@v2
        with:
          generate_release_notes: true
          files: release/*
```

- [ ] **Step 2: Verify the workflow syntax**

Run: `python3 -c "import yaml" 2>/dev/null && python3 -c "import sys,yaml; yaml.safe_load(open('.github/workflows/release.yml')); print('Valid YAML')" || echo "pyyaml not installed, skipping YAML validation"`
If pyyaml is available, expected: `Valid YAML`. Otherwise skip — the workflow will be validated by GitHub on push.

- [ ] **Step 3: Commit**

```bash
git add .github/workflows/release.yml
git commit -m "ci: add cross-platform release pipeline for GitHub Actions"
```

## Chunk 3: Install scripts

### Task 5: Create scripts/install.sh

**Files:**
- Create: `scripts/install.sh`

- [ ] **Step 1: Create the install script**

```bash
#!/usr/bin/env bash
set -euo pipefail

REPO="naejin/taoki"
INSTALL_DIR="$HOME/.claude/plugins/taoki"

# Colors (only if terminal supports it)
if [ -t 1 ]; then
  BOLD='\033[1m'
  GREEN='\033[0;32m'
  RED='\033[0;31m'
  RESET='\033[0m'
else
  BOLD='' GREEN='' RED='' RESET=''
fi

info()  { echo -e "${BOLD}taoki:${RESET} $1"; }
error() { echo -e "${RED}error:${RESET} $1" >&2; }

# Cleanup temp directory on exit
TMPDIR_INSTALL=""
cleanup() { [ -n "$TMPDIR_INSTALL" ] && rm -rf "$TMPDIR_INSTALL"; }
trap cleanup EXIT

# Detect platform
OS="$(uname -s)"
ARCH="$(uname -m)"

case "$OS" in
  Linux)  PLATFORM="linux" ;;
  Darwin) PLATFORM="macos" ;;
  *) error "Unsupported OS: $OS"; exit 1 ;;
esac

case "$ARCH" in
  x86_64|amd64)   ARCH="x86_64" ;;
  arm64|aarch64)   ARCH="aarch64" ;;
  *) error "Unsupported architecture: $ARCH"; exit 1 ;;
esac

ARTIFACT="taoki-${PLATFORM}-${ARCH}.tar.gz"

# Determine version
VERSION="${1:-}"
if [ -z "$VERSION" ]; then
  info "Fetching latest release..."
  API_URL="https://api.github.com/repos/${REPO}/releases/latest"
  CURL_ARGS=(-sSL -w "\n%{http_code}")
  [ -n "${GITHUB_TOKEN:-}" ] && CURL_ARGS+=(-H "Authorization: token $GITHUB_TOKEN")

  HTTP_RESPONSE=$(curl "${CURL_ARGS[@]}" "$API_URL" 2>/dev/null) || {
    error "Failed to connect to GitHub API."
    error "Check your internet connection or specify a version:"
    error "  curl -fsSL ... | bash -s -- v0.2.0"
    exit 1
  }

  HTTP_CODE=$(echo "$HTTP_RESPONSE" | tail -1)
  RESPONSE_BODY=$(echo "$HTTP_RESPONSE" | sed '$d')

  if [ "$HTTP_CODE" = "403" ]; then
    error "GitHub API rate limit exceeded."
    error "Set GITHUB_TOKEN env var or specify a version directly:"
    error "  curl -fsSL ... | bash -s -- v0.2.0"
    exit 1
  fi

  if [ "$HTTP_CODE" != "200" ]; then
    error "GitHub API returned HTTP $HTTP_CODE."
    error "Specify a version directly:"
    error "  curl -fsSL ... | bash -s -- v0.2.0"
    exit 1
  fi

  VERSION=$(echo "$RESPONSE_BODY" | grep '"tag_name"' | head -1 | sed 's/.*"tag_name"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/')
  if [ -z "$VERSION" ]; then
    error "Could not determine latest version. Specify one manually:"
    error "  curl -fsSL ... | bash -s -- v0.2.0"
    exit 1
  fi
fi

info "Installing taoki ${VERSION} (${PLATFORM}-${ARCH})..."

# Download
TMPDIR_INSTALL="$(mktemp -d)"
DOWNLOAD_URL="https://github.com/${REPO}/releases/download/${VERSION}/${ARTIFACT}"
CHECKSUM_URL="https://github.com/${REPO}/releases/download/${VERSION}/checksums.txt"

curl -fsSL -o "$TMPDIR_INSTALL/$ARTIFACT" "$DOWNLOAD_URL" || {
  error "Failed to download ${ARTIFACT} for version ${VERSION}"
  error "Check that the version exists: https://github.com/${REPO}/releases"
  exit 1
}

# Verify checksum
curl -fsSL -o "$TMPDIR_INSTALL/checksums.txt" "$CHECKSUM_URL" || {
  error "Failed to download checksums. Aborting for safety."
  exit 1
}

EXPECTED_SUM=$(grep "$ARTIFACT" "$TMPDIR_INSTALL/checksums.txt" | awk '{print $1}')
if [ -z "$EXPECTED_SUM" ]; then
  error "Checksum for ${ARTIFACT} not found in checksums.txt"
  exit 1
fi

if command -v sha256sum >/dev/null 2>&1; then
  ACTUAL_SUM=$(sha256sum "$TMPDIR_INSTALL/$ARTIFACT" | awk '{print $1}')
elif command -v shasum >/dev/null 2>&1; then
  ACTUAL_SUM=$(shasum -a 256 "$TMPDIR_INSTALL/$ARTIFACT" | awk '{print $1}')
else
  error "Neither sha256sum nor shasum found. Cannot verify checksum."
  exit 1
fi

if [ "$EXPECTED_SUM" != "$ACTUAL_SUM" ]; then
  error "Checksum verification failed!"
  error "  Expected: $EXPECTED_SUM"
  error "  Got:      $ACTUAL_SUM"
  exit 1
fi

info "Checksum verified."

# Extract to staging
STAGING="$TMPDIR_INSTALL/staging"
mkdir -p "$STAGING"
tar xzf "$TMPDIR_INSTALL/$ARTIFACT" -C "$STAGING"

# Atomic swap into install directory
mkdir -p "$(dirname "$INSTALL_DIR")"
if [ -d "$INSTALL_DIR" ]; then
  mv "$INSTALL_DIR" "${INSTALL_DIR}.bak"
fi
mv "$STAGING/taoki" "$INSTALL_DIR"
rm -rf "${INSTALL_DIR}.bak"

# Verify binary
if ! "$INSTALL_DIR/target/release/taoki" --version >/dev/null 2>&1; then
  error "Binary verification failed. The download may be corrupted."
  exit 1
fi

INSTALLED_VERSION=$("$INSTALL_DIR/target/release/taoki" --version 2>/dev/null || echo "unknown")
info "Installed ${INSTALLED_VERSION}"

# Register plugin with Claude Code
if command -v claude >/dev/null 2>&1; then
  info "Registering plugin with Claude Code..."
  claude plugin add "$INSTALL_DIR" 2>/dev/null && {
    info "Plugin registered successfully."
  } || {
    info "Plugin may already be registered. Run manually if needed:"
    info "  claude plugin add $INSTALL_DIR"
  }
else
  info "Claude Code not found on PATH. Register the plugin manually:"
  info "  claude plugin add $INSTALL_DIR"
fi

echo ""
info "${GREEN}Taoki installed successfully!${RESET}"
info "It will be available in your next Claude Code session."
```

- [ ] **Step 2: Make it executable**

Run: `chmod +x scripts/install.sh`

- [ ] **Step 3: Verify the script parses correctly**

Run: `bash -n scripts/install.sh && echo "Syntax OK"`
Expected: `Syntax OK`

- [ ] **Step 4: Commit**

```bash
git add scripts/install.sh
git commit -m "feat: add one-liner install script for Linux/macOS"
```

### Task 6: Create scripts/install.ps1

**Files:**
- Create: `scripts/install.ps1`

- [ ] **Step 1: Create the PowerShell install script**

```powershell
$ErrorActionPreference = "Stop"

$Repo = "naejin/taoki"
$InstallDir = Join-Path $env:USERPROFILE ".claude\plugins\taoki"

function Write-Info($msg) { Write-Host "taoki: $msg" }
function Write-Err($msg) { Write-Host "error: $msg" -ForegroundColor Red }

# Detect architecture
$Arch = $env:PROCESSOR_ARCHITECTURE
if ($Arch -ne "AMD64") {
    Write-Err "Unsupported architecture: $Arch"
    exit 1
}

$Artifact = "taoki-windows-x86_64.zip"

# Determine version
$Version = $env:TAOKI_VERSION
if (-not $Version) {
    Write-Info "Fetching latest release..."
    $ApiUrl = "https://api.github.com/repos/$Repo/releases/latest"
    try {
        $headers = @{ "User-Agent" = "taoki-installer" }
        if ($env:GITHUB_TOKEN) {
            $headers["Authorization"] = "token $($env:GITHUB_TOKEN)"
        }
        $release = Invoke-RestMethod -Uri $ApiUrl -Headers $headers
        $Version = $release.tag_name
    } catch {
        Write-Err "Failed to fetch latest release. $_"
        Write-Err "If rate-limited, set GITHUB_TOKEN or TAOKI_VERSION env var."
        exit 1
    }
}

Write-Info "Installing taoki $Version (windows-x86_64)..."

# Download
$TmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "taoki-install-$(Get-Random)"
New-Item -ItemType Directory -Path $TmpDir -Force | Out-Null

try {
    $DownloadUrl = "https://github.com/$Repo/releases/download/$Version/$Artifact"
    $ChecksumUrl = "https://github.com/$Repo/releases/download/$Version/checksums.txt"
    $ArtifactPath = Join-Path $TmpDir $Artifact
    $ChecksumPath = Join-Path $TmpDir "checksums.txt"

    Invoke-WebRequest -Uri $DownloadUrl -OutFile $ArtifactPath -UseBasicParsing
    Invoke-WebRequest -Uri $ChecksumUrl -OutFile $ChecksumPath -UseBasicParsing

    # Verify checksum
    $ExpectedLine = Get-Content $ChecksumPath | Where-Object { $_ -match $Artifact }
    if (-not $ExpectedLine) {
        Write-Err "Checksum for $Artifact not found."
        exit 1
    }
    $ExpectedSum = ($ExpectedLine -split '\s+')[0]
    $ActualSum = (Get-FileHash -Path $ArtifactPath -Algorithm SHA256).Hash.ToLower()

    if ($ExpectedSum -ne $ActualSum) {
        Write-Err "Checksum verification failed!"
        Write-Err "  Expected: $ExpectedSum"
        Write-Err "  Got:      $ActualSum"
        exit 1
    }

    Write-Info "Checksum verified."

    # Extract to staging
    $StagingDir = Join-Path $TmpDir "staging"
    Expand-Archive -Path $ArtifactPath -DestinationPath $StagingDir -Force

    # Atomic swap
    $ParentDir = Split-Path $InstallDir -Parent
    if (-not (Test-Path $ParentDir)) {
        New-Item -ItemType Directory -Path $ParentDir -Force | Out-Null
    }
    if (Test-Path $InstallDir) {
        $BackupDir = "$InstallDir.bak"
        if (Test-Path $BackupDir) { Remove-Item -Recurse -Force $BackupDir }
        Rename-Item -Path $InstallDir -NewName "taoki.bak"
    }
    Move-Item -Path (Join-Path $StagingDir "taoki") -Destination $InstallDir
    if (Test-Path "$($ParentDir)\taoki.bak") {
        Remove-Item -Recurse -Force "$($ParentDir)\taoki.bak"
    }

    # Verify binary
    $Binary = Join-Path $InstallDir "target\release\taoki.exe"
    $VersionOutput = & $Binary --version 2>&1
    Write-Info "Installed $VersionOutput"

    # Register plugin
    $ClaudePath = Get-Command claude -ErrorAction SilentlyContinue
    if ($ClaudePath) {
        Write-Info "Registering plugin with Claude Code..."
        try {
            & claude plugin add $InstallDir 2>$null
            Write-Info "Plugin registered successfully."
        } catch {
            Write-Info "Plugin may already be registered. Run manually if needed:"
            Write-Info "  claude plugin add $InstallDir"
        }
    } else {
        Write-Info "Claude Code not found on PATH. Register the plugin manually:"
        Write-Info "  claude plugin add $InstallDir"
    }

    Write-Host ""
    Write-Info "Taoki installed successfully!"
    Write-Info "It will be available in your next Claude Code session."
} finally {
    Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
}
```

- [ ] **Step 2: Verify syntax (if PowerShell available)**

Run: `pwsh -Command "Get-Command -Syntax scripts/install.ps1" 2>/dev/null && echo "Syntax OK" || echo "PowerShell not available, skipping syntax check"`
If pwsh is available, expected: no errors. Otherwise skip — the script uses only PS 5.1+ compatible cmdlets.

- [ ] **Step 3: Commit**

```bash
git add scripts/install.ps1
git commit -m "feat: add PowerShell install script for Windows"
```

## Chunk 4: README update

### Task 7: Update README.md install and update sections

**Files:**
- Modify: `README.md:24-48`

- [ ] **Step 1: Replace the Install and Update sections**

In `README.md`, replace the entire block from `## Install` (line 24) through the blank line after the `## Update` section (line 49, just before `## How It Works` on line 50). Use the Edit tool to replace this old_string:

Old (lines 24-49):
```
## Install

Requires a Rust toolchain (install via [rustup](https://rustup.rs/) if needed).
... (entire Install section) ...
## Update
... (entire Update section) ...
```

New content to insert at that location (note: this contains fenced code blocks — use the Write/Edit tool, not inline markdown):

The new `## Install` section should have:
- `### Quick install (no Rust required)` with `curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash` for Linux/macOS
- PowerShell one-liner: `irm https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.ps1 | iex` for Windows
- Version pinning example: `curl -fsSL ... | bash -s -- v0.2.0`
- `### Build from source` with `git clone` + `claude plugin add ./taoki` instructions

The new `## Update` section should have:
- Re-run the install script as the primary method (same curl/irm commands)
- `cd /path/to/taoki && git pull` for source builds

See the spec's "Updated README.md" section for the exact content structure.

- [ ] **Step 2: Verify README renders correctly**

Run: `head -60 README.md` — visually check the new sections look right.

- [ ] **Step 3: Commit**

```bash
git add README.md
git commit -m "docs: update install and update instructions for pre-built binaries"
```

