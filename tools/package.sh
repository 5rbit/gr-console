#!/usr/bin/env bash
# 배포 패키지 — 실행 파일 하나(웹·계약 내장) + 설정 + 안내문을 dist/ 에 묶는다.
#
#   tools/package.sh                                   # 현재 OS용
#   tools/package.sh --no-web                          # npm 빌드를 건너뛴다(apps/gr-web/dist 가 이미 최신일 때)
#   tools/package.sh --target x86_64-pc-windows-gnu    # Linux 에서 Windows exe 를 만든다
#                                                      #   (apt install mingw-w64 · rustup target add x86_64-pc-windows-gnu)
#
# 결과: dist/gr-console-<버전>-<target>/ 와 같은 이름의 .zip(또는 .tar.gz).
# 왜 스크립트인가: cargo 만으로는 프런트 빌드와 설정·안내문을 묶을 수 없고, 사람이 손으로 모으면
# 실행 파일과 dist 가 어긋난 채 나간다(그게 이 스크립트가 막는 사고다).
set -euo pipefail
cd "$(dirname "$0")/.."

WEB=1
TARGET=""
while [ $# -gt 0 ]; do
  case "$1" in
    --no-web) WEB=0 ;;
    --target) TARGET="$2"; shift ;;
    --target=*) TARGET="${1#--target=}" ;;
    *) echo "unknown arg: $1" >&2; exit 2 ;;
  esac
  shift
done

if [ "$WEB" = 1 ]; then
  (cd apps/gr-web && npm ci --silent && npm run build)
fi
[ -f apps/gr-web/dist/index.html ] || { echo "apps/gr-web/dist/index.html 이 없다 — 프런트 빌드가 먼저다" >&2; exit 1; }

# 실행 파일: 웹·계약을 내장(embed)한 release. --target 이면 크로스 빌드(산출물은 target/<triple>/release).
if [ -n "$TARGET" ]; then
  cargo build --release -p gr-console --features embed --target "$TARGET"
  BINDIR="target/$TARGET/release"
else
  cargo build --release -p gr-console --features embed
  TARGET=$(rustc -vV | sed -n 's/^host: //p')
  BINDIR="target/release"
fi

VERSION=$(grep -m1 '^version' Cargo.toml | sed -E 's/.*"([^"]+)".*/\1/')
SHA=$(git rev-parse --short HEAD 2>/dev/null || echo nogit)
NAME="gr-console-${VERSION}+${SHA}-${TARGET}"
OUT="dist/${NAME}"
BIN="$BINDIR/gr-console"
[ -f "$BIN.exe" ] && BIN="$BIN.exe"

rm -rf "$OUT"
mkdir -p "$OUT"
cp "$BIN" "$OUT/"
cp tools/package/gr-console.toml "$OUT/gr-console.toml"
cp tools/package/README.txt "$OUT/README.txt"
cat > "$OUT/run-demo.sh" <<'EOF'
#!/usr/bin/env sh
# 장비 없이 화면만 — 가짜 PLC 로 켠다. 이 파일이 있는 폴더가 기준(data/ 가 여기 생긴다).
cd "$(dirname "$0")" && exec ./gr-console --demo
EOF
cat > "$OUT/run.sh" <<'EOF'
#!/usr/bin/env sh
# 실장비로 켠다. 이 파일이 있는 폴더가 기준(gr-console.toml · data/ 가 여기).
cd "$(dirname "$0")" && exec ./gr-console "$@"
EOF
cat > "$OUT/stop.sh" <<'EOF'
#!/usr/bin/env sh
# 실행 중인 콘솔을 안전하게 끈다(진행 중인 PLC 쓰기를 마무리한 뒤 종료).
cd "$(dirname "$0")" && exec ./gr-console --stop
EOF
chmod +x "$OUT/run-demo.sh" "$OUT/run.sh" "$OUT/stop.sh" "$OUT/gr-console"* 2>/dev/null || true
cat > "$OUT/run-demo.cmd" <<'EOF'
@echo off
rem 장비 없이 화면만 — 가짜 PLC 로 켠다. 이 파일이 있는 폴더가 기준(data\ 가 여기 생긴다).
cd /d "%~dp0"
gr-console.exe --demo
pause
EOF
cat > "$OUT/run.cmd" <<'EOF'
@echo off
rem 실장비로 켠다. 이 파일이 있는 폴더가 기준(gr-console.toml · data\ 가 여기).
cd /d "%~dp0"
gr-console.exe %*
if errorlevel 1 pause
EOF
cat > "$OUT/stop.cmd" <<'EOF'
@echo off
rem 실행 중인 콘솔을 안전하게 끈다(진행 중인 PLC 쓰기를 마무리한 뒤 종료).
cd /d "%~dp0"
gr-console.exe --stop
pause
EOF
printf '%s\n%s\n' "$NAME" "$(date -u +%Y-%m-%dT%H:%M:%SZ)" > "$OUT/VERSION"

# 묶기 — zip 이 있으면 zip, 없으면 tar.gz
cd dist
if command -v zip >/dev/null 2>&1; then
  rm -f "${NAME}.zip"; zip -qr "${NAME}.zip" "$NAME"; ART="${NAME}.zip"
else
  rm -f "${NAME}.tar.gz"; tar czf "${NAME}.tar.gz" "$NAME"; ART="${NAME}.tar.gz"
fi
echo "packaged: dist/${ART}"
ls -la "$ART"
