$ErrorActionPreference = "Stop"

$MarketplaceRepo = "naejin/monet-plugins"
$MarketplaceName = "monet-plugins"
$PluginName = "taoki"

function Write-Info($msg) { Write-Host "taoki: $msg" }
function Write-Err($msg) { Write-Host "error: $msg" -ForegroundColor Red }

# Require Claude Code
$ClaudePath = Get-Command claude -ErrorAction SilentlyContinue
if (-not $ClaudePath) {
    Write-Err "Claude Code not found on PATH."
    Write-Err "Install it first: https://docs.anthropic.com/en/docs/claude-code"
    Write-Err ""
    Write-Err "Then run this script again, or install manually:"
    Write-Err "  claude plugin marketplace add $MarketplaceRepo"
    Write-Err "  claude plugin install $PluginName@$MarketplaceName"
    exit 1
}

# Clean up legacy installations from older versions
try { & claude mcp remove taoki -s user 2>$null } catch { }
$LegacyLocal = Join-Path $env:USERPROFILE ".claude\plugins\taoki-local"
if (Test-Path $LegacyLocal) {
    try { & claude plugin uninstall "$PluginName@taoki-local" 2>$null } catch { }
    try { & claude plugin marketplace remove "taoki-local" 2>$null } catch { }
    Remove-Item -Recurse -Force $LegacyLocal -ErrorAction SilentlyContinue
    Write-Info "Cleaned up legacy local marketplace."
}
$LegacyDir = Join-Path $env:USERPROFILE ".claude\plugins\taoki"
if (Test-Path $LegacyDir) {
    Remove-Item -Recurse -Force $LegacyDir -ErrorAction SilentlyContinue
    Write-Info "Cleaned up legacy install directory."
}

# Add marketplace (remove first for idempotent reinstall)
Write-Info "Adding marketplace..."
try { & claude plugin marketplace remove $MarketplaceName 2>$null } catch { }
& claude plugin marketplace add $MarketplaceRepo
if ($LASTEXITCODE -ne 0) {
    Write-Err "Failed to add marketplace. Try manually:"
    Write-Err "  claude plugin marketplace add $MarketplaceRepo"
    exit 1
}

# Install or update plugin
$pluginList = & claude plugin list 2>$null
if ($pluginList -match "$PluginName@$MarketplaceName") {
    Write-Info "Updating plugin..."
    & claude plugin marketplace update $MarketplaceName
    & claude plugin update "$PluginName@$MarketplaceName"
} else {
    Write-Info "Installing plugin..."
    & claude plugin install "$PluginName@$MarketplaceName"
    if ($LASTEXITCODE -ne 0) {
        Write-Err "Failed to install plugin. Try manually:"
        Write-Err "  claude plugin install $PluginName@$MarketplaceName"
        exit 1
    }
}

Write-Host ""
Write-Info "Taoki installed successfully!"
Write-Info "Restart Claude Code to start using taoki."
