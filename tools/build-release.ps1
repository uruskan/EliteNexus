# Builds an optimised Elite Nexus, assembles the portable folder .\dist\EliteNexus and the release
# zip .\dist\EliteNexus-v<version>-windows-x64.zip.
#   powershell -ExecutionPolicy Bypass -File .\tools\build-release.ps1
# Your data (nexus-data next to the exe) is kept. If Elite Nexus is running from dist, the new exe is
# staged as EliteNexus.new.exe and swapped in by Update-EliteNexus.cmd once you close the app.
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
. (Join-Path $PSScriptRoot 'env.ps1')
Set-Location $root

cargo build --release -p nexus-app
if ($LASTEXITCODE -ne 0) { throw "cargo build failed" }

$version = (Select-String -Path (Join-Path $root 'Cargo.toml') -Pattern '^version = "(.+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$built = Join-Path $root 'target\release\EliteNexus.exe'
$dist = Join-Path $root 'dist\EliteNexus'
New-Item -ItemType Directory -Force -Path $dist | Out-Null
foreach ($f in 'LICENSE', 'LICENSE-ADDITIONAL-TERMS.md', 'NAME-POLICY.md', 'README.md', 'NOTICE.md', 'docs\USER_GUIDE.md') { Copy-Item (Join-Path $root $f) $dist -Force }
# The bundled Inter font's licence (SIL OFL) travels with the program.
Copy-Item (Join-Path $root 'data\fonts\inter\LICENSE.txt') (Join-Path $dist 'Inter-LICENSE.txt') -Force

$target = Join-Path $dist 'EliteNexus.exe'
$running = Get-Process -Name EliteNexus -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $target }
if ($running) {
    Copy-Item $built (Join-Path $dist 'EliteNexus.new.exe') -Force
    "Elite Nexus is running, so the new version is staged as dist\EliteNexus\EliteNexus.new.exe."
    "Close Elite Nexus, then run Update-EliteNexus.cmd (or start it with Start-EliteNexus.cmd, which swaps it in)."
} else {
    Copy-Item $built $target -Force
}

# Release zip: the exe and documents, never your data.
$zip = Join-Path $root "dist\EliteNexus-v$version-windows-x64.zip"
$stage = Join-Path $root 'target\release-zip\EliteNexus'
Remove-Item (Split-Path $stage) -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force -Path $stage | Out-Null
Copy-Item $built $stage
foreach ($f in 'LICENSE', 'LICENSE-ADDITIONAL-TERMS.md', 'NAME-POLICY.md', 'README.md', 'NOTICE.md', 'docs\USER_GUIDE.md') { Copy-Item (Join-Path $root $f) $stage }
Copy-Item (Join-Path $root 'data\fonts\inter\LICENSE.txt') (Join-Path $stage 'Inter-LICENSE.txt')
Compress-Archive -Path $stage -DestinationPath $zip -Force
$exe = Get-Item $built
"Built Elite Nexus {0}: {1:N1} MB exe, zip {2:N1} MB -> {3}" -f $version, ($exe.Length / 1MB), ((Get-Item $zip).Length / 1MB), $zip
