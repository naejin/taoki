# Taoki Install & Update Pipeline Design

## Problem

Taoki is a Claude Code plugin that requires a Rust toolchain to build from source. This is a significant barrier for users who don't have Rust installed. The current install process requires cloning the repo to an arbitrary path, and the update process requires manually deleting the binary to trigger a rebuild.

## Goal

Make install and update a one-liner for all users on Linux, macOS, and Windows — no Rust toolchain required. Maintain the build-from-source path for contributors.

## Decisions

- **Distribution:** Pre-built binaries via GitHub Releases, downloaded by install scripts
- **Install location:** `~/.claude/plugins/taoki/` (Linux/macOS) or `%USERPROFILE%\.claude\plugins\taoki\` (Windows)
- **Release trigger:** Git tags matching `v*`
- **Update mechanism:** Re-run the install script (manual, user-controlled)
- **Plugin registration:** Automatic if `claude` is on PATH, otherwise print the command

## Components

### 1. GitHub Actions Release Pipeline

**File:** `.github/workflows/release.yml`

**Trigger:** Push of a tag matching `v*` (e.g., `v0.2.0`).

**Build matrix — 5 targets:**

| Target | Runner | Artifact |
|--------|--------|----------|
| `x86_64-unknown-linux-gnu` | `ubuntu-latest` | `taoki-linux-x86_64.tar.gz` |
| `aarch64-unknown-linux-gnu` | `ubuntu-latest` (cross-compile) | `taoki-linux-aarch64.tar.gz` |
| `x86_64-apple-darwin` | `macos-13` (Intel) | `taoki-macos-x86_64.tar.gz` |
| `aarch64-apple-darwin` | `macos-latest` (ARM) | `taoki-macos-aarch64.tar.gz` |
| `x86_64-pc-windows-msvc` | `windows-latest` | `taoki-windows-x86_64.zip` |

**Notes:**
- `x86_64-apple-darwin` uses `macos-13` (last Intel runner) for native compilation — avoids cross-compilation complexity.
- `aarch64-unknown-linux-gnu` cross-compiles from `ubuntu-latest` using the `cross` tool or `gcc-aarch64-linux-gnu` linker.
- **Windows ARM64 is out of scope.** Windows ARM64 runs x86_64 binaries via emulation with no user action required. We can add a native ARM64 target later if demand warrants it.

**Steps per matrix job:**

1. Checkout repo
2. Install Rust toolchain with appropriate target
3. `cargo build --release --target <target>`
4. Package binary + plugin files into tarball (or zip for Windows)
5. Upload artifact

**Release job** (runs after all builds):

- Creates a GitHub Release from the tag
- Attaches all 5 binary artifacts
- Generates and uploads `checksums.txt` (SHA256 of each artifact)
- Auto-generates release notes from commits since last tag

### 2. Release Artifact Contents

Each tarball/zip contains a self-contained plugin directory:

```
taoki/
├── .claude-plugin/
│   └── plugin.json
├── commands/
│   └── taoki-index.md
│   └── taoki-map.md
├── skills/
│   └── taoki-workflow.md
├── scripts/
│   ├── run.sh
│   └── run.cmd
└── target/
    └── release/
        └── taoki(.exe)
```

**Excluded from artifacts:** `src/`, `Cargo.toml`, `Cargo.lock`, `docs/`, `doc/`, `CLAUDE.md`, build intermediates, `scripts/install.sh`, `scripts/install.ps1` (install scripts are fetched from the repo directly, not bundled in the artifact).

### 3. Install Script — Linux/macOS (`scripts/install.sh`)

**Usage:**

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash
```

