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
