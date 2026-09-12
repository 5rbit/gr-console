# PLC 입출력 벤치

PLCSIM Advanced 라이선스 없이, 콘솔의 PLC 통신 코드를 끝까지 돌려 보는 시험대입니다.
실제 PLC 프로그램 로직은 돌지 않습니다. 대신 콘솔이 쓰는 OPC UA 노드와 S7 바이트를 **PLC 계약 레이아웃에서 그대로 생성**해서, 태그 이름·자료형·오프셋이 실제와 어긋나는 문제를 잡습니다.

## 실행

```
cargo build -p gr-console
target/debug/gr-console --demo-opcua
node tools/bench/plc-io-bench.mjs --out bench-report.json
```

- `--demo-opcua` 는 데모 PLC(가짜 S7 서버 GR2·GRM)에 더해 **가짜 GRM OPC UA 서버**를 띄우고, 명령을 실제 OPC UA 클라이언트 경로로 보냅니다.
- GRM OPC UA 서버의 `"OPCUA".GR[n].CMD` 노드는 `plc/contract/GRM_PLC` 레이아웃에서 만듭니다. Bool 비트 구조, 배열 요소 `Name[i]`, 자료형이 실제 UDT 와 같습니다.
- OPC UA 로 쓴 값은 GRM `OPCUA` DB 바이트에 들어가고, 헤더가 완성되면 GRM 처럼 GR2 로 중계됩니다. 에코 후 CMD 헤더·TaskData·Data 를 지웁니다.
- 벤치는 콘솔이 실기 PLC 에 붙어 있으면 실행을 거부합니다. 태스크를 제출하고 셀·스테이션 테이블을 쓰기 때문입니다.

## 검증 항목

| 구분 | 항목 |
|---|---|
| A. OPC UA | 세션 준비, 탐색한 노드 수 = 레이아웃 `GR[n].CMD` 멤버 수, 경로 일치, Command 비트 구조 노출 |
| B. Task | PICK·DROP(셀), PICK(스테이션), MOVE, MEASURE 제출. 에코 수락, GR2 `STAT.RES.Task` 가 원장 `plc_task` 와 전 필드 일치, 보낸 헤더 에코, GRM CMD 헤더 소거, 완료 |
| B. Task | PLC 에 없는 셀은 거부 코드 429, 대기 중 태스크 취소, 실행 중 태스크 강제 완료 |
| C. Cell/Station | PLC 읽기 = 레지스트리, 로컬 추가분 diff, AUTO 중 쓰기 거부, 강제 쓰기 후 바이트 검증, S7 재읽기로 GR2·GRM 반영 확인, diff 없음 |
| C. Excel | 레지스트리 내보내기 후 가져오기 미리보기에서 오류 0, 행 수 일치 |

2026-09-12 실행 결과는 47 / 47 통과입니다.

## 벤치가 찾은 결함

| 결함 | 실기 영향 | 수정 |
|---|---|---|
| 콘솔이 `Command.Stop` 등 8 개를 바이트로 0 채움 | 실제 UDT 는 Bool 비트 구조라 노드가 없어서 **태스크 제출 전체가 실패** | 0 채움은 탐색한 노드 맵에서만 수행 (`cmd/mod.rs`) |
| OPC UA 상태를 `watch::send` 로 갱신 | 콘솔이 수신자를 버려서 상태가 계속 Disconnected, **게이트가 모든 제출을 막음** | `send_replace` (`opcua-cmd/src/writer.rs`) |
| 노드 탐색 실패 시 쓰는 대체 목록이 옛 바이트 구조 | 탐색 실패 시 합성 노드 검증이 실패 | GR2 버전 비트 구조로 갱신 (`browse.rs`) |
| 데모 PLC 가 매 틱 DB 를 모델로 다시 인코딩 | 데모에서 셀·스테이션 쓰기가 사라짐 (실기 영향 없음) | CELL·STATION 은 S7 쓰기를 모델로 흡수 |

## 이 벤치로 알 수 없는 것

- GR2·GRM PLC 프로그램의 실제 판단: 태스크 검증 규칙, 모드 전환, 스텝 진행, 알람.
- 실제 S7-1500 OPC UA 서버의 탐색 이름·보안 정책. 가짜 서버는 익명·보안 없음입니다.
- 이 부분은 실기 또는 PLCSIM Advanced(라이선스 필요)와 가상 설비가 있어야 확인할 수 있습니다.
