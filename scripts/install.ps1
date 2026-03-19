# =============================================================================
# Taoki — Multi-Agent Installer (Windows)
#
# Interactive TUI installer for Claude Code, Gemini CLI, and OpenCode.
# =============================================================================

$ErrorActionPreference = "Stop"

$REPO = "naejin/taoki"
$MARKETPLACE_REPO = "naejin/monet-plugins"
$MARKETPLACE_NAME = "monet-plugins"
$PLUGIN_NAME = "taoki"
$BIN_DIR = Join-Path $env:LOCALAPPDATA "taoki"
$BIN_PATH = Join-Path $BIN_DIR "taoki.exe"

# -----------------------------------------------------------------------------
# 1. Colors & output helpers
# -----------------------------------------------------------------------------

$ESC = [char]0x1b

function Enable-AnsiSupport {
    if ($PSVersionTable.PSVersion.Major -lt 7) {
        $signature = @'
[DllImport("kernel32.dll", SetLastError = true)]
public static extern IntPtr GetStdHandle(int nStdHandle);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern bool GetConsoleMode(IntPtr hConsole, out uint lpMode);
[DllImport("kernel32.dll", SetLastError = true)]
public static extern bool SetConsoleMode(IntPtr hConsole, uint dwMode);
'@
        $Kernel32 = Add-Type -MemberDefinition $signature -Name 'Kernel32' -Namespace 'Win32' -PassThru
        $handle = $Kernel32::GetStdHandle(-11)
        $mode = [uint32]0
        $Kernel32::GetConsoleMode($handle, [ref]$mode) | Out-Null
        $Kernel32::SetConsoleMode($handle, $mode -bor 0x0004) | Out-Null  # ENABLE_VIRTUAL_TERMINAL_PROCESSING
    }
}

Enable-AnsiSupport

$BOLD = "$ESC[1m"
$DIM = "$ESC[2m"
$RED = "$ESC[0;31m"
$GREEN = "$ESC[0;32m"
$YELLOW = "$ESC[0;33m"
$CYAN = "$ESC[36m"
$BCYAN = "$ESC[96m"
$GRAY = "$ESC[90m"
$WHITE = "$ESC[97m"
$RESET = "$ESC[0m"

function Write-Info($msg) {
    Write-Host "${BOLD}taoki:${RESET} $msg"
}

function Write-Warn($msg) {
    Write-Host "${YELLOW}warning:${RESET} $msg" -NoNewline
    Write-Host ""
}

function Write-Err($msg) {
    Write-Host "${RED}error:${RESET} $msg" -NoNewline
    Write-Host ""
}

# Write text to a file as UTF-8 without BOM.
# Set-Content -Encoding UTF8 writes a BOM on PowerShell 5.1 (default on Windows 10/11),
# which breaks JSON parsers and CLI tools. This helper is safe on all PS versions.
function Write-Utf8NoBom {
    param([string]$Path, [string]$Content)
    $dir = Split-Path $Path -Parent
    if ($dir -and -not (Test-Path $dir)) {
        New-Item -ItemType Directory -Path $dir -Force | Out-Null
    }
    [System.IO.File]::WriteAllText($Path, $Content, [System.Text.UTF8Encoding]::new($false))
}

# -----------------------------------------------------------------------------
# 2. TUI functions
# -----------------------------------------------------------------------------

# TUI glyphs
$PTR = [char]0x276F   # ❯
$BFILL = [char]0x25CF # ●
$BEMPTY = [char]0x25CB # ○
$BCHECK = [char]0x2713 # ✓
$MDOT = [char]0x00B7   # ·

# State
$script:SELECTED_CLAUDE = 0
$script:SELECTED_GEMINI = 0
$script:SELECTED_OPENCODE = 0
$script:SCOPE = "global"
$script:CURSOR = 0

# Detection
$script:HAS_CLAUDE = 0
$script:HAS_GEMINI = 0
$script:HAS_OPENCODE = 0

