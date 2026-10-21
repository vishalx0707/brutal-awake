$ErrorActionPreference = 'Stop'
Push-Location $PSScriptRoot
try {
    cargo build --release --locked
    if ($LASTEXITCODE -ne 0) { throw 'Build failed.' }
    New-Item -ItemType Directory -Force -Path (Join-Path $PSScriptRoot 'dist') | Out-Null
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'target\release\brutal-awake.exe') -Destination (Join-Path $PSScriptRoot 'dist\BrutalAwake.exe')
    Write-Host 'Ready: dist\BrutalAwake.exe. This single file is all you need to run the toggle.'
} finally { Pop-Location }
