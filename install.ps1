# SpotGlow One-Command Installer for Windows 10/11
# Usage:
#   irm https://raw.githubusercontent.com/<user>/spotglow/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "  ✨ SpotGlow — Ambient Spotify Window Border Glow ✨" -ForegroundColor Cyan
Write-Host "  -------------------------------------------------" -ForegroundColor DarkGray

# 1. Architecture Check
if ([IntPtr]::Size -ne 8) {
    Write-Error "SpotGlow requires 64-bit Windows."
    exit 1
}

$repo = "SpotGlow"
$localInstaller = Join-Path $PSScriptRoot "src-tauri\target\release\bundle\nsis\SpotGlow_0.1.0_x64-setup.exe"
$tempInstaller = Join-Path $env:TEMP "SpotGlow_Setup.exe"

# 2. Locate or Download Installer
if ($PSScriptRoot -and (Test-Path $localInstaller)) {
    Write-Host "  [+] Using local build installer..." -ForegroundColor Green
    Copy-Item $localInstaller $tempInstaller -Force
} else {
    Write-Host "  [+] Fetching latest SpotGlow release..." -ForegroundColor Yellow
    # Replace with your published release URL or GitHub repo tag
    $downloadUrl = "https://github.com/spotglow/spotglow/releases/latest/download/SpotGlow_x64-setup.exe"
    
    try {
        [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12
        Invoke-WebRequest -Uri $downloadUrl -OutFile $tempInstaller -UseBasicParsing
    } catch {
        Write-Warning "Could not download from remote repository: $_"
        Write-Host "To install locally, run 'npm run tauri build' and execute the installer in src-tauri\target\release\bundle\nsis\" -ForegroundColor Yellow
        exit 1
    }
}

# 3. Silent Installation
Write-Host "  [+] Installing SpotGlow silently..." -ForegroundColor Green
$process = Start-Process -FilePath $tempInstaller -ArgumentList "/S" -Wait -PassThru

if ($process.ExitCode -eq 0) {
    Write-Host "  [✓] SpotGlow installed successfully!" -ForegroundColor Green
} else {
    Write-Warning "Installation finished with exit code $($process.ExitCode)."
}

# 4. Clean up installer temp file
Remove-Item $tempInstaller -Force -ErrorAction SilentlyContinue

# 5. Launch SpotGlow
$installedExe = Join-Path $env:LOCALAPPDATA "Programs\SpotGlow\SpotGlow.exe"
if (Test-Path $installedExe) {
    Write-Host "  [🚀] Launching SpotGlow..." -ForegroundColor Cyan
    Start-Process $installedExe
} else {
    # Check alternate install locations
    $altExe = "C:\Program Files\SpotGlow\SpotGlow.exe"
    if (Test-Path $altExe) {
        Start-Process $altExe
    }
}

Write-Host "  Enjoy the glow! Control via System Tray, F8 (styles), or F9 (cover mode)." -ForegroundColor Magenta
Write-Host ""