function Detect-Agents {
    if (Get-Command claude -ErrorAction SilentlyContinue) { $script:HAS_CLAUDE = 1 }
    if (Get-Command gemini -ErrorAction SilentlyContinue) { $script:HAS_GEMINI = 1 }
    if (Get-Command opencode -ErrorAction SilentlyContinue) { $script:HAS_OPENCODE = 1 }
    # Pre-select detected agents; default to Claude if none detected
    $script:SELECTED_CLAUDE = $script:HAS_CLAUDE
    $script:SELECTED_GEMINI = $script:HAS_GEMINI
    $script:SELECTED_OPENCODE = $script:HAS_OPENCODE
    if ($script:SELECTED_CLAUDE -eq 0 -and $script:SELECTED_GEMINI -eq 0 -and $script:SELECTED_OPENCODE -eq 0) {
        $script:SELECTED_CLAUDE = 1
    }
}

function Hide-Cursor {
    Write-Host "$ESC[?25l" -NoNewline
}

function Show-Cursor {
    Write-Host "$ESC[?25h" -NoNewline
}

function Draw-MultiSelect {
    param([bool]$FirstDraw = $false)

    $labels = @("Claude Code", "Gemini CLI", "OpenCode")
    $descs = @("marketplace plugin", "binary + mcp server", "binary + mcp server")
    $selected = @($script:SELECTED_CLAUDE, $script:SELECTED_GEMINI, $script:SELECTED_OPENCODE)
    $detected = @($script:HAS_CLAUDE, $script:HAS_GEMINI, $script:HAS_OPENCODE)

    # Move cursor up to redraw (12 lines)
    if (-not $FirstDraw) {
        Write-Host "$ESC[12A" -NoNewline
    }

    # Title
    Write-Host ""
    Write-Host "  ${BOLD}taoki${RESET}  ${DIM}structural code intelligence${RESET}"
    Write-Host "  ${DIM}radar ${MDOT} xray ${MDOT} ripple${RESET}"
    Write-Host ""

    # Heading
    Write-Host "  Select coding agents to install:"
    Write-Host ""

    # Agent rows
    for ($i = 0; $i -lt 3; $i++) {
        $ptr = "  "
        $icon = "${GRAY}${BEMPTY}${RESET}"

        if ($script:CURSOR -eq $i) {
            $ptr = "${BCYAN}${PTR}${RESET} "
        }
        if ($selected[$i] -eq 1) {
            $icon = "${GREEN}${BFILL}${RESET}"
        }

        $label = $labels[$i]
        $padLen = 20 - $label.Length
        $padding = " " * $padLen

        $detect = ""
        if ($detected[$i] -eq 1) {
            $detect = "  ${GREEN}${BCHECK}${RESET}"
        }

        Write-Host "  ${ptr}${icon} ${label}${padding}${DIM}$($descs[$i])${RESET}${detect}"
    }

    # Footer
    Write-Host ""
    Write-Host "  ${DIM}$([char]0x2191)$([char]0x2193) navigate   space toggle   enter confirm   esc quit${RESET}"
    Write-Host ""
}

function Draw-Scope {
    param([int]$ScopeCursor, [bool]$FirstDraw = $false)

    if (-not $FirstDraw) {
        Write-Host "$ESC[7A" -NoNewline
    }

    $globalPtr = "  "
    $projectPtr = "  "
    if ($ScopeCursor -eq 0) { $globalPtr = "${BCYAN}${PTR}${RESET} " }
    if ($ScopeCursor -eq 1) { $projectPtr = "${BCYAN}${PTR}${RESET} " }

    Write-Host ""
    Write-Host "  Install scope:"
    Write-Host ""
    Write-Host "  ${globalPtr}Global               ${DIM}all projects${RESET}"
    Write-Host "  ${projectPtr}Project              ${DIM}current directory only${RESET}"
    Write-Host ""
    Write-Host "  ${DIM}$([char]0x2191)$([char]0x2193) navigate   enter confirm   esc back${RESET}"
}

