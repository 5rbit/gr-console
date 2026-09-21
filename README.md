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
├─ run.cmd/.sh        실장비로 켜기 (오류로 끝나면 창을 잡아 둔다)
├─ stop.cmd/.sh       안전하게 끄기 (= `gr-console --stop`)
├─ run-demo.cmd/.sh   장비 없이 화면만(가짜 PLC)
├─ README.txt         현장용 한 장 안내
└─ data/              처음 켜면 생긴다 — 원장(SQLite) · 캐시 · 인증서 · 풀어 놓은 계약
```

### 직접 실행하기

1. **패키지 폴더에서** — `run.cmd`를 더블클릭한다(`gr-console.exe`를 바로 더블클릭해도 같다).
   설정은 옆의 `gr-console.toml`, 자료는 `data\`, 브라우저가 `http://127.0.0.1:8090/`을 연다
   (`open_browser = true`). Linux/macOS는 `./run.sh`.
2. **끄기** — 창에서 Ctrl+C, 또는 `stop.cmd`(= `gr-console --stop`, 다른 창에서). 창을 그냥 닫는 것은
   최후 수단이다(아래 "안전하게 끄기").
3. **데모** — `run-demo.cmd` / `./run-demo.sh`. 가짜 PLC가 Task를 돌린다.
4. **소스에서(개발자)** — `cd apps/gr-web && npm ci && npm run build` 뒤 `cargo run -p gr-console`
   (데모는 `cargo run -p gr-console -- --demo`). 저장소 루트의 `gr-console.toml`·`plc/contract`·
   `apps/gr-web/dist`를 읽는다. 프런트를 다른 곳에 빌드했으면 `GR_CONSOLE_WEB_DIR=<dist 경로>`.
   `just dev` 같은 레시피도 있지만 `just`가 깔려 있지 않을 수 있다 — 위의 `cargo run`이면 충분하다.
5. **한 번에 하나만** — 기계 전체에서 인스턴스 하나다(데모든 실장비든). 두 번째로 켜면 시작하지 않고
   "이미 실행 중입니다 — PID …, 주소 …"를 찍은 뒤 실행 중인 콘솔을 브라우저로 열고 **종료 코드 3**으로
   끝난다. 잠금은 `%ProgramData%\gr-console\gr-console.lock`(그 밖의 OS는 temp)에 건 OS 파일 잠금이고,
   PID·주소·data 폴더·종료 토큰은 사용자별 `%LOCALAPPDATA%\gr-console\instance.json`에 남는다(정상 종료
   때 지워진다). 개발·테스트용 예외는 `--allow-multi` 또는 `GR_CONSOLE_ALLOW_MULTI=1`(이때는 `--stop`으로
   끌 수 없다). 시험용으로 잠금 자리를 옮기려면 `GR_CONSOLE_LOCK_DIR`(안내는 `GR_CONSOLE_INFO_DIR`) —
   그 폴더 안에서는 그대로 하나만 뜬다. 포트가 이미 쓰이면 원시 OS 오류 대신 포트와 `[server] bind`를 짚어 주고 **종료 코드 4**.
6. **실장비** — `gr-console.toml`에서 `[[plcs]]`의 `host`(GR2·GRM IP)와 `[opcua] endpoint`를 맞춘다.
   왼쪽 PLC 패널의 두 PLC가 초록(연결됨 · 레이아웃 OK)이면 된다.
7. **다른 PC에서 열기** — `bind = "0.0.0.0:8090"`, 방화벽 8090/TCP 허용, `http://<이 PC IP>:8090/`.
   끄기는 콘솔이 떠 있는 PC에서만 된다.
8. **백업** — `data/` 폴더 하나(`gr-console.db`가 원장). 실행 파일은 언제든 새것으로 바꿔도 된다.

### 안전하게 끄기

Ctrl+C · `stop.cmd`(`--stop`) · 창 닫기가 **같은 절차 하나**를 탄다:

1. 새 쓰기·명령 요청은 503 "종료 중"(읽기는 계속 된다),
2. 시나리오는 다음 스텝을 내지 않는다 — 이미 PLC에 보낸 Task는 취소하지 않고 그대로 둔다,
3. 진행 중인 PLC 쓰기(Task 제출/취소/강제 완료, CELL·STATION 표 쓰기, 레이저 비트)를 **끝까지 기다린다**
   (최대 15초, 넘기면 무엇이 남았는지 로그에 남긴다),
