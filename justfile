set shell := ["powershell", "-NoProfile", "-Command"]

check:
    cargo fmt --all -- --check; cargo clippy --workspace --all-targets -- -D warnings

test:
    cargo test --workspace

verify: check test

web-install:
    cd apps/gr-web; npm install

web-dev:
    cd apps/gr-web; npm run dev

web-check:
    cd apps/gr-web; npm run check; npm run test:unit

web-build:
    cd apps/gr-web; npm run build

dev:
    cargo run -p gr-console -- --demo

# 명령 경로를 OPC UA 로: 계약 레이아웃에서 만든 가짜 GRM OPC UA 서버 + 가짜 S7 PLC (docs/bench/README.md)
dev-opcua:
    cargo run -p gr-console -- --demo-opcua

# dev-opcua 로 띄운 콘솔에 PLC 입출력 벤치 실행
bench:
    node tools/bench/plc-io-bench.mjs --out bench-report.json

console: web-build
    $env:GR_CONSOLE_WEB_DIR = "$PWD/apps/gr-web/dist"; cargo run -p gr-console
