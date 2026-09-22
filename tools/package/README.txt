gr-console — 겐트리 로봇(GR2) 엔지니어링 콘솔
==============================================

이 폴더 하나가 전부입니다. 설치할 것은 없습니다.

  gr-console(.exe)     실행 파일 — 화면(웹)과 PLC 계약이 안에 들어 있습니다
  gr-console.toml      설정 — PLC IP · OPC UA 주소 (실행 파일 옆에 두세요)
  run.cmd / run.sh     실장비로 켜기
  stop.cmd / stop.sh   안전하게 끄기
  run-demo.cmd / .sh   장비 없이 화면만 보기(가짜 PLC)
  data/                처음 켜면 생깁니다 — 원장(SQLite) · 캐시 · 인증서. 백업은 이 폴더 하나.

1. 직접 실행하기
   Windows: run.cmd 더블클릭 (gr-console.exe 를 바로 더블클릭해도 같습니다)
   Linux/macOS: ./run.sh
   기준은 이 폴더입니다 — 설정은 옆의 gr-console.toml, 자료는 data\ 에 쌓입니다.
   콘솔 창에 주소가 찍히고 브라우저가 http://127.0.0.1:8090/ 을 엽니다(안 열리면 직접 입력).
   **한 번에 하나만 실행됩니다.** 두 번째로 켜면 시작하지 않고 "이미 실행 중입니다 — PID …, 주소 …" 를
   알려 준 뒤 실행 중인 콘솔을 브라우저로 열어 줍니다(종료 코드 3). 데모도 같은 규칙입니다 —
   데모와 실장비를 동시에 띄울 수는 없습니다.

2. 끄기 (안전한 순서대로)
   - 콘솔 창에서 Ctrl+C          ← 가장 확실합니다
   - stop.cmd 더블클릭 (= gr-console.exe --stop)  ← 창을 못 찾을 때. 다른 창에서 끕니다
   - 창을 그냥 닫기              ← 최후 수단. Windows 가 몇 초만 주므로 마무리가 짧습니다
   - 작업 관리자에서 끝내기      ← 정말 안 될 때만. 진행 중이던 PLC 쓰기가 끊깁니다
   Ctrl+C · stop.cmd · 창 닫기 모두 같은 절차를 탑니다: 새 명령을 받지 않고 → 시나리오를 멈추고
   (이미 PLC 에 보낸 Task 는 그대로) → 진행 중인 PLC 쓰기를 마무리하고 → 기록을 저장하고 →
   OPC UA · S7 연결을 닫고 → 원장을 정리한 뒤 끝납니다. 창에 진행이 한 줄씩 찍힙니다.
   원장은 data/gr-console.db 에 남습니다.

3. 장비 없이 켜 보기
   Windows: run-demo.cmd 더블클릭     Linux/macOS: ./run-demo.sh

4. 실장비에 붙이기
   gr-console.toml 을 열어 [[plcs]] 의 host(GR2 · GRM 의 IP)와 [opcua] endpoint 를 맞춥니다.
   왼쪽 PLC 패널에서 두 PLC 가 초록(연결됨 · 레이아웃 OK)이면 됩니다. 붉으면 IP/방화벽/계약 버전을 보세요.

5. 다른 PC에서 열기
   gr-console.toml 의 bind 를 "0.0.0.0:8090" 으로 바꾸고, 이 PC 방화벽에서 8090/TCP 를 허용합니다.
   그 PC 브라우저에서 http://<이 PC IP>:8090/ 을 엽니다.
   끄기는 콘솔이 떠 있는 PC 에서만 됩니다(--stop 은 같은 PC · 같은 사용자 계정에서만 받습니다).

6. 옮기기 · 백업 · 새 버전으로 바꾸기 (포터블)
   이 폴더를 통째로 복사·이동하면 설정과 자료가 함께 갑니다(경로는 모두 이 폴더 기준).
   옮기기 전에 콘솔을 끄세요(stop.cmd) — 켜진 채 복사하면 최근 기록이 빠질 수 있습니다.
   - 백업:  gr-console --backup
            → backup\gr-console-backup-<시각>\ 에 설정 · DB(일관 사본) · 인증서 · 측정 기록 · 트레이스.
            켜져 있어도 됩니다(실행 중인 콘솔이 만듭니다). 복원은 그 폴더 내용을 이 폴더에 복사.
   - 새 버전: 새 폴더를 풀고, 옛 콘솔을 끈 뒤 새 폴더에서
            gr-console --import-from <옛 폴더>      (이미 data 가 있으면 --force, 기존 DB 는 data\backup 으로)
            처음 켤 때 DB 를 data\backup 에 백업한 뒤 필요한 변환을 합니다.
   - 옛 실행 파일로 새 DB 를 열면 시작을 거부합니다 — 새 실행 파일을 쓰거나 data\backup 의 백업으로 되돌리세요.
   - 데모(run-demo)는 data-demo\ 를 따로 씁니다 — 실데이터와 섞이지 않습니다.
   - 로그: data\logs\ (30 일 지난 것은 자동 정리).

옵션
   gr-console --demo                가짜 PLC 로 실행
   gr-console --stop                실행 중인 콘솔을 안전하게 끄기 (--stop --force 는 강제 종료)
   gr-console --backup              설정 · 자료 백업(위 6)
   gr-console --import-from <폴더>  옛 콘솔 폴더에서 설정 · 자료 가져오기(위 6)
   gr-console --bind 0.0.0.0:8090   주소 덮어쓰기
   gr-console --config <파일>       다른 설정 파일
   gr-console --example-config      기본 설정을 출력
   gr-console --allow-multi         개발용 — 단일 실행 제한을 건너뜀(현장에서는 쓰지 마세요)
   환경 변수 GR_CONSOLE_GR2_HOST · GR_CONSOLE_GRM_HOST · GR_CONSOLE_OPCUA_ENDPOINT · GR_CONSOLE_ADDR 도 됩니다.

종료 코드: 0 정상 · 1 오류 · 3 이미 실행 중 · 4 포트를 잡지 못함(다른 프로그램이 8090 을 쓰는 중).

문제가 생기면
   콘솔 창의 로그가 먼저입니다(RUST_LOG=debug 로 자세히). 화면의 PLC 패널 → 행 클릭 → 재검사/재연결.
   "이미 실행 중" 인데 창이 안 보이면: stop.cmd 로 끄거나, 작업 관리자에서 gr-console.exe 를 확인하세요.
