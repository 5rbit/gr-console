# 배포 패키지(Windows) — 실행 파일 하나(웹·계약 내장) + 설정 + 안내문을 dist\ 에 묶는다.
#
#   pwsh tools/package.ps1                                   # 또는 `just package`
#   pwsh tools/package.ps1 -NoWeb                            # npm 빌드를 건너뛴다(apps/gr-web/dist 가 이미 최신일 때)
#   pwsh tools/package.ps1 -Target x86_64-pc-windows-gnu     # 크로스 빌드(rustup target add … 가 먼저)
#
# 결과: dist\gr-console-<버전>+<sha>-<target>\ 와 같은 이름의 .zip. tools/package.sh 와 같은 일이다.
param([switch]$NoWeb, [string]$Target = '')
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

if (-not $NoWeb) {
  Push-Location apps/gr-web
  npm ci --silent; if ($LASTEXITCODE) { throw 'npm ci failed' }
  npm run build;   if ($LASTEXITCODE) { throw 'npm run build failed' }
  Pop-Location
}
if (-not (Test-Path apps/gr-web/dist/index.html)) { throw 'apps/gr-web/dist/index.html 이 없다 — 프런트 빌드가 먼저다' }

if ($Target) {
  cargo build --release -p gr-console --features embed --target $Target
  $binDir = "target/$Target/release"
} else {
  cargo build --release -p gr-console --features embed
  $Target = ((rustc -vV) | Select-String '^host: ').Line -replace '^host: ', ''
  $binDir = 'target/release'
}
if ($LASTEXITCODE) { throw 'cargo build failed' }

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$sha = (git rev-parse --short HEAD 2>$null); if (-not $sha) { $sha = 'nogit' }
$name = "gr-console-$version+$sha-$Target"
$out = Join-Path 'dist' $name
$bin = if (Test-Path "$binDir/gr-console.exe") { "$binDir/gr-console.exe" } else { "$binDir/gr-console" }

# 같은 이름 폴더가 이미 있으면(같은 커밋으로 다시 빌드) 운용 자료를 지우지 않는다 — data/ 와 운용 중인
# gr-console.toml 을 옆으로 옮겨 두었다가, zip 을 만든 **뒤에** 되돌린다(zip 에는 자료가 들어가지 않는다).
# 실행 중이면 data 를 옮기지 못해 여기서 멈춘다(콘솔을 먼저 끌 것).
$keep = $null
if (Test-Path $out) {
  if ((Test-Path (Join-Path $out 'data')) -or (Test-Path (Join-Path $out 'gr-console.toml'))) {
    $keep = "$out.keep-$((Get-Date).ToString('yyyyMMddHHmmss'))"
    New-Item -ItemType Directory -Path $keep | Out-Null
    if (Test-Path (Join-Path $out 'data')) { Move-Item (Join-Path $out 'data') (Join-Path $keep 'data') }
    if (Test-Path (Join-Path $out 'gr-console.toml')) { Move-Item (Join-Path $out 'gr-console.toml') (Join-Path $keep 'gr-console.toml') }
    Write-Host "operator data kept aside: $keep"
  }
  Remove-Item -Recurse -Force $out
}
New-Item -ItemType Directory -Path $out | Out-Null
Copy-Item $bin $out
Copy-Item tools/package/gr-console.toml (Join-Path $out 'gr-console.toml')
Copy-Item tools/package/README.txt (Join-Path $out 'README.txt')
@"
@echo off
rem Demo mode - fake PLC, no equipment needed.
chcp 65001 >nul
cd /d "%~dp0"
"%~dp0gr-console.exe" --demo
pause
"@ | Set-Content -Encoding ASCII (Join-Path $out 'run-demo.cmd')
@"
#!/usr/bin/env sh
cd "`$(dirname "`$0")" && exec ./gr-console --demo
"@ | Set-Content -Encoding ASCII (Join-Path $out 'run-demo.sh')
# 실장비 실행 — 오류로 끝났을 때만 창을 잡아 둔다(이미 실행 중 = 3, 포트 문제 = 4).
@"
@echo off
rem Run the console (real PLCs). Config and data live next to this file.
chcp 65001 >nul
cd /d "%~dp0"
"%~dp0gr-console.exe" %*
if errorlevel 1 pause
"@ | Set-Content -Encoding ASCII (Join-Path $out 'run.cmd')
@"
@echo off
rem Stop the running console gracefully.
chcp 65001 >nul
cd /d "%~dp0"
"%~dp0gr-console.exe" --stop
pause
"@ | Set-Content -Encoding ASCII (Join-Path $out 'stop.cmd')
@"
#!/usr/bin/env sh
cd "`$(dirname "`$0")" && exec ./gr-console "`$@"
"@ | Set-Content -Encoding ASCII (Join-Path $out 'run.sh')
@"
#!/usr/bin/env sh
cd "`$(dirname "`$0")" && exec ./gr-console --stop
"@ | Set-Content -Encoding ASCII (Join-Path $out 'stop.sh')
"$name`n$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))" | Set-Content (Join-Path $out 'VERSION')

$zip = Join-Path 'dist' "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $out -DestinationPath $zip
Write-Host "packaged: $zip"
if ($keep) {
  if (Test-Path (Join-Path $keep 'data')) { Move-Item (Join-Path $keep 'data') (Join-Path $out 'data') }
  if (Test-Path (Join-Path $keep 'gr-console.toml')) { Move-Item -Force (Join-Path $keep 'gr-console.toml') (Join-Path $out 'gr-console.toml') }
  Remove-Item -Recurse -Force $keep
  Write-Host "operator data restored into: $out"
}
Get-Item $zip | Select-Object Name, Length
