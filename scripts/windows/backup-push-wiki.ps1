# Empurra o wiki (markdown versionado) para o remote.
# Chamado pelo Task Scheduler em dias uteis.
#
# Robustez:
# - mutex para nao sobrepor duas execucoes do scheduler
# - remove index.lock obsoleto (crash/concorrencia com ai-memory)
# - git stderr ("Everything up-to-date") nao vira NativeCommandError fatal
# - retry curto se o servidor estiver commitando ao mesmo tempo
#
# Importante: manter este arquivo em ASCII (ou UTF-8 COM BOM).
# Windows PowerShell 5.1 le .ps1 sem BOM como ANSI; bytes UTF-8 de
# em-dash (E2 80 94) viram aspas CP1252 e quebram try/finally.

[CmdletBinding()]
param(
    [string]$WikiDir = $(Join-Path $env:USERPROFILE ".ai-memory\wiki"),
    [int]$StaleLockMinutes = 5,
    [int]$MaxRetries = 3,
    [int]$RetryDelaySec = 5
)

$ErrorActionPreference = "Stop"
$script:ExitCode = 1

function Write-BackupLog {
    param([string]$Message, [string]$Color = "Gray")
    Write-Host "[backup-push-wiki] $Message" -ForegroundColor $Color
}

function Invoke-Git {
    param(
        [Parameter(Mandatory, ValueFromRemainingArguments = $true)]
        [string[]]$GitArgs
    )
    # git writes progress/status to stderr; PowerShell must not treat that as failure.
    $prev = $ErrorActionPreference
    $ErrorActionPreference = "Continue"
    $raw = & git @GitArgs 2>&1
    $code = $LASTEXITCODE
    $ErrorActionPreference = $prev

    $lines = @(
        foreach ($item in @($raw)) {
            if ($null -eq $item) { continue }
            if ($item -is [System.Management.Automation.ErrorRecord]) {
                $item.Exception.Message
            } else {
                [string]$item
            }
        }
    )
    [pscustomobject]@{
        ExitCode = $code
        Output   = ($lines -join "`n").Trim()
        Lines    = $lines
    }
}

