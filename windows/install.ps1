# Builds xara for Windows and installs it for this user (no administrator needed):
# the app (xara-app.exe), the xara and xara-monitor commands, a Start menu and a desktop
# shortcut. Run from anywhere:
#
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1            # build and install
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1 -NoBuild   # install what is built
#   powershell -ExecutionPolicy Bypass -File windows\install.ps1 -Uninstall
param([switch]$NoBuild, [switch]$Uninstall)
$ErrorActionPreference = "Stop"

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$root = Split-Path -Parent $here
$dest = Join-Path $env:LOCALAPPDATA "Programs\xara"
$menu = Join-Path ([Environment]::GetFolderPath("Programs")) "xara.lnk"
$desk = Join-Path ([Environment]::GetFolderPath("Desktop")) "xara.lnk"

if ($Uninstall) {
    Get-Process xara-app, xara -ErrorAction SilentlyContinue | Stop-Process -Force
    Remove-Item $menu, $desk -ErrorAction SilentlyContinue
    Remove-Item $dest -Recurse -Force -ErrorAction SilentlyContinue
    Write-Host "xara removed (settings stay in $env:APPDATA\xara)"
    exit 0
}

if (-not $NoBuild) {
    cargo build --release --manifest-path (Join-Path $root "Cargo.toml") -p xara
    if ($LASTEXITCODE -ne 0) { throw "building xara failed" }
    cargo build --release --manifest-path (Join-Path $here "Cargo.toml")
    if ($LASTEXITCODE -ne 0) { throw "building the xara app failed" }
}

# A running copy holds its exe open.
Get-Process xara-app -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$dest*" } | Stop-Process -Force
New-Item -ItemType Directory -Force $dest | Out-Null
Copy-Item (Join-Path $here "target\release\xara-app.exe") $dest -Force
foreach ($exe in "xara.exe", "xara-monitor.exe") {
    Copy-Item (Join-Path $root "target\release\$exe") $dest -Force
}

$shell = New-Object -ComObject WScript.Shell
foreach ($lnk in $menu, $desk) {
    $s = $shell.CreateShortcut($lnk)
    $s.TargetPath = Join-Path $dest "xara-app.exe"
    $s.WorkingDirectory = $dest
    $s.Description = "xara modem (VARA compatible)"
    $s.Save()
}
Write-Host "xara installed in $dest, with Start menu and desktop shortcuts."
Write-Host "The xara command is $dest\xara.exe (add that folder to PATH to type just 'xara')."
