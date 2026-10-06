# SpotGlow One-Command Installer for Windows 10/11
# Usage:
#   irm https://raw.githubusercontent.com/Saimun-jd/spotglow/main/install.ps1 | iex

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "  === SpotGlow - Ambient Spotify Window Border Glow ===" -ForegroundColor Cyan
Write-Host "  -----------------------------------------------------" -ForegroundColor DarkGray

# 1. Architecture Check
if ([IntPtr]::Size -ne 8) {
    Write-Error "SpotGlow requires 64-bit Windows."
    exit 1
}

$repo = "Saimun-jd/spotglow"
$tempInstaller = Join-Path $env:TEMP "SpotGlow_Setup.exe"

# Check for local build installer first (works if cloned or running in repo directory)
$localInstaller = $null
$possibleLocalPaths = @()

if ($PSScriptRoot) {
    $possibleLocalPaths += (Join-Path $PSScriptRoot "src-tauri\target\release\bundle\nsis\SpotGlow_0.1.0_x64-setup.exe")
}

$currentDir = (Get-Location).Path
if ($currentDir) {
    $possibleLocalPaths += (Join-Path $currentDir "src-tauri\target\release\bundle\nsis\SpotGlow_0.1.0_x64-setup.exe")
    $possibleLocalPaths += (Join-Path $currentDir "spotglow\src-tauri\target\release\bundle\nsis\SpotGlow_0.1.0_x64-setup.exe")
}

foreach ($cand in $possibleLocalPaths) {
    if (Test-Path $cand) {
        $localInstaller = $cand
        break
    }
}

# 2. Locate or Download Installer
if ($localInstaller) {
    Write-Host "  [+] Found local installer: $localInstaller" -ForegroundColor Green
    Copy-Item $localInstaller $tempInstaller -Force
} else {
    Write-Host "  [+] Fetching latest SpotGlow release from GitHub..." -ForegroundColor Yellow
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    $downloadUrl = $null
    try {
        $apiUrl = "https://api.github.com/repos/$repo/releases/latest"
        $release = Invoke-RestMethod -Uri $apiUrl -UseBasicParsing -ErrorAction Stop
        $exeAsset = $release.assets | Where-Object { $_.name -like "*setup*.exe" -or $_.name -like "*.exe" } | Select-Object -First 1
        if ($exeAsset) {
            $downloadUrl = $exeAsset.browser_download_url
        }
    } catch {
        # API might return 404 if no releases have been published yet
    }

    if (-not $downloadUrl) {
        $downloadUrl = "https://github.com/$repo/releases/latest/download/SpotGlow_0.1.0_x64-setup.exe"
    }

    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $tempInstaller -UseBasicParsing
    } catch {
        Write-Warning "Could not find a published release installer on GitHub."
        Write-Host ""
        Write-Host "  To install locally from source:" -ForegroundColor Yellow
        Write-Host "    1. Build the installer: npm run tauri build" -ForegroundColor Gray
        Write-Host "    2. Run installer script: .\install.ps1" -ForegroundColor Gray
        Write-Host ""
        Write-Host "  To distribute via the web:" -ForegroundColor Yellow
        Write-Host "    Create a release at https://github.com/$repo/releases and upload SpotGlow_0.1.0_x64-setup.exe" -ForegroundColor Gray
        Write-Host ""
        exit 1
    }
}

# 3. Silent Installation
Write-Host "  [+] Installing SpotGlow silently..." -ForegroundColor Green
$process = Start-Process -FilePath $tempInstaller -ArgumentList "/S" -Wait -PassThru

if ($process.ExitCode -eq 0) {
    Write-Host "  [OK] SpotGlow installed successfully!" -ForegroundColor Green
} else {
    Write-Warning "Installation finished with exit code $($process.ExitCode)."
}

# 4. Clean up installer temp file
Remove-Item $tempInstaller -Force -ErrorAction SilentlyContinue

# 5. Launch SpotGlow
$candidates = @(
    (Join-Path $env:LOCALAPPDATA "SpotGlow\spotglow.exe"),
    (Join-Path $env:LOCALAPPDATA "SpotGlow\SpotGlow.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\SpotGlow\spotglow.exe"),
    (Join-Path $env:LOCALAPPDATA "Programs\SpotGlow\SpotGlow.exe"),
    "C:\Program Files\SpotGlow\spotglow.exe",
    "C:\Program Files\SpotGlow\SpotGlow.exe"
)

$installedExe = $candidates | Where-Object { Test-Path $_ } | Select-Object -First 1

if ($installedExe) {
    Write-Host "  [>] Launching SpotGlow ($installedExe)..." -ForegroundColor Cyan
    Start-Process $installedExe
}

Write-Host "  Enjoy the glow! Control via System Tray, F8 (styles), or F9 (cover mode)." -ForegroundColor Magenta
Write-Host ""
