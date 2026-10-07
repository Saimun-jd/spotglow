# SpotGlow One-Command Installer for Windows 10/11
# Usage:
#   .\install.ps1
#   .\install.ps1 -CheckOnly
#   .\install.ps1 -Force
#   irm https://raw.githubusercontent.com/Saimun-jd/spotglow/main/install.ps1 | iex

param(
    [switch]$CheckOnly,
    [switch]$Force,
    [switch]$Reinstall
)

$ErrorActionPreference = "Stop"

$TARGET_VERSION = "0.2.0"
$repo = "Saimun-jd/spotglow"

Write-Host ""
Write-Host "  === SpotGlow - Ambient Spotify Window Border Glow (v$TARGET_VERSION) ===" -ForegroundColor Cyan
Write-Host "  ------------------------------------------------------------" -ForegroundColor DarkGray

# 1. Architecture Check
if ([IntPtr]::Size -ne 8) {
    Write-Error "SpotGlow requires 64-bit Windows."
    exit 1
}

# 2. Check if SpotGlow is Already Installed
function Get-InstalledSpotGlow {
    $result = @{
        Installed       = $false
        Version         = "Unknown"
        InstallLocation = $null
        ExePath         = $null
        IsRunning       = $false
    }

    # Check running processes
    $proc = Get-Process -Name "spotglow" -ErrorAction SilentlyContinue
    if ($proc) {
        $result.IsRunning = $true
    }

    # Check Registry entries
    $regPaths = @(
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\com.spotglow.app",
        "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SpotGlow",
        "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\com.spotglow.app",
        "HKLM:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SpotGlow"
    )

    foreach ($regPath in $regPaths) {
        if (Test-Path $regPath) {
            $props = Get-ItemProperty -Path $regPath -ErrorAction SilentlyContinue
            if ($props) {
                $result.Installed = $true
                if ($props.DisplayVersion) { $result.Version = $props.DisplayVersion }
                if ($props.InstallLocation) { $result.InstallLocation = $props.InstallLocation }
                break
            }
        }
    }

    # Check Filesystem locations
    $candidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\SpotGlow\spotglow.exe"),
        (Join-Path $env:LOCALAPPDATA "Programs\SpotGlow\SpotGlow.exe"),
        (Join-Path $env:LOCALAPPDATA "SpotGlow\spotglow.exe"),
        (Join-Path $env:LOCALAPPDATA "SpotGlow\SpotGlow.exe"),
        "C:\Program Files\SpotGlow\spotglow.exe",
        "C:\Program Files\SpotGlow\SpotGlow.exe"
    )

    foreach ($cand in $candidates) {
        if (Test-Path $cand) {
            $result.Installed = $true
            $result.ExePath = $cand
            if (-not $result.InstallLocation) {
                $result.InstallLocation = Split-Path $cand -Parent
            }
            if ($result.Version -eq "Unknown") {
                try {
                    $ver = (Get-Item $cand).VersionInfo.ProductVersion
                    if ($ver) { $result.Version = $ver }
                } catch {}
            }
            break
        }
    }

    return $result
}

$currentInstall = Get-InstalledSpotGlow

# Handle -CheckOnly switch
if ($CheckOnly) {
    if ($currentInstall.Installed) {
        Write-Host "  [i] SpotGlow is currently installed:" -ForegroundColor Cyan
        Write-Host "      Installed Version: v$($currentInstall.Version)" -ForegroundColor Gray
        Write-Host "      Target Release:    v$TARGET_VERSION" -ForegroundColor Gray
        Write-Host "      Install Directory: $($currentInstall.InstallLocation)" -ForegroundColor Gray
        Write-Host "      Process Status:    $($(if ($currentInstall.IsRunning) { 'Running' } else { 'Stopped' }))" -ForegroundColor Gray
    } else {
        Write-Host "  [-] SpotGlow is not currently installed on this system." -ForegroundColor Yellow
        Write-Host "      Ready for fresh installation of v$TARGET_VERSION." -ForegroundColor DarkGray
    }
    Write-Host ""
    exit 0
}

# Provide installation pre-check feedback
if ($currentInstall.Installed) {
    Write-Host "  [i] Existing SpotGlow installation detected:" -ForegroundColor Cyan
    Write-Host "      Version:  v$($currentInstall.Version) (Installed) -> v$TARGET_VERSION (Target)" -ForegroundColor Gray
    Write-Host "      Location: $($currentInstall.InstallLocation)" -ForegroundColor Gray
    Write-Host "      Status:   Preparing safe in-place upgrade..." -ForegroundColor Yellow
    Write-Host ""
} else {
    Write-Host "  [+] No previous SpotGlow installation found. Proceeding with fresh setup..." -ForegroundColor Green
    Write-Host ""
}

# Stop any running SpotGlow instance so binaries are not locked during install/update
if ($currentInstall.IsRunning) {
    Write-Host "  [+] Stopping running SpotGlow instance to unlock files..." -ForegroundColor Yellow
    Stop-Process -Name "spotglow" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 400
}

$tempInstaller = Join-Path $env:TEMP "SpotGlow_Setup.exe"

# 3. Locate Local Build Installer or Download from GitHub Releases
$localInstaller = $null
$possibleLocalPaths = @()

if ($PSScriptRoot) {
    $possibleLocalPaths += (Join-Path $PSScriptRoot "src-tauri\target\release\bundle\nsis\SpotGlow_${TARGET_VERSION}_x64-setup.exe")
    $possibleLocalPaths += (Join-Path $PSScriptRoot "src-tauri\target\release\bundle\nsis\SpotGlow_0.1.0_x64-setup.exe")
}