function Read-Key {
    $key = [System.Console]::ReadKey($true)

    if ($key.Key -eq [ConsoleKey]::UpArrow) { return "UP" }
    if ($key.Key -eq [ConsoleKey]::DownArrow) { return "DOWN" }
    if ($key.Key -eq [ConsoleKey]::Spacebar) { return "SPACE" }
    if ($key.Key -eq [ConsoleKey]::Enter) { return "ENTER" }
    if ($key.Key -eq [ConsoleKey]::Escape) { return "ESC" }
    # Ctrl-C
    if ($key.Key -eq [ConsoleKey]::C -and $key.Modifiers -band [ConsoleModifiers]::Control) { return "ESC" }
    return "OTHER"
}

function Select-Agents {
    Detect-Agents
    Hide-Cursor

    Draw-MultiSelect -FirstDraw $true

    while ($true) {
        $key = Read-Key

        switch ($key) {
            "UP" {
                $script:CURSOR = ($script:CURSOR + 2) % 3
                Draw-MultiSelect
            }
            "DOWN" {
                $script:CURSOR = ($script:CURSOR + 1) % 3
                Draw-MultiSelect
            }
            "SPACE" {
                switch ($script:CURSOR) {
                    0 { $script:SELECTED_CLAUDE = 1 - $script:SELECTED_CLAUDE }
                    1 { $script:SELECTED_GEMINI = 1 - $script:SELECTED_GEMINI }
                    2 { $script:SELECTED_OPENCODE = 1 - $script:SELECTED_OPENCODE }
                }
                Draw-MultiSelect
            }
            "ENTER" {
                # At least one must be selected
                if ($script:SELECTED_CLAUDE -eq 1 -or $script:SELECTED_GEMINI -eq 1 -or $script:SELECTED_OPENCODE -eq 1) {
                    Show-Cursor
                    Write-Host ""
                    return
                }
                # Nothing selected — ignore
            }
            "ESC" {
                Show-Cursor
                Write-Host ""
                Write-Host "Cancelled."
                exit 0
            }
        }
    }
}

function Select-Scope {
    $scopeCursor = 0

    Hide-Cursor

    Draw-Scope -ScopeCursor $scopeCursor -FirstDraw $true

    while ($true) {
        $key = Read-Key

        switch ($key) {
            { $_ -eq "UP" -or $_ -eq "DOWN" } {
                $scopeCursor = 1 - $scopeCursor
                Draw-Scope -ScopeCursor $scopeCursor
            }
            "ENTER" {
                if ($scopeCursor -eq 0) {
                    $script:SCOPE = "global"
                } else {
                    $script:SCOPE = "project"
                }
                Show-Cursor
                Write-Host ""
                return
            }
            "ESC" {
                # Go back — re-show agent selection
                Show-Cursor
                Write-Host ""
                Select-Agents
                # After agent selection, check if scope is still needed
                if ($script:SELECTED_GEMINI -eq 1 -or $script:SELECTED_OPENCODE -eq 1) {
                    Select-Scope
                }
                return
            }
        }
    }
}

# -----------------------------------------------------------------------------
# 3. JSON manipulation helpers
# -----------------------------------------------------------------------------

function ConvertTo-Hashtable($obj) {
    if ($obj -is [System.Management.Automation.PSCustomObject]) {
        $ht = [ordered]@{}
        foreach ($prop in $obj.PSObject.Properties) {
            $ht[$prop.Name] = ConvertTo-Hashtable $prop.Value
        }
        return $ht
    }
    elseif ($obj -is [System.Collections.IEnumerable] -and $obj -isnot [string]) {
        return @($obj | ForEach-Object { ConvertTo-Hashtable $_ })
    }
    return $obj
}

