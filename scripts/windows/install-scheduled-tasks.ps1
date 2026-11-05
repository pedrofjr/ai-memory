# Cria as tarefas agendadas no Task Scheduler para backup do ai-memory.
# Push do wiki: 10:00, 12:00, 16:00, 18:30 (dias uteis)
# Backup full:  19:00 (dias uteis)
$ErrorActionPreference = "Stop"
$ScriptsDir = $PSScriptRoot

function New-WikiPushTask {
    $name = "ai-memory-push-wiki"
    $script = Join-Path $ScriptsDir "backup-push-wiki.ps1"

    # Remove se ja existir
    Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue

    $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$script`""

    $weekdays = @([System.DayOfWeek]::Monday,[System.DayOfWeek]::Tuesday,
                   [System.DayOfWeek]::Wednesday,[System.DayOfWeek]::Thursday,
                   [System.DayOfWeek]::Friday)

    $triggers = @("10:00","12:00","16:00","18:30") | ForEach-Object {
        New-ScheduledTaskTrigger -Weekly -WeeksInterval 1 -DaysOfWeek $weekdays -At $_
    }

    $settings = New-ScheduledTaskSettingsSet `
        -AllowStartIfOnBatteries `
        -DontStopIfGoingOnBatteries `
        -StartWhenAvailable `
        -MultipleInstances IgnoreNew

    Register-ScheduledTask -TaskName $name -Action $action -Trigger $triggers -Settings $settings -RunLevel Limited -Force | Out-Null
    Write-Host "  OK $name" -ForegroundColor Green
}

function New-BackupFullTask {
    $name = "ai-memory-backup-full"
    $script = Join-Path $ScriptsDir "backup-full.ps1"

    Unregister-ScheduledTask -TaskName $name -Confirm:$false -ErrorAction SilentlyContinue

    $action = New-ScheduledTaskAction -Execute "powershell.exe" -Argument "-NoProfile -ExecutionPolicy Bypass -File `"$script`""

    $weekdays = @([System.DayOfWeek]::Monday,[System.DayOfWeek]::Tuesday,
                   [System.DayOfWeek]::Wednesday,[System.DayOfWeek]::Thursday,
                   [System.DayOfWeek]::Friday)

    $trigger = New-ScheduledTaskTrigger -Weekly -WeeksInterval 1 -DaysOfWeek $weekdays -At "19:00"

    $settings = New-ScheduledTaskSettingsSet `
        -AllowStartIfOnBatteries `
        -DontStopIfGoingOnBatteries `
        -StartWhenAvailable `
        -MultipleInstances IgnoreNew

    Register-ScheduledTask -TaskName $name -Action $action -Trigger $trigger -Settings $settings -RunLevel Limited -Force | Out-Null
    Write-Host "  OK $name" -ForegroundColor Green
}

Write-Host "Instalando tarefas agendadas..." -ForegroundColor Cyan
New-WikiPushTask
New-BackupFullTask
Write-Host "Concluido. As tarefas rodam segunda a sexta (dias uteis)." -ForegroundColor Cyan
Write-Host "  - ai-memory-push-wiki:   10:00, 12:00, 16:00, 18:30" -ForegroundColor White
Write-Host "  - ai-memory-backup-full: 19:00" -ForegroundColor White
Write-Host "Backups em: $env:USERPROFILE\ai-memory-backups\" -ForegroundColor DarkGray
