# gr-console

겐트리 로봇(GR2) 엔지니어링 테스트 콘솔. Rust(axum) 백엔드 하나가 PLC를 읽고(S7comm), 명령을 보내고
(OPC UA → GRM `"OPCUA".GR[2].CMD`, GCS와 같은 경로), 웹 화면(React)을 서빙한다.

- 읽기/쓰기: S7comm 직접(표준 접근 DB). 레이아웃은 `plc/contract`의 TIA 소스에서 생성하고 접속 시 검증한다.
- 화면 넷: 작업 명령 · Task 관리 · 측정 모니터 · 시나리오. 창은 도킹 워크스페이스로 자유롭게 배치한다.
- 디자인·UX 규칙은 `docs/DESIGN.md`(진실원), 결정 기록은 `docs/ui-ux-plan.md`.

## 바로 쓰기 (배포 패키지)

배포는 **실행 파일 하나**다. 웹 화면과 PLC 계약(`plc/contract`)이 실행 파일 안에 들어 있어 설치할 것이
없고, 어디에 풀어도 된다.

```
gr-console-<버전>+<sha>-<target>/
├─ gr-console(.exe)   실행 파일 — 화면·계약 내장
├─ gr-console.toml    설정 — PLC IP · OPC UA 주소 · 포트 (실행 파일 옆에 둔다)
├─ run-demo.cmd/.sh   장비 없이 화면만(가짜 PLC)
├─ README.txt         현장용 한 장 안내
└─ data/              처음 켜면 생긴다 — 원장(SQLite) · 캐시 · 인증서 · 풀어 놓은 계약
```

1. **장비 없이 켜 보기** — `run-demo.cmd`(Windows) 또는 `./run-demo.sh`. 브라우저가
   `http://127.0.0.1:8090/`을 연다(`open_browser = true`). 가짜 PLC가 Task를 돌린다.
2. **실장비** — `gr-console.toml`에서 `[[plcs]]`의 `host`(GR2·GRM IP)와 `[opcua] endpoint`를 맞추고
   `gr-console(.exe)`을 실행한다. 왼쪽 PLC 패널의 두 PLC가 초록(연결됨 · 레이아웃 OK)이면 된다.
3. **다른 PC에서 열기** — `bind = "0.0.0.0:8090"`, 방화벽 8090/TCP 허용, `http://<이 PC IP>:8090/`.
4. **백업** — `data/` 폴더 하나(`gr-console.db`가 원장). 실행 파일은 언제든 새것으로 바꿔도 된다.

실행 파일은 **설정 파일이 있는 폴더**를 기준으로 `data/`를 만든다(CWD가 아니라). 더블클릭·바로 가기·
서비스 어디서 켜도 같은 자리에 쌓인다. 옵션: `--demo` · `--bind 0.0.0.0:8090` · `--config <파일>` ·
`--example-config`, 환경 변수 `GR_CONSOLE_GR2_HOST` · `GR_CONSOLE_GRM_HOST` · `GR_CONSOLE_OPCUA_ENDPOINT` ·
`GR_CONSOLE_ADDR` · `GR_CONSOLE_DATA_DIR`. 로그는 콘솔 창(`RUST_LOG=debug`로 자세히).

## 패키지 만들기

```
tools/package.sh              # Linux/macOS — dist/gr-console-<버전>+<sha>-<target>.zip
pwsh tools/package.ps1        # Windows      (또는 `just package`)
        --no-web / -NoWeb     # apps/gr-web/dist 가 이미 최신이면 npm 빌드 생략
```

스크립트가 하는 일: `npm ci && npm run build` → `cargo build --release -p gr-console --features embed`
→ 실행 파일 + `tools/package/gr-console.toml` + `tools/package/README.txt` + 데모 실행 스크립트를
`dist/`에 모아 zip. **`embed` feature**가 `apps/gr-web/dist`와 `plc/contract`를 `include_dir`로 실행
파일에 넣는다 — 개발 빌드(feature 없음)는 디스크를 읽으므로 프런트를 안 만들어도 컴파일된다.
Windows용 exe는 Windows에서(또는 `x86_64-pc-windows-msvc` 타깃으로) 만든다. 실장비 계약이 바뀌면
`plc/contract`를 갱신하고 다시 패키징한다 — 실행 파일과 계약은 항상 한 몸이다.

필요한 도구: Rust 1.87+(C 컴파일러 — SQLite가 번들로 빌드된다), Node 20+, git.

## 개발

```
just                      # 레시피 목록 (justfile은 PowerShell 셸)
cargo check --workspace · cargo clippy --workspace · cargo fmt
cargo run -p gr-console -- --demo      # 백엔드만(화면은 5173의 vite dev가 /api 를 프록시)

cd apps/gr-web
npm install
npm run dev               # http://localhost:5173 — /api 는 GR_BACKEND(기본 127.0.0.1:8090)로 프록시
npm run check             # tsc(app + node) + 디자인 린트(docs/DESIGN.md 의 예산 — 기준선 0)
TZ=Asia/Seoul npm run test:unit
npm run build             # dist/ — 백엔드가 apps/gr-web/dist 를 찾아 서빙한다
```

개발 체크아웃에서 `cargo run`은 CWD(저장소 루트)의 `gr-console.toml`·`plc/contract`·`apps/gr-web/dist`를
읽는다. 에이전트 안내는 `CLAUDE.md`, 프런트 규약·소유 규칙은 `apps/gr-web/README.md`.

## 구조

```
apps/gr-console/   백엔드(axum) — PLC S7 읽기/쓰기 · OPC UA 명령 · 원장(SQLite) · SSE · 웹 서빙
apps/gr-web/       프런트(React 19 + Vite + Tailwind v4)
crates/            공유 크레이트(s7 · plc-layout · gr-proto · opcua-cmd)
tools/gr-contract/ TIA 소스 → PLC 레이아웃 생성기
tools/package.*    배포 패키지 스크립트 (+ tools/package/ 설정 템플릿·안내문)
plc/contract/      TIA 소스(진실원) — 패키지에 내장된다
docs/              DESIGN.md(UI 규칙) · ui-ux-plan.md(결정 기록) · screenshots/
```