# Strip JSONC comments and trailing commas using a string-aware state machine.
# Safe for URLs containing // — only strips comments outside of string literals.
function Strip-Jsonc {
    param([string]$Text)

    $result = [System.Text.StringBuilder]::new()
    $i = 0
    $inString = $false

    while ($i -lt $Text.Length) {
        $c = $Text[$i]

        if ($inString) {
            $result.Append($c) | Out-Null
            if ($c -eq '\' -and ($i + 1) -lt $Text.Length) {
                $i++
                $result.Append($Text[$i]) | Out-Null
            }
            elseif ($c -eq '"') {
                $inString = $false
            }
            $i++
        }
        else {
            if ($c -eq '"') {
                $inString = $true
                $result.Append($c) | Out-Null
                $i++
            }
            elseif ($c -eq '/' -and ($i + 1) -lt $Text.Length) {
                if ($Text[$i + 1] -eq '/') {
                    # Line comment — skip to end of line
                    while ($i -lt $Text.Length -and $Text[$i] -ne "`n") {
                        $i++
                    }
                }
                elseif ($Text[$i + 1] -eq '*') {
                    # Block comment — skip to */
                    $i += 2
                    while (($i + 1) -lt $Text.Length -and -not ($Text[$i] -eq '*' -and $Text[$i + 1] -eq '/')) {
                        $i++
                    }
                    $i += 2
                }
                else {
                    $result.Append($c) | Out-Null
                    $i++
                }
            }
            else {
                $result.Append($c) | Out-Null
                $i++
            }
        }
    }

    # Strip trailing commas before ] or }
    $cleaned = $result.ToString()
    $cleaned = [regex]::Replace($cleaned, ',\s*([\]\}])', '$1')
    return $cleaned
}

# Upsert a key into a JSON object at a given path.
# Usage: Upsert-JsonMcp <file> <parent_key> <child_key> <value_hashtable>
function Upsert-JsonMcp {
    param(
        [string]$File,
        [string]$ParentKey,
        [string]$ChildKey,
        [object]$Value
    )

    # If file doesn't exist, create it
    if (-not (Test-Path $File)) {
        Write-Utf8NoBom -Path $File -Content '{}'
    }

    $rawContent = Get-Content -Path $File -Raw -ErrorAction SilentlyContinue
    if ([string]::IsNullOrWhiteSpace($rawContent)) {
        $rawContent = '{}'
    }

    try {
        $cleanJson = Strip-Jsonc -Text $rawContent
        $parsed = $cleanJson | ConvertFrom-Json
        $data = ConvertTo-Hashtable $parsed
    }
    catch {
        # Parse error — back up and give manual instructions
        $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
        $backup = "${File}.bak.${timestamp}"
        Copy-Item -Path $File -Destination $backup -Force
        Write-Warn "Failed to parse $File (backed up to $backup)"
        Write-Warn "Add this manually to ${File}:"
        Write-Host ""
        Write-Host "  `"$ParentKey`": {"
        Write-Host "    `"$ChildKey`": $(ConvertTo-Json $Value -Compress)"
        Write-Host "  }"
        Write-Host ""
        return $false
    }

    if (-not $data) {
        $data = [ordered]@{}
    }

    if (-not $data.Contains($ParentKey)) {
        $data[$ParentKey] = [ordered]@{}
    }
    $data[$ParentKey][$ChildKey] = $Value

    $json = ConvertTo-Json $data -Depth 10
    # Atomic write: temp file + move prevents data loss on interrupt
    $tmpFile = "${File}.tmp.$PID"
    Write-Utf8NoBom -Path $tmpFile -Content $json
    Move-Item -Path $tmpFile -Destination $File -Force
    return $true
}

# Upsert a value into a JSON array at a given key.
function Upsert-JsonArray {
    param(
        [string]$File,
        [string]$Key,
        [string]$Value
    )

    if (-not (Test-Path $File)) {
        Write-Utf8NoBom -Path $File -Content '{}'
    }

    $rawContent = Get-Content -Path $File -Raw -ErrorAction SilentlyContinue
    if ([string]::IsNullOrWhiteSpace($rawContent)) {
        $rawContent = '{}'
    }

    try {
        $cleanJson = Strip-Jsonc -Text $rawContent
        $parsed = $cleanJson | ConvertFrom-Json
        $data = ConvertTo-Hashtable $parsed
    }
    catch {
        $timestamp = [DateTimeOffset]::UtcNow.ToUnixTimeSeconds()
        $backup = "${File}.bak.${timestamp}"
        Copy-Item -Path $File -Destination $backup -Force
        Write-Warn "Failed to parse $File (backed up to $backup)"
        Write-Warn "Add this manually to ${File}:"
        Write-Host ""
        Write-Host "  `"$Key`": [`"$Value`"]"
        Write-Host ""
        return $false
    }

    if (-not $data) {
        $data = [ordered]@{}
    }

    if (-not $data.Contains($Key)) {
        $data[$Key] = @()
    }
    if ($data[$Key] -notcontains $Value) {
        $data[$Key] = @($data[$Key]) + @($Value)
    }

    $json = ConvertTo-Json $data -Depth 10
    # Atomic write: temp file + move prevents data loss on interrupt
    $tmpFile = "${File}.tmp.$PID"
    Write-Utf8NoBom -Path $tmpFile -Content $json
    Move-Item -Path $tmpFile -Destination $File -Force
    return $true
}