function Test-GitProcessInRepo {
    param([string]$RepoPath)
    $norm = (Resolve-Path -LiteralPath $RepoPath).Path.Replace('\', '/').ToLowerInvariant()
    Get-CimInstance Win32_Process -ErrorAction SilentlyContinue |
        Where-Object {
            $_.Name -match '^git(\.exe)?$' -and
            $_.CommandLine -and
            $_.CommandLine.ToLowerInvariant().Replace('\', '/').Contains($norm)
        }
}

function Clear-StaleGitIndexLock {
    param(
        [string]$RepoPath,
        [int]$MaxAgeMinutes
    )
    $lock = Join-Path $RepoPath ".git\index.lock"
    if (-not (Test-Path -LiteralPath $lock)) { return $false }

    $info = Get-Item -LiteralPath $lock
    $ageMin = [math]::Round(((Get-Date) - $info.LastWriteTime).TotalMinutes, 1)
    $gitProcs = @(Test-GitProcessInRepo -RepoPath $RepoPath)

    if ($gitProcs.Count -gt 0 -and $ageMin -lt $MaxAgeMinutes) {
        Write-BackupLog "index.lock presente e git ativo (age=${ageMin}m) - aguardando" "Yellow"
        return $false
    }

    if ($ageMin -lt $MaxAgeMinutes -and $gitProcs.Count -eq 0) {
        Write-BackupLog "removendo index.lock orfao age=${ageMin}m (sem processo git)" "Yellow"
    } elseif ($ageMin -ge $MaxAgeMinutes) {
        Write-BackupLog "removendo index.lock obsoleto age=${ageMin}m" "Yellow"
    }

    Remove-Item -LiteralPath $lock -Force -ErrorAction Stop
    return $true
}

if (-not (Test-Path -LiteralPath (Join-Path $WikiDir ".git"))) {
    Write-BackupLog "ERRO: $WikiDir nao tem .git" "Red"
    exit 1
}

# Single-instance mutex (global so Task Scheduler overlaps are skipped)
$mutexName = "Global\ai-memory-backup-push-wiki"
$mutex = $null
$created = $false
try {
    $mutex = New-Object System.Threading.Mutex($false, $mutexName, [ref]$created)
} catch {
    $mutex = New-Object System.Threading.Mutex($false, "Local\ai-memory-backup-push-wiki", [ref]$created)
}

$owned = $false
$pushedLocation = $false
try {
    $owned = $mutex.WaitOne(0)
    if (-not $owned) {
        Write-BackupLog "outra instancia ja em execucao - saindo" "Yellow"
        $script:ExitCode = 0
    } else {
        Push-Location -LiteralPath $WikiDir
        $pushedLocation = $true

        [void](Clear-StaleGitIndexLock -RepoPath $WikiDir -MaxAgeMinutes $StaleLockMinutes)

        $remote = Invoke-Git remote
        if ($remote.ExitCode -ne 0 -or [string]::IsNullOrWhiteSpace($remote.Output)) {
            Write-BackupLog "Nenhum remote configurado, pulando" "Yellow"
            $script:ExitCode = 0
        } else {
            $attempt = 0
            $pushed = $false
            $failed = $false
            while ($attempt -lt $MaxRetries -and -not $pushed -and -not $failed) {
                $attempt++
                try {
                    [void](Clear-StaleGitIndexLock -RepoPath $WikiDir -MaxAgeMinutes $StaleLockMinutes)

                    $status = Invoke-Git status --porcelain
                    if ($status.ExitCode -ne 0) {
                        throw "git status falhou: $($status.Output)"
                    }

                    if (-not [string]::IsNullOrWhiteSpace($status.Output)) {
                        $add = Invoke-Git add -A
                        if ($add.ExitCode -ne 0) {
                            throw "git add falhou: $($add.Output)"
                        }
                        $msg = "chore(wiki): backup automatico $(Get-Date -Format 'yyyy-MM-ddTHHmmss')"
                        $commit = Invoke-Git commit -m $msg
                        if ($commit.ExitCode -ne 0 -and $commit.Output -notmatch 'nothing to commit') {
                            throw "git commit falhou: $($commit.Output)"
                        }
                        if ($commit.Output) {
                            Write-BackupLog $commit.Output "DarkGray"
                        }
                    }

                    $push = Invoke-Git push
                    if ($push.ExitCode -ne 0) {
                        throw "git push falhou (exit $($push.ExitCode)): $($push.Output)"
                    }
                    if ($push.Output) {
                        Write-BackupLog $push.Output "DarkGray"
                    }
                    $pushed = $true
                } catch {
                    $errText = "$_"
                    $isLock = $errText -match 'index\.lock|Unable to create'
                    if ($isLock -and $attempt -lt $MaxRetries) {
                        Write-BackupLog "conflito de lock (tentativa $attempt/$MaxRetries): $errText" "Yellow"
                        Start-Sleep -Seconds $RetryDelaySec
                        [void](Clear-StaleGitIndexLock -RepoPath $WikiDir -MaxAgeMinutes 0)
                    } else {
                        Write-BackupLog "ERRO: $errText" "Red"
                        $failed = $true
                    }
                }
            }

            if ($pushed) {
                Write-BackupLog "OK $(Get-Date -Format HH:mm)" "Green"
                $script:ExitCode = 0
            } else {
                $script:ExitCode = 1
            }
        }
    }
} finally {
    if ($pushedLocation) {
        Pop-Location
    }
    if ($owned -and $mutex) {
        [void]$mutex.ReleaseMutex()
    }
    if ($mutex) {
        $mutex.Dispose()
    }
}

exit $script:ExitCode
