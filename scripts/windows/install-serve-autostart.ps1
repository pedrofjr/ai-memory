# Registra a tarefa agendada "ai-memory-serve" para iniciar o servidor HTTP + Web UI
# automaticamente ao fazer logon no Windows e ja coloca o servidor no ar imediatamente.
# Reexecutar re-registra a tarefa (idempotente).
$ErrorActionPreference = "Stop"

$TaskName = "ai-memory-serve"
$ServeScript = Join-Path $PSScriptRoot "serve.ps1"

# O Agendador de Tarefas nao consegue iniciar os aliases MSIX de
# %LOCALAPPDATA%\Microsoft\WindowsApps (sao stubs/reparse points): o CreateProcess
# falha com 0x80070002 ERROR_FILE_NOT_FOUND e a tarefa nunca sobe.
# Por isso resolvemos o pwsh.exe real dentro do pacote e, na falta dele,
# caímos para o Windows PowerShell 5.1 (sempre presente em System32).
$pwshPath = (Get-Command pwsh.exe -ErrorAction SilentlyContinue).Source
if ($pwshPath -and $pwshPath -like '*\Microsoft\WindowsApps\pwsh.exe') {
    $packaged = Join-Path $env:LOCALAPPDATA 'Microsoft\WindowsApps\Microsoft.PowerShell_8wekyb3d8bbwe\pwsh.exe'
    if (Test-Path $packaged) { $pwshPath = $packaged }
}
if (-not $pwshPath -or -not (Test-Path $pwshPath)) {
    $pwshPath = Join-Path $env:SystemRoot 'System32\WindowsPowerShell\v1.0\powershell.exe'
}
Write-Host "[..] Interpretador da tarefa: $pwshPath" -ForegroundColor DarkGray

Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue

$action = New-ScheduledTaskAction `
    -Execute $pwshPath `
    -Argument "-NoProfile -NonInteractive -WindowStyle Hidden -ExecutionPolicy Bypass -File `"$ServeScript`" -Web" `
    -WorkingDirectory (Split-Path -Parent (Split-Path -Parent $PSScriptRoot))

$trigger = New-ScheduledTaskTrigger -AtLogOn -User $env:USERNAME

$settings = New-ScheduledTaskSettingsSet `
    -AllowStartIfOnBatteries `
    -DontStopIfGoingOnBatteries `
    -StartWhenAvailable `
    -ExecutionTimeLimit ([TimeSpan]::Zero) `
    -MultipleInstances IgnoreNew `
    -RestartCount 3 `
    -RestartInterval (New-TimeSpan -Minutes 1)

Register-ScheduledTask `
    -TaskName $TaskName `
    -Action $action `
    -Trigger $trigger `
    -Settings $settings `
    -Description "Inicia o servidor HTTP + MCP + Web UI do ai-memory (127.0.0.1:49374) automaticamente ao fazer logon." `
    -RunLevel Limited `
    -Force | Out-Null

Write-Host "[OK] Tarefa agendada '$TaskName' registrada com sucesso (Gatilho: Ao fazer logon)." -ForegroundColor Green
Start-ScheduledTask -TaskName $TaskName
Write-Host "[OK] Tarefa '$TaskName' disparada!" -ForegroundColor Green
