# Builds the unsigned MSIX for the Microsoft Store: the Store signs it after certification.
# Needs the Windows App Development CLI (winget install Microsoft.WinAppCli).
# Output: target/msix/magma-mcsr.msix
# The Store does not allow a version starting with 0, so app version 0.x.y is packaged as 1.x.y.0.
$ErrorActionPreference = 'Stop'
$app = Split-Path $PSScriptRoot
$root = Split-Path $app
Set-Location $app
npx tauri build --no-bundle
$layout = Join-Path $root 'target/msix/layout'
Remove-Item -Recurse -Force $layout -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $layout | Out-Null
Copy-Item (Join-Path $root 'target/release/magma-app.exe') $layout
$version = (Get-Content (Join-Path $app 'src-tauri/tauri.conf.json') -Raw | ConvertFrom-Json).version
$parts = $version.Split('.')
$manifest = (Get-Content (Join-Path $PSScriptRoot 'Package.appxmanifest') -Raw) -replace '(<Identity [^>]*Version=")[^"]*', ('${1}' + "1.$($parts[1]).$($parts[2]).0")
Set-Content (Join-Path $layout 'Package.appxmanifest') $manifest -NoNewline
Copy-Item -Recurse (Join-Path $PSScriptRoot 'Assets') $layout
Set-Location $layout
winapp package . --output ../magma-mcsr.msix --no-sign
