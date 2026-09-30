# gr-console live swap - the ONLY command Claude is allowed to run on the live console.
# Install: copy this file to C:\gr-console\deploy.ps1 (the siemens workspace .claude/settings.local.json allows exactly
#   powershell -NoProfile -ExecutionPolicy Bypass -File C:\gr-console\deploy.ps1 ...).
#
#   powershell -NoProfile -ExecutionPolicy Bypass -File C:\gr-console\deploy.ps1 -Exe <new gr-console.exe> -Label "<sha text>"
#   powershell -NoProfile -ExecutionPolicy Bypass -File C:\gr-console\deploy.ps1 -Rollback
#
# Refuses while a robot has live tasks (submitted/accepted/queued/running) unless -Force.
# Never kills a process: graceful --stop only, aborts if 8090 does not go down.
# Keeps the previous exe as gr-console.prev.exe (-Rollback swaps it back).
# ASCII only: Windows PowerShell 5.1 reads a BOM-less script as ANSI.
param(
  [string]$Exe,
  [string]$Label = '',
  [switch]$Rollback,
  [switch]$Force
)
$ErrorActionPreference = 'Stop'
$dir = 'C:\gr-console'
$live = Join-Path $dir 'gr-console.exe'
$prev = Join-Path $dir 'gr-console.prev.exe'
$base = 'http://127.0.0.1:8090'

function Get-Json([string]$path) {
  $r = Invoke-WebRequest -UseBasicParsing -TimeoutSec 5 "$base$path"
  [Text.Encoding]::UTF8.GetString($r.RawContentStream.ToArray()) | ConvertFrom-Json
}
function Test-Up { [bool](Get-NetTCPConnection -LocalPort 8090 -State Listen -ErrorAction SilentlyContinue) }

if ($Rollback) {
  if (-not (Test-Path $prev)) { throw "no $prev to roll back to" }
  $src = Join-Path $dir 'gr-console.rollback-src.exe'
  Copy-Item $prev $src -Force
  $Exe = $src
  if (-not $Label) { $Label = 'rollback to gr-console.prev.exe' }
} elseif (-not $Exe -or -not (Test-Path $Exe)) {
  throw "new exe not found: '$Exe'"
}

if ((Get-FileHash $Exe).Hash -eq (Get-FileHash $live).Hash) {
  Write-Output 'same exe is already live - nothing to do'
  exit 0
}

# 1. refuse while robots are working
if (Test-Up) {
  $robots = Get-Json '/api/robots'
  $busy = @($robots | Where-Object { $_.active_tasks -gt 0 })
  foreach ($r in $robots) { Write-Output ("robot {0}: active_tasks={1}" -f $r.name, $r.active_tasks) }
  if ($busy.Count -and -not $Force) {
    throw ('live tasks on ' + (($busy | ForEach-Object { $_.name }) -join ',') + ' - refusing (use -Force only if the user said so)')
  }

  # 2. backup + graceful stop
  Write-Output 'backup'
  & $live --backup
  Write-Output 'stop'
  & $live --stop
  $down = $false
  foreach ($i in 1..40) { if (-not (Test-Up)) { $down = $true; break }; Start-Sleep -Seconds 1 }
  if (-not $down) { throw '8090 still listening after --stop - aborted, nothing swapped' }
} else {
  Write-Output '8090 is not running - swapping without stop'
}
foreach ($i in 1..20) {
  if (-not (Get-Process -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $live })) { break }
  Start-Sleep -Seconds 1
}
$wal = Get-Item (Join-Path $dir 'data\gr-console.db-wal') -ErrorAction SilentlyContinue
Write-Output ('WAL bytes: ' + $(if ($wal) { $wal.Length } else { 'none' }))

# 3. swap
if (-not $Rollback) { Copy-Item $live $prev -Force }
$ok = $false
foreach ($i in 1..15) {
  try { Copy-Item $Exe $live -Force; $ok = $true; break } catch { Start-Sleep -Seconds 1 }
}
if (-not $ok) { throw "could not replace $live (locked)" }
Set-Content (Join-Path $dir 'VERSION') ("$Label " + (Get-Date).ToString('s')) -Encoding utf8
Write-Output "swapped: $Label"

# 4. start detached (explorer, so closing a shell does not stop it) and verify
Start-Process explorer.exe (Join-Path $dir 'run.cmd')
$up = $false
foreach ($i in 1..60) { if (Test-Up) { $up = $true; break }; Start-Sleep -Seconds 1 }
if (-not $up) { throw '8090 did not come up in 60 s - check the console window; roll back with -Rollback' }
Get-NetTCPConnection -LocalPort 8090 -State Listen | ForEach-Object {
  $p = Get-Process -Id $_.OwningProcess
  Write-Output ("8090 PID {0} {1} {2}" -f $p.Id, $p.Path, $p.StartTime)
}
Start-Sleep -Seconds 8
try {
  foreach ($p in (Get-Json '/api/plcs')) {
    Write-Output ("plc {0,-10} connected={1} layout_ok={2} {3}" -f $p.id, $p.connected, $p.layout.ok, $p.layout.detail)
  }
} catch { Write-Output "plcs check failed: $($_.Exception.Message)" }
