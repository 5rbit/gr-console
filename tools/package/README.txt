gr-console — 겐트리 로봇(GR2) 엔지니어링 콘솔
==============================================

이 폴더 하나가 전부입니다. 설치할 것은 없습니다.

  gr-console(.exe)     실행 파일 — 화면(웹)과 PLC 계약이 안에 들어 있습니다
  gr-console.toml      설정 — PLC IP · OPC UA 주소 (실행 파일 옆에 두세요)
  run-demo.cmd / .sh   장비 없이 화면만 보기(가짜 PLC)
  data/                처음 켜면 생깁니다 — 원장(SQLite) · 캐시 · 인증서. 백업은 이 폴더 하나.

1. 바로 켜 보기 (장비 없이)
   Windows: run-demo.cmd 더블클릭     Linux/macOS: ./run-demo.sh
   브라우저가 http://127.0.0.1:8090/ 을 엽니다(안 열리면 직접 입력).

2. 실장비에 붙이기
   gr-console.toml 을 열어 [[plcs]] 의 host(GR2 · GRM 의 IP)와 [opcua] endpoint 를 맞춥니다.
   gr-console(.exe) 을 실행합니다(더블클릭 또는 터미널). 콘솔 창에 주소가 찍히고 브라우저가 열립니다.
   왼쪽 PLC 패널에서 두 PLC 가 초록(연결됨 · 레이아웃 OK)이면 됩니다. 붉으면 IP/방화벽/계약 버전을 보세요.

3. 다른 PC에서 열기
   gr-console.toml 의 bind 를 "0.0.0.0:8090" 으로 바꾸고, 이 PC 방화벽에서 8090/TCP 를 허용합니다.
   그 PC 브라우저에서 http://<이 PC IP>:8090/ 을 엽니다.

4. 끄기
   콘솔 창에서 Ctrl+C (또는 창 닫기). 원장은 data/gr-console.db 에 남습니다.

옵션
   gr-console --demo                가짜 PLC 로 실행
   gr-console --bind 0.0.0.0:8090   주소 덮어쓰기
   gr-console --config <파일>       다른 설정 파일
   gr-console --example-config      기본 설정을 출력
   환경 변수 GR_CONSOLE_GR2_HOST · GR_CONSOLE_GRM_HOST · GR_CONSOLE_OPCUA_ENDPOINT · GR_CONSOLE_ADDR 도 됩니다.

문제가 생기면
   콘솔 창의 로그가 먼저입니다(RUST_LOG=debug 로 자세히). 화면의 PLC 패널 → 행 클릭 → 재검사/재연결.
