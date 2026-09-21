# 트레이스 (gr-console)

PLC 가 고른 변수를 **매 스캔 사이클** 읽어 청크로 밀어 올리면, gr-console 이 받아 저장하고 화면에 그린다.
S7 폴링(`/api/record`, 최소 20 ms)과 달리 샘플이 PLC 사이클에 붙어 있어 누락이 없고,
누락이 생기면 청크 헤더의 `Overrun` 으로 보고된다.

- 와이어 규격 : `docs/link/wire-spec.md` 6 절 (메시지 40 `TraceCfg`, 41 `Trace`)
- PLC 쪽 블록·부하·안전 규칙 : siemens 레포 `docs/link/trace.md`
- 청크 디코딩 : `crates/plc-link/src/trace.rs` (`TraceLayout`, 오프셋은 계약 UDT 에서 읽는다)

## 경로

```
PLC (SOCK_CFG.Conn[n] : Active, FRAME, BIN, AllowTrace)
   └─ TCP ─→ gr-console [link] bind (기본 0.0.0.0:2000)
                 ├─ apps/gr-console/src/link   LinkSession 이벤트 → 트레이스 저장소
                 └─ apps/gr-console/src/trace  세션 · 파일 · /api/trace/*
```

`plc-link` 테스트 서버와 gr-console 은 **같은 포트를 동시에 쓸 수 없다.** 실기 PLC 를 콘솔에 붙일 때는
테스트 서버를 내리거나 서로 다른 포트를 쓴다.

## 채널
`PEEK` 로 읽는 메모리라서 **표준 접근 DB** 만 고를 수 있다. 계약(`plc/contract/<PLC>`)에서 DB 번호 · 오프셋 ·
타입을 계산해 디스크립터를 만들고, PLC 가 받은 주소를 다시 검증한다.
- 가능한 타입 : Bool, Byte / USInt / SInt / Char, Word / Int / UInt, DWord / DInt / UDInt, Real, Time
- 불가 : LReal, LTime, DTL, String, 그리고 최적화 접근 DB(`DRIVE`, `MACHINE` …) 전체
- 축 위치 · 속도 · 토크 · 랙은 `WEBMON` 복사본에 매 사이클 올라오므로 그쪽을 쓴다

## API

| 메서드 | 경로 | 내용 |
|---|---|---|
| GET | `/api/trace` | 링크 준비 여부, 진행 중 세션, 저장된 세션 목록 |
| GET | `/api/trace/channels?q=&limit=` | 채널 카탈로그 (경로 부분 일치). 멤버가 수만 개라 항상 검색해서 쓴다 |
| POST | `/api/trace/start` | `{plc, robot?, channels[], divider, flush_ms, label, note}` → PLC Ack 까지 기다린다 |
| POST | `/api/trace/stop` / `/api/trace/mark` | 정지 / 현재 시각에 주석 |
| GET | `/api/trace/stream` | SSE `trace` : `chunk` / `mark` / `stopped` |
| GET | `/api/trace/{id}?from&to&max` | 저장된 세션 (행은 `[cycle, t_ms, 값…]`, `max` 는 고정 간격 축약) |
| GET | `/api/trace/{id}/export.csv` | 전체 행 CSV (UTF-8 BOM) |
| DELETE | `/api/trace/{id}` | 삭제 |

시작 실패 응답은 PLC 가 준 이유를 그대로 싣는다 — 110 은 채널 주소가 범위를 벗어났거나 읽을 수 없다는 뜻이고,
111 은 슬롯의 `AllowTrace` 가 꺼져 있거나 프레이밍이 FRAME 이 아니라는 뜻이다.

## 저장

`<data_dir>/traces/<id>/`
- `samples.bin` : 한 행이 `cycle`(u32) + `t_ms`(i32) + 채널마다 원시 워드(u32), 리틀엔디언 고정 길이
- `meta.json` : 채널 정의, 시작 · 종료 시각, PLC 시각, 행 / 청크 / `overrun` / `gaps` 수, 오류, 마크

동시에 한 세션만 돈다(기록 기능과 같은 규칙). 링크가 끊기면 세션은 그 자리에서 닫히고 그 사실이 `errors` 에 남는다.

## 시뮬레이터로 확인

```
cargo run -p plc-link-server --bin plc-link -- simulate --server 127.0.0.1:2000 --plc GR2 \
  --framing frame --format bin --trace-channels 4
```
콘솔을 `--demo` 로 띄우고 위 시뮬레이터를 붙이면 실기 없이 시작 → 청크 수신 → 정지 → CSV 까지 확인할 수 있다.
`--trace-ms <ms>` 를 주면 `TraceCfg` 없이도 스스로 청크를 보낸다(`CfgId` 0 이라 콘솔 세션에는 쌓이지 않는다).
