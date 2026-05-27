# Instala o comando global ai-memory (estilo agentmemory no PATH).
param(
    [string]$RepoRoot = "C:\GIT\ai-memory",
    [string]$BinDir = $(Join-Path $env:USERPROFILE ".local\bin"),
    [switch]$AddToPath
)

$ErrorActionPreference = "Stop"
$RepoRoot = (Resolve-Path $RepoRoot).Path
$Exe = Join-Path $RepoRoot "target\release\ai-memory.exe"

if (-not (Test-Path $Exe)) {
    Write-Host "Binary missing — building..." -ForegroundColor Yellow
    & (Join-Path $RepoRoot "scripts\windows\build.ps1")
}

New-Item -ItemType Directory -Force -Path $BinDir | Out-Null

$ps1Path = Join-Path $BinDir "ai-memory.ps1"
$cmdPath = Join-Path $BinDir "ai-memory.cmd"
$legacyPs1Path = Join-Path $BinDir "ai_memory.ps1"
$legacyCmdPath = Join-Path $BinDir "ai_memory.cmd"

$launcher = @"
# ai-memory — launcher global (gerado por install-global.ps1)
`$ErrorActionPreference = "Stop"

`$RepoRoot = if (`$env:AI_MEMORY_REPO) { `$env:AI_MEMORY_REPO } else { "$RepoRoot" }
`$Bin = Join-Path `$RepoRoot "target\release\ai-memory.exe"

. (Join-Path `$RepoRoot "scripts\windows\Import-AiMemoryEnv.ps1")
Import-AiMemoryEnv -RepoRoot `$RepoRoot

if (-not (Test-Path `$Bin)) {
    Write-Error "ai-memory.exe not found. Build: cd `$RepoRoot; .\scripts\windows\build.ps1"
}

`$userArgs = @(`$args)
if (`$userArgs.Count -eq 0) {
    `$userArgs = @("serve", "--transport", "http", "--bind", "127.0.0.1:49374")
} elseif (`$userArgs[0] -eq "serve" -and -not (`$userArgs | Where-Object { `$_ -eq "--transport" -or `$_ -like "--transport=*" })) {
    # serve sem --transport cai em stdio (sem /web nem MCP HTTP do Cursor)
    `$userArgs = @("serve", "--transport", "http", "--bind", "127.0.0.1:49374") + `$userArgs[1..(`$userArgs.Length - 1)]
}

& `$Bin @userArgs
exit `$LASTEXITCODE
"@

Set-Content -Path $ps1Path -Value $launcher -Encoding UTF8

$cmd = @"
@echo off
setlocal
where pwsh >nul 2>&1
if %ERRORLEVEL%==0 (
  pwsh -NoProfile -ExecutionPolicy Bypass -File "%USERPROFILE%\.local\bin\ai-memory.ps1" %*
  exit /b %ERRORLEVEL%
)
powershell -NoProfile -ExecutionPolicy Bypass -File "%USERPROFILE%\.local\bin\ai-memory.ps1" %*
exit /b %ERRORLEVEL%
"@
Set-Content -Path $cmdPath -Value $cmd -Encoding ASCII

# Shims legados (ai_memory → ai-memory) para instalações antigas no PATH.
$legacyPs1 = @"
# Deprecated: use ai-memory. Encaminha para ai-memory.ps1.
& (Join-Path `$PSScriptRoot "ai-memory.ps1") @args
exit `$LASTEXITCODE
"@
Set-Content -Path $legacyPs1Path -Value $legacyPs1 -Encoding UTF8

$legacyCmd = @"
@echo off
"%~dp0ai-memory.cmd" %*
"@
Set-Content -Path $legacyCmdPath -Value $legacyCmd -Encoding ASCII

if ($AddToPath) {
    $userPath = [Environment]::GetEnvironmentVariable("Path", "User")
    if ($userPath -notlike "*$BinDir*") {
        $newPath = if ($userPath) { "$userPath;$BinDir" } else { $BinDir }
        [Environment]::SetEnvironmentVariable("Path", $newPath, "User")
        $env:Path = "$env:Path;$BinDir"
        Write-Host "Added to user PATH: $BinDir" -ForegroundColor Green
    } else {
        Write-Host "PATH already contains $BinDir" -ForegroundColor DarkGray
    }
}

Write-Host "Installed:" -ForegroundColor Green
Write-Host "  $ps1Path"
Write-Host "  $cmdPath"
Write-Host "  $legacyPs1Path (shim → ai-memory)"
Write-Host "  $legacyCmdPath (shim → ai-memory)"
Write-Host ""
Write-Host "Usage:" -ForegroundColor Cyan
Write-Host "  ai-memory              # start server (serve)"
Write-Host "  ai-memory status --json"
Write-Host "  ai-memory search `"termo`""
Write-Host ""
Write-Host "Env: $RepoRoot\.env (+ .env.local if present)" -ForegroundColor DarkGray