# -----------------------------------------------------------------------------
# 4. Instruction file copy helper
# -----------------------------------------------------------------------------

# Copy a template file to a destination. Looks for the template adjacent to this
# script first; falls back to downloading from GitHub.
function Copy-InstructionFile {
    param(
        [string]$TemplateName,
        [string]$DestPath
    )

    $scriptDir = $PSScriptRoot
    if (-not $scriptDir) {
        $scriptDir = Split-Path -Parent $MyInvocation.MyCommand.Path
    }
    $localPath = Join-Path $scriptDir $TemplateName

    # Try local copy first (source checkout)
    if (Test-Path $localPath) {
        $destDir = Split-Path $DestPath -Parent
        if ($destDir -and -not (Test-Path $destDir)) {
            New-Item -ItemType Directory -Path $destDir -Force | Out-Null
        }
        Copy-Item -Path $localPath -Destination $DestPath -Force
        return $true
    }

    # Fallback: download from GitHub
    $version = $null
    try { $version = Get-LatestVersion } catch { }
    $ref = if ($version) { $version } else { "master" }
    $url = "https://raw.githubusercontent.com/${REPO}/${ref}/scripts/${TemplateName}"

    $destDir = Split-Path $DestPath -Parent
    if ($destDir -and -not (Test-Path $destDir)) {
        New-Item -ItemType Directory -Path $destDir -Force | Out-Null
    }

    try {
        Invoke-WebRequest -Uri $url -OutFile $DestPath -UseBasicParsing -ErrorAction Stop
        return $true
    }
    catch { }

    # Last resort: try master
    if ($ref -ne "master") {
        $url = "https://raw.githubusercontent.com/${REPO}/master/scripts/${TemplateName}"
        try {
            Invoke-WebRequest -Uri $url -OutFile $DestPath -UseBasicParsing -ErrorAction Stop
            return $true
        }
        catch { }
    }

    Write-Warn "Could not download $TemplateName"
    return $false
}

# -----------------------------------------------------------------------------
# 5. Agent install functions
# -----------------------------------------------------------------------------

# --- Claude Code ---

