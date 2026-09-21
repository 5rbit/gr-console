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

if (Test-Path $out) { Remove-Item -Recurse -Force $out }
New-Item -ItemType Directory -Path $out | Out-Null
Copy-Item $bin $out
Copy-Item tools/package/gr-console.toml (Join-Path $out 'gr-console.toml')
Copy-Item tools/package/README.txt (Join-Path $out 'README.txt')
@"
@echo off
rem 장비 없이 화면만 — 가짜 PLC 로 켠다. 이 파일이 있는 폴더가 기준(data\ 가 여기 생긴다).
cd /d "%~dp0"
gr-console.exe --demo
pause
"@ | Set-Content -Encoding ASCII (Join-Path $out 'run-demo.cmd')
@"
#!/usr/bin/env sh
cd "`$(dirname "`$0")" && exec ./gr-console --demo
"@ | Set-Content -Encoding ASCII (Join-Path $out 'run-demo.sh')
# 실장비 실행 — 오류로 끝났을 때만 창을 잡아 둔다(이미 실행 중 = 3, 포트 문제 = 4).
@"
@echo off
rem 실장비로 켠다. 이 파일이 있는 폴더가 기준(gr-console.toml · data\ 가 여기).
cd /d "%~dp0"
gr-console.exe %*
if errorlevel 1 pause
"@ | Set-Content -Encoding ASCII (Join-Path $out 'run.cmd')
@"
@echo off
rem 실행 중인 콘솔을 안전하게 끈다(진행 중인 PLC 쓰기를 마무리한 뒤 종료).
cd /d "%~dp0"
gr-console.exe --stop
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
Get-Item $zip | Select-Object Name, Length