Install a specific version:

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash -s -- v0.2.0
```

**Flow:**

1. Detect platform via `uname -s` (Linux/Darwin) and `uname -m` (x86_64/arm64/aarch64)
2. Determine version — use argument if provided, otherwise query `https://api.github.com/repos/naejin/taoki/releases/latest`. On API rate limit (HTTP 403), print a helpful error suggesting `GITHUB_TOKEN` env var or passing a specific version.
3. Download the correct tarball from release assets to a temp directory. Set a `trap` to clean up the temp directory on exit (success or failure).
4. Download `checksums.txt` from the same release. Verify the tarball's SHA256 checksum matches. Abort on mismatch.
5. Extract to a temp staging directory first. If `~/.claude/plugins/taoki/` already exists, rename it to `~/.claude/plugins/taoki.bak`, move the new directory into place, then delete `taoki.bak`. This avoids both a broken install window and data loss on extraction failure.
6. Verify the binary runs via `taoki --version` (requires adding `--version` flag to the binary — see below).
7. If `claude` is on PATH, run `claude plugin add ~/.claude/plugins/taoki`. Otherwise print the command.
8. Print success message with version installed

**Binary `--version` flag:** The taoki binary needs a `--version` flag that prints the version and exits (before entering the MCP stdio loop). This is used by the install script for health checks and by users to see their installed version.

### 4. Install Script — Windows (`scripts/install.ps1`)

**Usage:**

```powershell
irm https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.ps1 | iex
```

**Flow mirrors `install.sh`:**

1. Detect architecture via `$env:PROCESSOR_ARCHITECTURE`
2. Get latest version from GitHub API (or accept version as parameter)
3. Download `.zip` and `checksums.txt` to temp directory. Verify SHA256 checksum.
4. Extract to temp staging directory, then move into `$env:USERPROFILE\.claude\plugins\taoki\`
5. Verify binary via `taoki.exe --version`
6. If `claude` is on PATH, register the plugin. Otherwise print the command.
7. Print success message

Install a specific version:

```powershell
$env:TAOKI_VERSION="v0.2.0"; irm https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.ps1 | iex
```

**PowerShell compatibility:** The script targets PowerShell 5.1+ (ships with Windows 10/11). Uses only `Invoke-RestMethod`, `Invoke-WebRequest`, `Expand-Archive`, and standard cmdlets available in 5.1.

### 5. Windows MCP Entry Point (`scripts/run.cmd`)

```cmd
@echo off
set "DIR=%~dp0.."
set "BIN=%DIR%\target\release\taoki.exe"
if not exist "%BIN%" (
    where cargo >nul 2>&1
    if %errorlevel% equ 0 (
        cargo build --release --manifest-path "%DIR%\Cargo.toml" 2>&1
    ) else (
        echo Error: taoki binary not found. Re-run the install script to download it. >&2
        exit /b 1
    )
)
"%BIN%" %*
```

### 6. Updated `scripts/run.sh`

Add fallback logic:

1. If binary exists at `target/release/taoki` → exec it
2. If `Cargo.toml` exists in the repo root → `cargo build --release`, then exec
3. Otherwise → print error: "taoki binary not found. Re-run the install script to download it."

### 7. Updated `README.md`

Replace current Install and Update sections:

**Install section:**

- Lead with the one-liner `curl | bash` for Linux/macOS
- PowerShell one-liner for Windows
- Specific version install
- Build-from-source instructions for contributors

**Update section:**

- Re-run the install script (primary path)
- `git pull` for source builds

## User Experience

### New install (no Rust required)

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash
```

Done. Plugin registered and ready to use on next Claude Code session.

### Update

```bash
curl -fsSL https://raw.githubusercontent.com/naejin/taoki/master/scripts/install.sh | bash
```

Same command. Downloads latest release, overwrites previous install.

### Developer / Contributor

```bash
git clone https://github.com/naejin/taoki.git
claude plugin add ./taoki
# Builds from source automatically on first use
```

## Additional Notes

- **Plugin entry point:** Claude Code discovers the MCP server via `scripts/run.sh` by convention when the plugin is added. This works the same for both pre-built installs and source clones. `plugin.json` contains only metadata.
- **Pre-built artifacts always include the binary** at `target/release/taoki`, so the `cargo build` fallback in `run.sh` is only reachable in the clone-from-source path (where `Cargo.toml` is present).

## What Doesn't Change

- The Rust codebase (`src/`)
- `plugin.json` manifest
- The build-from-source path (clone + `claude plugin add`)
- How the MCP server protocol works
- Cache location and format (`.cache/taoki/`)
