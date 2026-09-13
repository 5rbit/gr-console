# 배포 패키지(Windows) — 실행 파일 하나(웹·계약 내장) + 설정 + 안내문을 dist\ 에 묶는다.
#
#   pwsh tools/package.ps1            # 또는 `just package`
#   pwsh tools/package.ps1 -NoWeb     # npm 빌드를 건너뛴다(apps/gr-web/dist 가 이미 최신일 때)
#
# 결과: dist\gr-console-<버전>+<sha>-<target>\ 와 같은 이름의 .zip. tools/package.sh 와 같은 일이다.
param([switch]$NoWeb)
$ErrorActionPreference = 'Stop'
Set-Location (Join-Path $PSScriptRoot '..')

if (-not $NoWeb) {
  Push-Location apps/gr-web
  npm ci --silent; if ($LASTEXITCODE) { throw 'npm ci failed' }
  npm run build;   if ($LASTEXITCODE) { throw 'npm run build failed' }
  Pop-Location
}
if (-not (Test-Path apps/gr-web/dist/index.html)) { throw 'apps/gr-web/dist/index.html 이 없다 — 프런트 빌드가 먼저다' }

cargo build --release -p gr-console --features embed
if ($LASTEXITCODE) { throw 'cargo build failed' }

$version = (Select-String -Path Cargo.toml -Pattern '^version\s*=\s*"([^"]+)"' | Select-Object -First 1).Matches[0].Groups[1].Value
$sha = (git rev-parse --short HEAD 2>$null); if (-not $sha) { $sha = 'nogit' }
$target = ((rustc -vV) | Select-String '^host: ').Line -replace '^host: ', ''
$name = "gr-console-$version+$sha-$target"
$out = Join-Path 'dist' $name
$bin = if (Test-Path target/release/gr-console.exe) { 'target/release/gr-console.exe' } else { 'target/release/gr-console' }

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
"$name`n$((Get-Date).ToUniversalTime().ToString('yyyy-MM-ddTHH:mm:ssZ'))" | Set-Content (Join-Path $out 'VERSION')

$zip = Join-Path 'dist' "$name.zip"
if (Test-Path $zip) { Remove-Item -Force $zip }
Compress-Archive -Path $out -DestinationPath $zip
Write-Host "packaged: $zip"
Get-Item $zip | Select-Object Name, Length
