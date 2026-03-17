$ErrorActionPreference = "Stop"

$Repo = "naejin/taoki"
$InstallDir = Join-Path $env:USERPROFILE ".claude\plugins\taoki"

function Write-Info($msg) { Write-Host "taoki: $msg" }
function Write-Err($msg) { Write-Host "error: $msg" -ForegroundColor Red }

# Detect architecture
$Arch = $env:PROCESSOR_ARCHITECTURE
if ($Arch -ne "AMD64" -and $Arch -ne "ARM64") {
    Write-Err "Unsupported architecture: $Arch. Only x86_64 and ARM64 (via emulation) are supported."
    exit 1
}
if ($Arch -eq "ARM64") {
    Write-Info "ARM64 detected. Installing x86_64 binary (runs via emulation)."
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
        $statusCode = $_.Exception.Response.StatusCode.value__
        if ($statusCode -eq 403) {
            Write-Err "GitHub API rate limit exceeded."
            Write-Err "Set GITHUB_TOKEN or TAOKI_VERSION env var to continue."
        } else {
            Write-Err "Failed to fetch latest release (HTTP $statusCode). $_"
            Write-Err "If rate-limited, set GITHUB_TOKEN or TAOKI_VERSION env var."
        }
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
        $BackupPath = "${InstallDir}.bak"
        if (Test-Path $BackupPath) { Remove-Item -Recurse -Force $BackupPath }
        Move-Item -Path $InstallDir -Destination $BackupPath
    }
    Move-Item -Path (Join-Path $StagingDir "taoki") -Destination $InstallDir
    if (Test-Path "${InstallDir}.bak") {
        Remove-Item -Recurse -Force "${InstallDir}.bak"
    }

    # Verify binary
    $Binary = Join-Path $InstallDir "target\release\taoki.exe"
    try {
        $VersionOutput = & $Binary --version 2>&1
        if ($LASTEXITCODE -ne 0) { throw "Binary exited with code $LASTEXITCODE" }
        Write-Info "Installed $VersionOutput"
    } catch {
        Write-Err "Binary verification failed. The download may be corrupted."
        exit 1
    }

    # Remove stale .mcp.json if present (older releases shipped it, causing conflicts)
    $StaleConfig = Join-Path $InstallDir ".mcp.json"
    if (Test-Path $StaleConfig) {
        Remove-Item -Force $StaleConfig
        Write-Info "Removed stale .mcp.json (plugin.json is the MCP config source)."
    }

    # Register MCP server
    $RunCmd = Join-Path $InstallDir "scripts\run.cmd"
    $ClaudePath = Get-Command claude -ErrorAction SilentlyContinue
    if ($ClaudePath) {
        Write-Info "Registering MCP server with Claude Code..."
        try {
            & claude mcp remove taoki -s user 2>$null
        } catch { }
        try {
            & claude mcp add -s user taoki -- $RunCmd
            Write-Info "MCP server registered successfully."
        } catch {
            Write-Err "MCP registration failed. Register manually:"
            Write-Err "  claude mcp add -s user taoki -- $RunCmd"
        }
    } else {
        Write-Info "Claude Code not found on PATH. Register the MCP server manually:"
        Write-Info "  claude mcp add -s user taoki -- $RunCmd"
    }

    Write-Host ""
    Write-Info "Taoki installed successfully!"
    Write-Info "Restart Claude Code to start using taoki."
} finally {
    Remove-Item -Recurse -Force $TmpDir -ErrorAction SilentlyContinue
}
