# Purrdr 主機外掛(Windows,best-effort):herdr 在 pane.agent_status_changed 時以新程序執行這個腳本。
# herdr 的 plugin 在 Windows 仍是 preview,這個腳本沒有在 Windows 主機上實測。
#
# 行為與 notify.sh 相同:Agent 進入 blocked 時，對 %USERPROFILE%\.purrdr\push-*\pairing 的每一支配對裝置
# 呼叫 Purrdr 後台的 /v1/push,只帶 workspace 名稱、狀態與 pane id,不帶任何對話內容;後台回 410 就刪掉該配對。
# 檔案帶 UTF-8 BOM:Windows PowerShell 5.1 讀沒有 BOM 的檔案會用系統字碼頁，中文註解會讓整支腳本解析失敗。
$ErrorActionPreference = 'Stop'

try {
    $event = $env:HERDR_PLUGIN_EVENT_JSON | ConvertFrom-Json
} catch {
    exit 0
}
if ($event.data.agent_status -ne 'blocked') { exit 0 }

$pane = [string]$event.data.pane_id
if ($pane -notmatch '^[A-Za-z0-9:_.-]{1,64}$') { $pane = $null }

$workspace = $null
try {
    $context = $env:HERDR_PLUGIN_CONTEXT_JSON | ConvertFrom-Json
    $workspace = [string]$context.workspace_label
} catch {}
if ([string]::IsNullOrEmpty($workspace)) { $workspace = [string]$event.data.workspace_id }
if ([string]::IsNullOrEmpty($workspace)) { exit 0 }

$herdr = if ($env:HERDR_BIN_PATH) { $env:HERDR_BIN_PATH } else { 'herdr' }
$session = $null
$asked = $false

$root = Join-Path $env:USERPROFILE '.purrdr'
if (-not (Test-Path -LiteralPath $root)) { exit 0 }

foreach ($directory in Get-ChildItem -LiteralPath $root -Directory -Filter 'push-*') {
    $file = Join-Path $directory.FullName 'pairing'
    if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { continue }
    $lines = Get-Content -LiteralPath $file
    $key = ($lines | Where-Object { $_ -like 'key=*' } | Select-Object -First 1) -replace '^key=', ''
    $endpoint = ($lines | Where-Object { $_ -like 'endpoint=*' } | Select-Object -First 1) -replace '^endpoint=', ''
    if (-not $key -or -not $endpoint) { continue }

    $muted = @($lines | Where-Object { $_ -like 'muted=*' } | ForEach-Object { $_.Substring(6) })
    if ($muted.Count -gt 0) {
        if (-not $asked -and $pane) {
            $asked = $true
            try {
                $info = & $herdr pane get $pane 2>$null | ConvertFrom-Json
                $session = [string]$info.result.pane.agent_session.value
            } catch {}
        }
        if ($session -and ($muted -contains $session)) { continue }
    }

    $body = [ordered]@{ key = $key; workspace = $workspace; status = 'blocked' }
    if ($pane) { $body.pane = $pane }
    try {
        Invoke-RestMethod -Method Post -Uri $endpoint -ContentType 'application/json' -TimeoutSec 10 `
            -Body ([System.Text.Encoding]::UTF8.GetBytes(($body | ConvertTo-Json -Compress))) | Out-Null
    } catch {
        # 410:裝置已被遠端關閉(配對撤銷),刪掉配對之後不再呼叫後台。其他失敗不重試，下一次「需要你」會再送。
        $response = $_.Exception.Response
        if ($response -and [int]$response.StatusCode -eq 410) {
            Remove-Item -LiteralPath $directory.FullName -Recurse -Force -ErrorAction SilentlyContinue
        }
    }
}