$currentDir = (Get-Location).Path
if ($currentDir) {
    $possibleLocalPaths += (Join-Path $currentDir "src-tauri\target\release\bundle\nsis\SpotGlow_${TARGET_VERSION}_x64-setup.exe")
    $possibleLocalPaths += (Join-Path $currentDir "spotglow\src-tauri\target\release\bundle\nsis\SpotGlow_${TARGET_VERSION}_x64-setup.exe")
    $possibleLocalPaths += (Join-Path $currentDir "src-tauri\target\release\bundle\nsis\SpotGlow_0.1.0_x64-setup.exe")
}

foreach ($cand in $possibleLocalPaths) {
    if (Test-Path $cand) {
        $localInstaller = $cand
        break
    }
}

# Remove stale temp installer
Remove-Item $tempInstaller -Force -ErrorAction SilentlyContinue

if ($localInstaller) {
    Write-Host "  [+] Found local build installer: $localInstaller" -ForegroundColor Green
    Copy-Item $localInstaller $tempInstaller -Force
} else {
    Write-Host "  [+] Fetching latest SpotGlow v$TARGET_VERSION from GitHub Releases..." -ForegroundColor Yellow
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
        # Releases API might return 404 if draft or repo pending
    }

    if (-not $downloadUrl) {
        $downloadUrl = "https://github.com/$repo/releases/download/v$TARGET_VERSION/SpotGlow_${TARGET_VERSION}_x64-setup.exe"
    }

    try {
        Invoke-WebRequest -Uri $downloadUrl -OutFile $tempInstaller -UseBasicParsing
    } catch {
        Write-Warning "Could not download published release installer from GitHub."
        Write-Host ""
        Write-Host "  To build and install locally from source:" -ForegroundColor Yellow
        Write-Host "    1. Build the installer: npm run tauri build" -ForegroundColor Gray
        Write-Host "    2. Run installer script: .\install.ps1" -ForegroundColor Gray
        Write-Host ""
        Write-Host "  To distribute online:" -ForegroundColor Yellow
        Write-Host "    Create release v$TARGET_VERSION at https://github.com/$repo/releases and upload SpotGlow_${TARGET_VERSION}_x64-setup.exe" -ForegroundColor Gray
        Write-Host ""
        exit 1
    }
}

# 4. Silent Installation
Write-Host "  [+] Installing SpotGlow v$TARGET_VERSION silently..." -ForegroundColor Green
$process = Start-Process -FilePath $tempInstaller -ArgumentList "/S" -Wait -PassThru

if ($process.ExitCode -eq 0) {
    Write-Host "  [OK] SpotGlow v$TARGET_VERSION installed successfully!" -ForegroundColor Green
} else {
    Write-Warning "Installation finished with exit code $($process.ExitCode)."
}

# 5. Clean up temporary installer
Remove-Item $tempInstaller -Force -ErrorAction SilentlyContinue

# 6. Launch SpotGlow
$postInstall = Get-InstalledSpotGlow
$installedExe = $postInstall.ExePath

if (-not $installedExe) {
    $searchCandidates = @(
        (Join-Path $env:LOCALAPPDATA "Programs\SpotGlow\spotglow.exe"),
        (Join-Path $env:LOCALAPPDATA "SpotGlow\spotglow.exe"),
        "C:\Program Files\SpotGlow\spotglow.exe"
    )
    $installedExe = $searchCandidates | Where-Object { Test-Path $_ } | Select-Object -First 1
}

function Add-DirectoryToUserPath {
    param([string]$Directory)
    if (-not $Directory -or -not (Test-Path $Directory)) { return }

    try {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        $pathItems = if ($userPath) { $userPath -split ';' | Where-Object { $_ } } else { @() }

        $alreadyPresent = $false
        foreach ($p in $pathItems) {
            if ($p.TrimEnd('\') -ieq $Directory.TrimEnd('\')) {
                $alreadyPresent = $true
                break
            }
        }

        if (-not $alreadyPresent) {
            $newPath = if ($userPath) { "$userPath;$Directory" } else { $Directory }
            [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
            Write-Host "  [+] Added '$Directory' to User PATH environment variable." -ForegroundColor Green
        }

        # Also update current session PATH so 'spotglow' works immediately in this terminal!
        $sessionPaths = $env:Path -split ';'
        $sessionPresent = $false
        foreach ($p in $sessionPaths) {
            if ($p.TrimEnd('\') -ieq $Directory.TrimEnd('\')) {
                $sessionPresent = $true
                break
            }
        }
        if (-not $sessionPresent) {
            $env:Path = "$env:Path;$Directory"
        }
    } catch {
        Write-Warning "Could not update User PATH: $_"
    }
}

if ($installedExe) {
    $installDir = Split-Path $installedExe -Parent
    Add-DirectoryToUserPath $installDir
    Write-Host "  [>] Launching SpotGlow ($installedExe)..." -ForegroundColor Cyan
    Start-Process $installedExe
}

Write-Host "  SpotGlow is running in system tray! Control via Tray Icon, F8 (styles), or F9 (cover mode)." -ForegroundColor Magenta
Write-Host "  SpotGlow added to PATH: run 'spotglow' from any terminal anytime!" -ForegroundColor Green
Write-Host "  To uninstall anytime via terminal: spotglow --uninstall OR .\uninstall.ps1" -ForegroundColor DarkCyan
Write-Host ""
