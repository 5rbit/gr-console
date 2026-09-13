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

# PLC 링크 생성물: siemens export 의 Socket/Gen SCL + LNK_Const_Gen.xml + plc/generated/link/GR2_PLC (docs/link/wire-spec.md)
gen-link:
    cargo run -p gr-contract -- gen-link --plc GR2_PLC --export ../siemens/export --prune

# 생성물이 최신인지만 확인 (아무것도 쓰지 않음, 다르면 exit 1)
gen-link-check:
    cargo run -p gr-contract -- gen-link --plc GR2_PLC --export ../siemens/export --prune --check --verify-sync

# PLC 링크 테스트 서버: PLC 포트 127.0.0.1:2000, 제어 API/화면 http://127.0.0.1:8091/ (docs/link/README.md)
link-serve:
    cargo run -p plc-link-server --bin plc-link -- serve

# 가상 PLC (Active) 를 link-serve 에 연결. 예: just link-sim frame bin
link-sim framing="frame" format="json":
    cargo run -p plc-link-server --bin plc-link -- simulate --server 127.0.0.1:2000 --plc GR2 --framing {{framing}} --format {{format}}

# 인프로세스 서버 × 시뮬레이터 매트릭스 (결과 link-selftest.json)
link-selftest:
    cargo run -p plc-link-server --bin plc-link -- selftest --out link-selftest.json
