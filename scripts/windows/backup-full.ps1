# Backup completo (wiki + SQLite + config) via ai-memory backup.
# Mantem apenas os 30 dias mais recentes.
$ErrorActionPreference = "Stop"

$BackupDir = "$env:USERPROFILE\ai-memory-backups"
if (-not (Test-Path $BackupDir)) {
    New-Item -ItemType Directory -Path $BackupDir -Force | Out-Null
}

$date = Get-Date -Format yyyyMMdd
$tarball = Join-Path $BackupDir "ai-memory-$date.tar.gz"

try {
    ai-memory backup --to $tarball
    Write-Host "[backup-full] OK $tarball" -ForegroundColor Green

    # Limpa backups com mais de 30 dias
    $cutoff = (Get-Date).AddDays(-30)
    Get-ChildItem $BackupDir -Filter "ai-memory-*.tar.gz" |
        Where-Object { $_.LastWriteTime -lt $cutoff } |
        ForEach-Object {
            Remove-Item $_.FullName -Force
            Write-Host "[backup-full] Removeu expirado $($_.Name)" -ForegroundColor DarkGray
        }
} catch {
    Write-Host "[backup-full] ERRO: $_" -ForegroundColor Red
    exit 1
}
