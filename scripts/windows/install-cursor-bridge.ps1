# Instala dependencias Node do bridge @cursor/sdk (uma vez).
$ErrorActionPreference = "Stop"
$BridgeDir = Join-Path (Split-Path -Parent (Split-Path -Parent $PSScriptRoot)) "scripts\cursor-bridge"
Set-Location $BridgeDir
if (-not (Get-Command node -ErrorAction SilentlyContinue)) {
    throw "Node.js not found on PATH — install from https://nodejs.org/"
}
Write-Host "npm install in $BridgeDir" -ForegroundColor Cyan
npm install
if (-not (Test-Path "$BridgeDir\node_modules\@cursor\sdk")) {
    throw "@cursor/sdk was not installed"
}
Write-Host "OK: cursor bridge ready" -ForegroundColor Green
