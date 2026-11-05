# Carrega .env e .env.local do repo para o processo atual.
# Dot-source: . .\Import-AiMemoryEnv.ps1  depois  Import-AiMemoryEnv -RepoRoot $path

function Import-DotEnvFile {
    param([string]$Path)
    if (-not (Test-Path $Path)) { return }
    Get-Content $Path | ForEach-Object {
        $line = $_.Trim()
        if ($line -eq "" -or $line.StartsWith("#")) { return }
        if ($line -match '^\s*([^#=]+)=(.*)$') {
            $name = $Matches[1].Trim()
            $value = $Matches[2].Trim().Trim('"').Trim("'")
            Set-Item -Path "Env:$name" -Value $value
        }
    }
}

function Import-AiMemoryEnv {
    param(
        [Parameter(Mandatory = $true)]
        [string]$RepoRoot
    )

    $root = $RepoRoot.TrimEnd('\', '/')
    Import-DotEnvFile (Join-Path $root ".env")
    Import-DotEnvFile (Join-Path $root ".env.local")

    if (-not $env:AI_MEMORY_DATA_DIR) {
        $env:AI_MEMORY_DATA_DIR = Join-Path $env:LOCALAPPDATA "ai-memory"
    }
}
