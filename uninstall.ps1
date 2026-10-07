# SpotGlow Terminal Uninstaller for Windows 10/11
# Usage:
#   .\uninstall.ps1
#   .\uninstall.ps1 -Purge
#   irm https://raw.githubusercontent.com/Saimun-jd/spotglow/main/uninstall.ps1 | iex

param(
    [switch]$Purge,
    [switch]$Force,
    [switch]$CheckOnly
)

$ErrorActionPreference = "Stop"

Write-Host ""
Write-Host "  === SpotGlow Terminal Uninstaller ===" -ForegroundColor Red
Write-Host "  -------------------------------------" -ForegroundColor DarkGray

# 1. Detection Function
function Get-SpotGlowInstallation {
    $result = @{
        Installed       = $false
        Version         = "Unknown"
        InstallLocation = $null
        UninstallerPath = $null
        ExePath         = $null
    }

    # A. Check Registry
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
                if ($props.UninstallString) {
                    $uStr = $props.UninstallString.Trim().Trim('"')
                    if (Test-Path $uStr) { $result.UninstallerPath = $uStr }
                }
                break
            }
        }
    }

    # B. Check Filesystem candidate paths
    $candidateDirs = @(
        (Join-Path $env:LOCALAPPDATA "Programs\SpotGlow"),
        (Join-Path $env:LOCALAPPDATA "SpotGlow"),
        "C:\Program Files\SpotGlow"
    )

    foreach ($dir in $candidateDirs) {
        $exe = Join-Path $dir "spotglow.exe"
        if (-not (Test-Path $exe)) { $exe = Join-Path $dir "SpotGlow.exe" }
        if (Test-Path $exe) {
            $result.Installed = $true
            $result.ExePath = $exe
            if (-not $result.InstallLocation) { $result.InstallLocation = $dir }
            if ($result.Version -eq "Unknown") {
                try {
                    $ver = (Get-Item $exe).VersionInfo.ProductVersion
                    if ($ver) { $result.Version = $ver }
                } catch {}
            }
            $uninst = Join-Path $dir "Uninstall SpotGlow.exe"
            if ((Test-Path $uninst) -and (-not $result.UninstallerPath)) {
                $result.UninstallerPath = $uninst
            }
            break
        }
    }

    return $result
}

$installInfo = Get-SpotGlowInstallation

if ($CheckOnly) {
    if ($installInfo.Installed) {
        Write-Host "  [i] SpotGlow is currently installed:" -ForegroundColor Cyan
        Write-Host "      Version:   v$($installInfo.Version)" -ForegroundColor Gray
        Write-Host "      Location:  $($installInfo.InstallLocation)" -ForegroundColor Gray
        if ($installInfo.UninstallerPath) {
            Write-Host "      Uninstaller: $($installInfo.UninstallerPath)" -ForegroundColor Gray
        }
    } else {
        Write-Host "  [-] SpotGlow is not installed on this system." -ForegroundColor Yellow
    }
    Write-Host ""
    exit 0
}

if (-not $installInfo.Installed) {
    Write-Host "  [-] SpotGlow was not found in registry or standard install locations." -ForegroundColor Yellow
    Write-Host "      Nothing to uninstall." -ForegroundColor DarkGray
    Write-Host ""
    exit 0
}

Write-Host "  [i] Detected SpotGlow installation:" -ForegroundColor Cyan
Write-Host "      Version:  v$($installInfo.Version)" -ForegroundColor Gray
Write-Host "      Location: $($installInfo.InstallLocation)" -ForegroundColor Gray
Write-Host ""

# 2. Stop running process
$running = Get-Process -Name "spotglow" -ErrorAction SilentlyContinue
if ($running) {
    Write-Host "  [+] Terminating active SpotGlow processes..." -ForegroundColor Yellow
    Stop-Process -Name "spotglow" -Force -ErrorAction SilentlyContinue
    Start-Sleep -Milliseconds 400
}