function Install-ClaudeCode {
    Write-Info "Installing for Claude Code..."
    Write-Host ""

    # Check claude command exists
    $claudeCmd = Get-Command claude -ErrorAction SilentlyContinue
    if (-not $claudeCmd) {
        Write-Err "Claude Code not found on PATH."
        Write-Err "Install it first: https://docs.anthropic.com/en/docs/claude-code"
        Write-Err ""
        Write-Err "Then run this script again, or install manually:"
        Write-Err "  claude plugin marketplace add $MARKETPLACE_REPO"
        Write-Err "  claude plugin install ${PLUGIN_NAME}@${MARKETPLACE_NAME}"
        return $false
    }

    # Clean up legacy installations from older versions
    try { & claude mcp remove taoki -s user 2>$null } catch { }
    $legacyLocal = Join-Path $env:USERPROFILE ".claude\plugins\taoki-local"
    if (Test-Path $legacyLocal) {
        try { & claude plugin uninstall "${PLUGIN_NAME}@taoki-local" 2>$null } catch { }
        try { & claude plugin marketplace remove "taoki-local" 2>$null } catch { }
        Remove-Item -Recurse -Force $legacyLocal -ErrorAction SilentlyContinue
        Write-Info "Cleaned up legacy local marketplace."
    }
    $legacyDir = Join-Path $env:USERPROFILE ".claude\plugins\taoki"
    if (Test-Path $legacyDir) {
        Remove-Item -Recurse -Force $legacyDir -ErrorAction SilentlyContinue
        Write-Info "Cleaned up legacy install directory."
    }

    # Add marketplace if not already registered
    $marketplaceList = ""
    try { $marketplaceList = & claude plugin marketplace list 2>$null } catch { }
    if ($marketplaceList -notmatch [regex]::Escape($MARKETPLACE_NAME)) {
        Write-Info "Adding marketplace..."
        try {
            & claude plugin marketplace add $MARKETPLACE_REPO 2>&1
            if ($LASTEXITCODE -ne 0) { throw "exit code $LASTEXITCODE" }
        }
        catch {
            Write-Err "Failed to add marketplace. Try manually:"
            Write-Err "  claude plugin marketplace add $MARKETPLACE_REPO"
            return $false
        }
    }

    # Install or update plugin
    $pluginList = ""
    try { $pluginList = & claude plugin list 2>$null } catch { }
    if ($pluginList -match [regex]::Escape("${PLUGIN_NAME}@${MARKETPLACE_NAME}")) {
        Write-Info "Updating plugin..."
        & claude plugin marketplace update $MARKETPLACE_NAME 2>&1
        & claude plugin update "${PLUGIN_NAME}@${MARKETPLACE_NAME}" 2>&1
    }
    else {
        Write-Info "Installing plugin..."
        try {
            & claude plugin install "${PLUGIN_NAME}@${MARKETPLACE_NAME}" 2>&1
            if ($LASTEXITCODE -ne 0) { throw "exit code $LASTEXITCODE" }
        }
        catch {
            Write-Err "Failed to install plugin. Try manually:"
            Write-Err "  claude plugin install ${PLUGIN_NAME}@${MARKETPLACE_NAME}"
            return $false
        }
    }

    Write-Host ""
    Write-Info "${GREEN}Claude Code: installed.${RESET}"
    return $true
}

# --- Binary download (shared by Gemini CLI and OpenCode) ---

function Get-LatestVersion {
    $response = Invoke-WebRequest -Uri "https://api.github.com/repos/${REPO}/releases/latest" -UseBasicParsing -ErrorAction Stop
    $json = $response.Content | ConvertFrom-Json
    $tag = $json.tag_name
    if ([string]::IsNullOrEmpty($tag)) {
        throw "No tag found"
    }
    return $tag
}

$script:BINARY_ENSURED = $false

