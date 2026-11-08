# Build SensMatch for Windows x64 and package it as dist\SensMatch-v1.0.0-win-x64.zip
# Usage: powershell -ExecutionPolicy Bypass -File scripts\build.ps1 [-NoBundle]
param([switch]$NoBundle)
$ErrorActionPreference = 'Stop'

$root    = Split-Path -Parent $PSScriptRoot
$tauri   = Join-Path $root 'src-tauri'
$dist    = Join-Path $root 'dist'
$release = 'SensMatch-v1.0.0-win-x64.zip'

Push-Location $tauri
try {
    if (-not $NoBundle -and (Get-Command cargo-tauri -ErrorAction SilentlyContinue)) {
        cargo tauri build
    } else {
        Write-Host 'cargo-tauri not found or -NoBundle set: building the plain release binary'
        cargo build --release
    }
} finally { Pop-Location }

$target = if ($env:CARGO_TARGET_DIR) { $env:CARGO_TARGET_DIR } else { Join-Path $tauri 'target' }
$exe = Join-Path $target 'release\SensMatch.exe'
if (-not (Test-Path $exe)) { $exe = Join-Path $target 'release\sensmatch.exe' }
if (-not (Test-Path $exe)) { throw "Release binary not found in $target\release" }

$stage = Join-Path $dist 'SensMatch'
Remove-Item $stage -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item $exe (Join-Path $stage 'SensMatch.exe')
Copy-Item (Join-Path $root 'LICENSE') $stage

$zip = Join-Path $dist $release
Remove-Item $zip -Force -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage '*') -DestinationPath $zip
Write-Host ("Packaged {0} ({1:N1} MB)" -f $zip, ((Get-Item $zip).Length / 1MB))