# 3. Execute Official Uninstaller if present
if ($installInfo.UninstallerPath -and (Test-Path $installInfo.UninstallerPath)) {
    Write-Host "  [+] Executing official uninstaller silently (/S)..." -ForegroundColor Yellow
    $p = Start-Process -FilePath $installInfo.UninstallerPath -ArgumentList "/S" -Wait -PassThru
    if ($p.ExitCode -eq 0) {
        Write-Host "  [OK] Uninstaller execution completed." -ForegroundColor Green
    } else {
        Write-Warning "Uninstaller returned exit code: $($p.ExitCode)"
    }
}

# Ensure install directory is completely removed
if ($installInfo.InstallLocation -and (Test-Path $installInfo.InstallLocation)) {
    Remove-Item -Path $installInfo.InstallLocation -Recurse -Force -ErrorAction SilentlyContinue
    if (-not (Test-Path $installInfo.InstallLocation)) {
        Write-Host "  [+] Cleaned installation folder: $($installInfo.InstallLocation)" -ForegroundColor Green
    }
}

# 4. Remove Shortcuts
$shortcuts = @(
    (Join-Path $env:APPDATA "Microsoft\Windows\Start Menu\Programs\SpotGlow.lnk"),
    (Join-Path ([Environment]::GetFolderPath("Desktop")) "SpotGlow.lnk"),
    (Join-Path $env:PUBLIC "Desktop\SpotGlow.lnk")
)

foreach ($sc in $shortcuts) {
    if (Test-Path $sc) {
        Remove-Item -Path $sc -Force -ErrorAction SilentlyContinue
        Write-Host "  [+] Removed shortcut: $sc" -ForegroundColor Green
    }
}

# 5. Clean Registry entries if still present
$regKeysToClean = @(
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\com.spotglow.app",
    "HKCU:\Software\Microsoft\Windows\CurrentVersion\Uninstall\SpotGlow"
)
foreach ($rk in $regKeysToClean) {
    if (Test-Path $rk) {
        Remove-Item -Path $rk -Recurse -Force -ErrorAction SilentlyContinue
        Write-Host "  [+] Cleaned registry entry: $rk" -ForegroundColor Green
    }
}

# 6. Remove SpotGlow from User PATH environment variable
if ($installInfo.InstallLocation) {
    try {
        $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
        if ($userPath) {
            $pathItems = $userPath -split ';'
            $cleanItems = @()
            $changed = $false
            foreach ($p in $pathItems) {
                if ($p -and ($p.TrimEnd('\') -ieq $installInfo.InstallLocation.TrimEnd('\'))) {
                    $changed = $true
                } elseif ($p) {
                    $cleanItems += $p
                }
            }
            if ($changed) {
                $newUserPath = $cleanItems -join ';'
                [Environment]::SetEnvironmentVariable("Path", $newUserPath, "User")
                Write-Host "  [+] Removed '$($installInfo.InstallLocation)' from User PATH." -ForegroundColor Green
            }
        }
    } catch {
        Write-Warning "Could not clean User PATH: $_"
    }
}

# 6. Optional: Purge User Data / Settings
if ($Purge) {
    Write-Host "  [+] Purging SpotGlow user cache and settings (-Purge requested)..." -ForegroundColor Yellow
    $purgeDirs = @(
        (Join-Path $env:LOCALAPPDATA "SpotGlow"),
        (Join-Path $env:APPDATA "com.spotglow.app"),
        (Join-Path $env:APPDATA "SpotGlow")
    )
    foreach ($pDir in $purgeDirs) {
        if (Test-Path $pDir) {
            Remove-Item -Path $pDir -Recurse -Force -ErrorAction SilentlyContinue
            Write-Host "  [+] Purged: $pDir" -ForegroundColor Green
        }
    }
}

Write-Host ""
Write-Host "  [OK] SpotGlow has been completely uninstalled from this system." -ForegroundColor Green
Write-Host ""