4. 기록 중이면 정상 정지 경로로 끝 스냅샷과 파일을 마무리하고,
5. OPC UA 세션·S7 연결을 닫고 SQLite WAL을 체크포인트한 뒤,
6. 안내 파일을 지우고 잠금을 풀고 끝낸다. 진행은 창에 한 줄씩 찍힌다.

`--stop`은 실행 중인 콘솔의 루프백 전용 엔드포인트(`POST /api/admin/shutdown`, 안내 파일의 토큰 필요)를
불러 이 절차를 시작하고 프로세스가 끝날 때까지 기다린다 — 웹 화면에는 종료 버튼이 **없다**(네트워크로
노출되지 않게). 안 끝나면 `gr-console --stop --force`가 강제 종료한다(진행 중이던 PLC 쓰기가 끊긴다).
**창 닫기**는 Windows가 몇 초(≈5초)만 주므로 짧은 예산(4초)으로 같은 절차를 도는 최선 노력이다 —
확실한 방법은 Ctrl+C · `stop.cmd` · `--stop`이다.

실행 파일은 **설정 파일이 있는 폴더**를 기준으로 `data/`를 만든다(CWD가 아니라). 더블클릭·바로 가기·
서비스 어디서 켜도 같은 자리에 쌓인다. 옵션: `--demo` · `--stop [--force]` · `--bind 0.0.0.0:8090` ·
`--config <파일>` · `--example-config` · `--allow-multi`, 환경 변수 `GR_CONSOLE_GR2_HOST` ·
`GR_CONSOLE_GRM_HOST` · `GR_CONSOLE_OPCUA_ENDPOINT` · `GR_CONSOLE_ADDR` · `GR_CONSOLE_DATA_DIR` ·
`GR_CONSOLE_ALLOW_MULTI`. 로그는 콘솔 창(`RUST_LOG=debug`로 자세히).
종료 코드: `0` 정상 · `1` 오류 · `3` 이미 실행 중 · `4` 포트를 잡지 못함.

## 패키지 만들기

```
tools/package.sh              # Linux/macOS — dist/gr-console-<버전>+<sha>-<target>.zip
pwsh tools/package.ps1        # Windows      (또는 `just package`)
        --no-web / -NoWeb     # apps/gr-web/dist 가 이미 최신이면 npm 빌드 생략
        --target <triple> / -Target <triple>   # 크로스 빌드 — 아래 "Linux에서 Windows exe"
```

**Linux에서 Windows exe 만들기** (CI·컨테이너에서 Windows 패키지를 낼 때):

```
sudo apt install mingw-w64
rustup target add x86_64-pc-windows-gnu
tools/package.sh --target x86_64-pc-windows-gnu     # → dist/gr-console-…-x86_64-pc-windows-gnu.zip
```

SQLite(번들 C 소스)와 링크가 mingw로 되고, OPC UA 클라이언트는 순수 Rust라 다른 네이티브 의존이 없다.
이렇게 만든 exe를 Wine 9에서 빈 폴더에 두고 `--demo`로 켜 계약 추출·화면·API까지 확인했다
(`base=Z:\…`, 실행 파일 옆 기준). Windows 네이티브 툴체인(`x86_64-pc-windows-msvc`)으로 만들어도 된다.

스크립트가 하는 일: `npm ci && npm run build` → `cargo build --release -p gr-console --features embed`
→ 실행 파일 + `tools/package/gr-console.toml` + `tools/package/README.txt` + 실행·종료·데모 스크립트
(`run`·`stop`·`run-demo`의 `.cmd`/`.sh`)를
`dist/`에 모아 zip. **`embed` feature**가 `apps/gr-web/dist`와 `plc/contract`를 `include_dir`로 실행
파일에 넣는다 — 개발 빌드(feature 없음)는 디스크를 읽으므로 프런트를 안 만들어도 컴파일된다.
Windows용 exe는 Windows에서 직접, 또는 위처럼 Linux에서 크로스 빌드로 만든다. 실장비 계약이 바뀌면
`plc/contract`를 갱신하고 다시 패키징한다 — 실행 파일과 계약은 항상 한 몸이다.

필요한 도구: Rust 1.89+(C 컴파일러 — SQLite가 번들로 빌드된다), Node 20+, git.

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
docs/              DESIGN.md(UI 규칙) · ui-ux-plan.md(결정 기록) · item-spec-z.md(화물 규격·집는 높이·눌림양)
                   · station-offset.md · palletizing.md · screenshots/
```