function Ensure-Binary {
    # Only download once per run (both Gemini and OpenCode share the binary)
    if ($script:BINARY_ENSURED) {
        return $true
    }

    # Check if binary already exists and is up to date
    if (Test-Path $BIN_PATH) {
        $currentVersion = $null
        try {
            $currentVersion = & $BIN_PATH --version 2>$null | Select-Object -First 1
        } catch { }

        $latestVersion = $null
        try { $latestVersion = Get-LatestVersion } catch { }

        if ($latestVersion -and $currentVersion) {
            # Normalize: strip leading 'v' and 'taoki ' prefix for comparison
            $currentV = ($currentVersion -replace '^taoki ', '') -replace '^v', ''
            $latestV = $latestVersion -replace '^v', ''
            if ($currentV -eq $latestV) {
                Write-Info "Binary already up to date ($latestVersion)."
                $script:BINARY_ENSURED = $true
                return $true
            }
            Write-Info "Updating binary: $currentV -> $latestV"
        }
    }

    $version = $null
    if ($env:TAOKI_VERSION) {
        $version = $env:TAOKI_VERSION
        Write-Info "Using pinned version: $version"
    } else {
        try { $version = Get-LatestVersion } catch { }
        if (-not $version) {
            Write-Err "Could not determine latest taoki version from GitHub."
            Write-Err "If rate-limited, set `$env:TAOKI_VERSION='v1.3.0' to specify a version manually."
            return $false
        }
    }

    # Windows only: x86_64
    $artifact = "taoki-windows-x86_64.zip"
    $url = "https://github.com/${REPO}/releases/download/${version}/${artifact}"

    Write-Info "Downloading taoki ${version} (windows-x86_64)..."

    $tmpDir = Join-Path ([System.IO.Path]::GetTempPath()) "taoki-install-$([System.Guid]::NewGuid().ToString('N'))"
    New-Item -ItemType Directory -Path $tmpDir -Force | Out-Null

    try {
        $zipPath = Join-Path $tmpDir $artifact
        Invoke-WebRequest -Uri $url -OutFile $zipPath -UseBasicParsing -ErrorAction Stop

        Expand-Archive -Path $zipPath -DestinationPath $tmpDir -Force

        if (-not (Test-Path $BIN_DIR)) {
            New-Item -ItemType Directory -Path $BIN_DIR -Force | Out-Null
        }
        Copy-Item -Path (Join-Path $tmpDir "taoki\target\release\taoki.exe") -Destination $BIN_PATH -Force
    }
    catch {
        Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue
        Write-Err "Failed to download taoki binary from ${url}"
        return $false
    }

    Remove-Item -Recurse -Force $tmpDir -ErrorAction SilentlyContinue

    Write-Info "Installed taoki binary to $BIN_PATH"

    # Check PATH
    $pathDirs = $env:Path -split ';'
    $binDirNorm = $BIN_DIR.TrimEnd('\')
    $onPath = $false
    foreach ($d in $pathDirs) {
        if ($d.TrimEnd('\') -eq $binDirNorm) {
            $onPath = $true
            break
        }
    }
    if (-not $onPath) {
        Write-Warn "$BIN_DIR is not on your PATH."
        Write-Warn "Add it via System Properties > Environment Variables, or run:"
        Write-Host ""
        Write-Host "  `$env:Path += `";$BIN_DIR`""
        Write-Host ""
    }

    $script:BINARY_ENSURED = $true
    return $true
}

# --- Gemini CLI ---

function Install-GeminiCli {
    param([string]$Scope)

    Write-Info "Installing for Gemini CLI (${Scope})..."
    Write-Host ""

    # Ensure binary is available
    if (-not (Ensure-Binary)) {
        Write-Err "Binary download failed. Install Gemini CLI support manually:"
        Write-Err "  Download from https://github.com/${REPO}/releases"
        Write-Err "  Place binary at $BIN_PATH"
        return $false
    }

    if ($Scope -eq "global") {
        $geminiDir = Join-Path $env:USERPROFILE ".gemini"
    }
    else {
        $geminiDir = ".gemini"
        Write-Info "Target directory: $(Get-Location)"
    }
    $settingsFile = Join-Path $geminiDir "settings.json"
    $instructionDest = Join-Path $geminiDir "taoki.md"
    $geminiMd = Join-Path $geminiDir "GEMINI.md"

    # MCP config — use absolute path so taoki works even if $BIN_DIR is not on PATH
    $mcpValue = [ordered]@{ command = $BIN_PATH; args = @() }
    if (Upsert-JsonMcp -File $settingsFile -ParentKey "mcpServers" -ChildKey "taoki" -Value $mcpValue) {
        Write-Info "MCP config written to $settingsFile"
    }

    # Instruction file
    if (Copy-InstructionFile -TemplateName "taoki-gemini.md" -DestPath $instructionDest) {
        Write-Info "Instruction file written to $instructionDest"
    }
    else {
        Write-Warn "Could not copy instruction file to $instructionDest"
    }

    # GEMINI.md — prepend @./taoki.md if not already present
    $importLine = '@./taoki.md'
    if (Test-Path $geminiMd) {
        $content = Get-Content -Path $geminiMd -Raw
        if ($content -notmatch [regex]::Escape($importLine)) {
            $newContent = "${importLine}`r`n`r`n${content}"
            Write-Utf8NoBom -Path $geminiMd -Content $newContent
            Write-Info "Added $importLine to $geminiMd"
        }
        else {
            Write-Info "$importLine already in $geminiMd"
        }
    }
    else {
        $geminiMdDir = Split-Path $geminiMd -Parent
        if ($geminiMdDir -and -not (Test-Path $geminiMdDir)) {
            New-Item -ItemType Directory -Path $geminiMdDir -Force | Out-Null
        }
        Write-Utf8NoBom -Path $geminiMd -Content $importLine
        Write-Info "Created $geminiMd with $importLine"
    }

    Write-Host ""
    Write-Info "${GREEN}Gemini CLI: installed.${RESET}"
    return $true
}

# --- OpenCode ---

function Install-OpenCode {
    param([string]$Scope)

    Write-Info "Installing for OpenCode (${Scope})..."
    Write-Host ""

    # Ensure binary is available
    if (-not (Ensure-Binary)) {
        Write-Err "Binary download failed. Install OpenCode support manually:"
        Write-Err "  Download from https://github.com/${REPO}/releases"
        Write-Err "  Place binary at $BIN_PATH"
        return $false
    }

    if ($Scope -eq "global") {
        $configFile = Join-Path $env:USERPROFILE ".config\opencode\opencode.json"
        $instructionDest = Join-Path $env:USERPROFILE ".config\opencode\taoki.md"
    }
    else {
        $configFile = "opencode.json"
        $instructionDest = "taoki.md"
        Write-Info "Target directory: $(Get-Location)"
    }

    # MCP config (OpenCode format differs from Gemini) — use absolute path
    $mcpValue = [ordered]@{ type = "local"; command = @($BIN_PATH) }
    if (Upsert-JsonMcp -File $configFile -ParentKey "mcp" -ChildKey "taoki" -Value $mcpValue) {
        Write-Info "MCP config written to $configFile"
    }

    # Instruction file
    if (Copy-InstructionFile -TemplateName "taoki-opencode.md" -DestPath $instructionDest) {
        Write-Info "Instruction file written to $instructionDest"
    }
    else {
        Write-Warn "Could not copy instruction file to $instructionDest"
    }

    # Add instruction file path to instructions array
    if (Upsert-JsonArray -File $configFile -Key "instructions" -Value $instructionDest) {
        Write-Info "Added $instructionDest to instructions array in $configFile"
    }

    Write-Host ""
    Write-Info "${GREEN}OpenCode: installed.${RESET}"
    return $true
}

# -----------------------------------------------------------------------------
# 6. Main flow
# -----------------------------------------------------------------------------

function Main {
    # Non-interactive detection
    if (-not [Environment]::UserInteractive -or [Console]::IsInputRedirected) {
        Write-Host "taoki: Interactive terminal required for the installer TUI."
        Write-Host ""
        Write-Host "Run this instead:"
        Write-Host "  Invoke-WebRequest -Uri 'https://github.com/naejin/taoki/releases/latest/download/install.ps1' -OutFile `$env:TEMP\taoki-install.ps1; & `$env:TEMP\taoki-install.ps1"
        exit 1
    }

    # Agent selection
    Select-Agents

    # Scope prompt if Gemini or OpenCode selected
    if ($script:SELECTED_GEMINI -eq 1 -or $script:SELECTED_OPENCODE -eq 1) {
        Select-Scope
    }

    Write-Host ""

    # Install each selected agent
    $hadError = $false

    if ($script:SELECTED_CLAUDE -eq 1) {
        if (-not (Install-ClaudeCode)) {
            $hadError = $true
        }
        Write-Host ""
    }

    if ($script:SELECTED_GEMINI -eq 1) {
        if (-not (Install-GeminiCli -Scope $script:SCOPE)) {
            $hadError = $true
        }
        Write-Host ""
    }

    if ($script:SELECTED_OPENCODE -eq 1) {
        if (-not (Install-OpenCode -Scope $script:SCOPE)) {
            $hadError = $true
        }
        Write-Host ""
    }

    # Summary
    Write-Host ""
    if (-not $hadError) {
        Write-Info "${GREEN}All done!${RESET}"
    }
    else {
        Write-Info "${YELLOW}Completed with errors -- see messages above.${RESET}"
    }

    if ($script:SELECTED_CLAUDE -eq 1) {
        Write-Info "  Claude Code: restart Claude Code to start using taoki."
    }
    if ($script:SELECTED_GEMINI -eq 1) {
        Write-Info "  Gemini CLI: restart Gemini to start using taoki."
    }
    if ($script:SELECTED_OPENCODE -eq 1) {
        Write-Info "  OpenCode: restart OpenCode to start using taoki."
    }
}

Main
