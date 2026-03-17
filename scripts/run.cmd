@echo off
setlocal enabledelayedexpansion
set "DIR=%~dp0.."
set "BIN=%DIR%\target\release\taoki.exe"

:: 1. Binary exists — run it
if exist "%BIN%" (
    "%BIN%" %*
    exit /b %errorlevel%
)

:: 2. Source checkout with Rust — build from source
where cargo >nul 2>&1
if %errorlevel% equ 0 (
    if exist "%DIR%\Cargo.toml" (
        cargo build --release --manifest-path "%DIR%\Cargo.toml" 1>&2
        "%BIN%" %*
        exit /b %errorlevel%
    )
)

:: 3. Download pre-built binary from GitHub Releases
set "REPO=naejin/taoki"
set "ARTIFACT=taoki-windows-x86_64.zip"
set "VERSION="

:: Get version from plugin.json if available
if exist "%DIR%\.claude-plugin\plugin.json" (
    where python3 >nul 2>&1
    if !errorlevel! equ 0 (
        for /f "delims=" %%v in ('python3 -c "import json; print('v'+json.load(open(r'%DIR%\.claude-plugin\plugin.json'))['version'])" 2^>nul') do set "VERSION=%%v"
    )
)

:: Fall back to latest release
if "%VERSION%"=="" (
    where curl >nul 2>&1
    if !errorlevel! equ 0 (
        for /f "tokens=2 delims=:" %%a in ('curl -fsSL "https://api.github.com/repos/%REPO%/releases/latest" 2^>nul ^| findstr "tag_name"') do (
            set "RAW=%%a"
            set "RAW=!RAW: =!"
            set "RAW=!RAW:"=!"
            set "RAW=!RAW:,=!"
            set "VERSION=!RAW!"
        )
    )
)

if "%VERSION%"=="" (
    echo Error: could not determine taoki version to download. >&2
    exit /b 1
)

echo Downloading taoki %VERSION% (windows-x86_64)... >&2
set "TMPDIR=%TEMP%\taoki-dl-%RANDOM%"
mkdir "%TMPDIR%" 2>nul

set "URL=https://github.com/%REPO%/releases/download/%VERSION%/%ARTIFACT%"
curl -fsSL -o "%TMPDIR%\%ARTIFACT%" "%URL%" 2>nul
if %errorlevel% neq 0 (
    echo Error: failed to download taoki binary from %URL% >&2
    rd /s /q "%TMPDIR%" 2>nul
    exit /b 1
)

:: Extract and install
cd /d "%TMPDIR%"
tar xf "%ARTIFACT%" 2>nul || (
    powershell -Command "Expand-Archive -Path '%ARTIFACT%' -DestinationPath '.' -Force" 2>nul
)
if not exist "%DIR%\target\release" mkdir "%DIR%\target\release"
copy /y "taoki\target\release\taoki.exe" "%BIN%" >nul
cd /d "%DIR%"
rd /s /q "%TMPDIR%" 2>nul
echo Downloaded taoki %VERSION% successfully. >&2

"%BIN%" %*
exit /b %errorlevel%
