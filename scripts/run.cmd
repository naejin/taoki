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
