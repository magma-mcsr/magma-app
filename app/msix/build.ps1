# Builds the unsigned MSIX for the Microsoft Store: the Store signs it after certification.
# Needs the Windows App Development CLI (winget install Microsoft.WinAppCli).
# Output: target/msix/magma-mcsr.msix
$ErrorActionPreference = 'Stop'
$app = Split-Path $PSScriptRoot
$root = Split-Path $app
Set-Location $app
npx tauri build --no-bundle
$layout = Join-Path $root 'target/msix/layout'
Remove-Item -Recurse -Force $layout -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $layout | Out-Null
Copy-Item (Join-Path $root 'target/release/magma-app.exe') $layout
Copy-Item (Join-Path $PSScriptRoot 'Package.appxmanifest') $layout
Copy-Item -Recurse (Join-Path $PSScriptRoot 'Assets') $layout
Set-Location $layout
winapp package . --output ../magma-mcsr.msix --no-sign
