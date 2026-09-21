# plc-link 테스트 서버 (apps/plc-link-server)

PLC 소켓 링크(`docs/link/wire-spec.md`)의 PC 측 테스트 서버, 가상 PLC(시뮬레이터), 셀프 테스트.
gr-console 앱과는 독립된 바이너리 `plc-link` 이다.
트레이스(`TraceCfg` / `Trace`)를 실제로 쓰는 쪽은 gr-console 이다 — `docs/link/trace.md`.

## 구성

```
            PLC 포트 (기본 127.0.0.1:2000)                      제어 API / 화면 (기본 127.0.0.1:8091)
PLC Active ─ FRAME / NDJSON / HTTP 자동 판별 ─┐              ┌─ GET /            PLC 카드, 로그, 명령 패널
                                              ├─ Hub ────────┼─ REST /api/...    상태, 마지막 메시지, 명령, 로그, 변환
PLC Passive ← --connect NAME=ip:port,... ─────┘  (상태·큐·로그) └─ SSE /api/log/stream (event: msg / plc / cmd)
                                                  │
                                                  └─ data/plc-link/log-YYYYMMDD.jsonl
```

- 계약: `plc/contract/<PLC>`(기본 `GR2_PLC`) + `plc/link/messages.toml`. 계약에 `LNK_Hello` / `LNK_Ack` /
  `LNK_GR_Status` 가 없으면 서버에 내장된 사본을 채워 넣는다(`/api/health` 의 `server.fallback_udts`).
  현재 GR2_PLC 는 동기화되어 있어 레지스트리 해시 `0x8E7B313C` (PLC `LNK_REGISTRY_HASH`) 와 같다.
  해시는 `plc/generated/link/<PLC>/registry.json` 의 `registry_hash_hex` 가 정본이다.
- 수신 메시지는 모두 UDT 바이트(표준 레이아웃)로 정규화해서 저장하고, JSON 은 선언 순서로 표시한다.
- 세션 규칙(와이어 사양 4절): 연결을 연 쪽이 Hello 먼저, RegistryHash 불일치 → `contract_mismatch`(BIN 거부 =
  Ack 7, JSON 허용), 송신 없이 HeartbeatMs 경과 → Heartbeat, 3 × HeartbeatMs 무수신 → 끊음, MeasLog /
  CommandResult 는 Ack, 페이로드 오류 → Ack(code) 후 링크 유지, 프레이밍 오류(bad magic/version, 최대 길이 초과)
  → Ack 후 끊음.
- PC → PLC 메시지는 PLC 가 Hello(첫 메시지)에 쓴 포맷으로 보낸다(계약 불일치 시 JSON).
- 소켓을 쥐는 부분(프레이밍 판별, Hello, Heartbeat, 자동 Ack)은 `crates/plc-link/src/io` 의 `LinkSession`
  (cargo feature `io`) 이고, 서버는 그 이벤트/명령 채널을 Hub 에 연결하기만 한다. Hub 없이 같은 세션을 쓰는 앱
  (gr-console) 도 같은 코드를 쓴다.

## 실행

```powershell
cargo run -p plc-link-server --bin plc-link -- serve                     # just link-serve
cargo run -p plc-link-server --bin plc-link -- simulate --server 127.0.0.1:2000 --plc GR2 --framing frame --format bin
                                                                         # just link-sim frame bin
cargo run -p plc-link-server --bin plc-link -- selftest --out link-selftest.json   # just link-selftest
```

브라우저: http://127.0.0.1:8091/

### serve

| 옵션 | 기본 | 설명 |
|---|---|---|
| `--plc-bind` | `127.0.0.1:2000` | PLC 포트. 실기 PLC 는 `0.0.0.0:2000` |
| `--api-bind` | `127.0.0.1:8091` | 제어 API. 인증 없음 → 루프백 유지 |
| `--contract` | `GR2_PLC` | `plc/contract` 아래 이름 또는 디렉터리 |
| `--messages` | `plc/link/messages.toml` | |
| `--connect NAME=ip:port,framing=frame\|ndjson\|http,format=json\|bin,poll_ms=N` | | Passive PLC 클라이언트 (반복 가능). 재접속 1 → 10 s |
| `--log-cap` | 5000 | 로그 링 크기 |
| `--log-dir` / `--no-log-file` | `data/plc-link` | JSONL 파일 (1 s 마다 flush) |
| `--cmd-ttl-s` | 60 | 큐/전송 중 명령 만료 |

Passive PLC:
- FRAME / NDJSON: PC 가 Hello 먼저 보내는 세션.
- HTTP: 접속 후 `GET /api/hello`, 이후 `poll_ms` 마다 `GET /api/status`(내용이 바뀔 때만 로그) 와
  `GET /api/measlog/last`(seq+내용 중복 제거). 명령은 `POST /api/command` 로 보내고 응답
  `200 CommandResult` → done, `200 Ack(rc)` / `400 Ack(읽기 오류)` / `503 Ack(BUSY)` → rejected
  (명령 상태에 `code`, `http_status` 표시).

HTTP-Active PLC 는 요청마다 연결을 유지/재사용할 수 있고, 마지막 요청 후 3 × HeartbeatMs 가 지나면 끊김으로 표시한다.
명령은 항상 큐에 들어가고 PLC 의 `GET /api/plc/<Plc>/command` 폴링으로 전달된다.

### simulate (가상 PLC)

```powershell
# Active PLC (서버에 접속)
plc-link simulate --server 127.0.0.1:2000 --plc GR2 --framing http --format json --measlog-ms 1000
# Passive PLC (PC 가 접속) + 서버에서 --connect
plc-link simulate --listen 127.0.0.1:2101 --plc GR2 --framing frame --format bin
plc-link serve --connect GR2=127.0.0.1:2101,framing=frame,format=bin
```

| 옵션 | 설명 |
|---|---|
| `--measlog-ms` / `--status-ms` / `--poll-ms` | MeasLog(Seq 증가, TimeStamp 현재, Kind 1..5 순환) / Status / HTTP 명령 폴링 주기 |
| `--count N` / `--exit-after-s S` | MeasLog N 건 Ack 후 종료 / S 초 후 종료 |
| `--reject-every N` | N 번째 CommandResult 마다 `Data[0]` = 16#80 (평소 `Data[2]` = 1, Task = TaskData) |
| `--ack-every N --ack-code C` | N 번째 명령마다 Ack(C) 응답 (기본 104 UNSUPPORTED, HTTP Passive = 200 Ack) |
| `--busy-every N` | N 번째 명령마다 Ack(BUSY=9) (HTTP Passive = 503 Ack) |
| `--trace-ms MS` | Trace 청크 주기(ms). 0(기본) = TraceCfg 가 시작한 트레이스만 보내고 주기는 그 `FlushMs` |
| `--trace-channels N` | `--trace-ms` 로 스스로 시작하는 트레이스의 채널 수(1..32, 기본 4). TraceCfg 는 자기 `ChanCount` 를 쓴다 |
| `--fault split-writes\|coalesce\|bad-sig\|bad-magic` | 1–3 바이트 분할 쓰기 / 여러 메시지 한 번에 쓰기 / MeasLog 시그니처 오류 / 첫 연결에서 bad magic |

트레이스(와이어 사양 6절)는 FRAME 프레이밍에서만 동작한다. `TraceCfg`(`Cmd` 1 시작 / 0 정지)를 받으면 한 사이클 뒤
`Ack(0, RefType = TraceCfg, RefSeq = 그 seq)` 로 답하고 `CfgId` 를 그대로 되돌려주는 `Trace` 청크(BIN)를 밀어 올린다.
행은 `[Tick(ms), 채널값 …]` 이고 짝수 채널은 Real 사인파, 홀수 채널은 정수 램프이며 `FirstCycle` 은 `Count × Divider`
만큼 증가한다. `ChanCount` 가 32 를 넘거나 `Divider` 가 0 이면 `Ack(110 TRACE_BAD_CFG)`, FRAME 이 아니거나 계약에
`LNK_Trace` 가 없으면 `Ack(111 TRACE_NOT_ALLOWED)` 로 거부한다.

HTTP 쓰기는 PLC 와 같게 `Content-Length: NNNNN`(5 자리), 204 에도 `Content-Length: 00000` 을 쓴다.
NDJSON × BIN 조합은 거부된다(FORMAT_NOT_ALLOWED).

### selftest

인프로세스로 서버(포트 0)와 시뮬레이터를 띄워 Active / Passive × (frame×json, frame×bin, ndjson×json,
http×json, http×bin) 10 가지와 ndjson×bin 거부를 확인한다. 항목: Hello + 해시, MeasLog 수신(+Ack),
Status 마지막 값(JSON / BIN), `command?wait_ms=5000` 의 CommandResult.Task == 보낸 TaskData 바이트, 로그 전부 OK.
실패하면 exit 1.

## 제어 API

| 메서드 | 경로 | 내용 |
|---|---|---|
| GET | `/api/health` | 계약, 레지스트리 해시, 바인드 주소, fallback UDT |
| GET | `/api/registry` | 메시지(시그니처·크기·JSON 최대), 오류 코드, 라우트, hash_text |
| GET | `/api/schema/{msg}` | UDT 멤버 표(경로, 오프셋, 비트, 타입, 크기) |
| GET | `/api/schema/{msg}/template` | 0 값 JSON (선언 순서) |
| GET | `/api/plcs`, `/api/plcs/{plc}` | 링크 상태, 카운터, 마지막 메시지, 명령 |
| GET | `/api/plcs/{plc}/last/{msg}[?format=bin]` | 마지막 수신 값 (bin → UDT 바이트 + `X-GR-Type/Seq/Sig/Ts`) |
| POST | `/api/plcs/{plc}/command[?wait_ms=N&queue=1]` | 본문 = Command data JSON 또는 엔벨로프. 엄격 검사(400 `{error, code, path}`), 202 `{id, seq, state}`, wait_ms → 200 결과 / 504, 미연결 409 |
| GET | `/api/plcs/{plc}/commands` | queue / inflight / history |
| DELETE | `/api/plcs/{plc}/commands/{id}` | 큐/전송 중 명령 취소 |
| POST | `/api/plcs/{plc}/send/{msg}` | PLC 로 임의 메시지(Hello, Heartbeat, Ack, Command) 전송 |
| POST | `/api/plcs/{plc}/disconnect` | 세션 끊기 (Passive 는 재접속) |
| GET | `/api/log?plc=&msg=&dir=rx\|tx&since_id=&limit=` | 로그 링 조회 |
| GET | `/api/log/stream` | SSE, 이름 붙은 이벤트 `msg`, `plc`, `cmd` (브라우저는 `addEventListener` 필요) |
| POST | `/api/convert` | `{msg, from: "json"\|"bin_hex", value[, strict]}` → `{json_text, envelope, bin_hex, frame_bin_hex, report}` |

### curl 예시 (PowerShell 에서는 `curl.exe`)

```powershell
curl.exe -s http://127.0.0.1:8091/api/health
curl.exe -s http://127.0.0.1:8091/api/plcs
curl.exe -s http://127.0.0.1:8091/api/schema/Command/template -o cmd.json
# cmd.json 편집 후 전송하고 5 초 대기
curl.exe -s -X POST "http://127.0.0.1:8091/api/plcs/GR2/command?wait_ms=5000" -H "Content-Type: application/json" --data-binary "@cmd.json"
curl.exe -s "http://127.0.0.1:8091/api/plcs/GR2/last/MeasLog"
curl.exe -s "http://127.0.0.1:8091/api/plcs/GR2/last/Status?format=bin" -o status.bin
curl.exe -s "http://127.0.0.1:8091/api/log?plc=GR2&msg=MeasLog&limit=20"
curl.exe -N http://127.0.0.1:8091/api/log/stream
curl.exe -s -X POST http://127.0.0.1:8091/api/convert -H "Content-Type: application/json" --data-binary "{\"msg\":\"Status\",\"from\":\"json\",\"value\":{}}"
```

PowerShell 의 `curl` 별칭(Invoke-WebRequest)은 옵션이 다르므로 반드시 `curl.exe` 를 쓴다.

## 실기 PLC 연결

1. PLC 와 같은 서브넷의 PC 주소로 바인드: `plc-link serve --plc-bind 0.0.0.0:2000` (제어 API 는 루프백 유지 권장).
2. Windows 방화벽 인바운드 허용 (관리자 PowerShell):
   ```powershell
   New-NetFirewallRule -DisplayName "plc-link 2000" -Direction Inbound -Protocol TCP -LocalPort 2000 -Action Allow -Profile Domain,Private
   ```
   Passive PLC 로 PC 가 접속하는 경우는 아웃바운드라 보통 규칙이 필요 없다.
3. PLC `SOCK_CFG.Conn[n]`: Active 는 RemoteIP = PC 주소, RemotePort = 2000. Passive 는 LocalPort 를 정하고
   `--connect GR2=<PLC IP>:<LocalPort>,framing=...,format=...`.
4. `/api/plcs` 에서 `registry_ok: true` 를 먼저 확인한다. `false`(contract_mismatch) 면 BIN 은 Ack 7 로 거부되므로
   `gr-contract sync` / `gen-link` 로 양쪽 계약을 맞춘다.
5. 명령은 PLC `SOCK.CmdEnable`(비retain, 기본 FALSE) 과 슬롯의 `AcceptCommand` 가 켜져 있어야 주입된다.

## 제한 사항

- 인증·TLS 없음. 제어 API 를 공장망에 열지 말 것.
- 명령 결과 매칭은 seq 기준(HTTP Passive 는 요청-응답 1:1). 연결이 끊기면 전송 중 명령은 failed, 큐 명령은 TTL 까지 유지.
- PLC 에서 받은 JSON 은 관대하게 읽는다(모르는 키·누락 멤버는 경고, 로그 `warnings`). API 입력만 엄격 검사.
- JSON 메시지의 엔벨로프 `sig` 불일치는 경고만(BIN 만 SIG_MISMATCH Ack). `--fault bad-sig` 는 BIN 에서 Ack 4 를 만든다.
- Hello 전에 들어온 메시지는 PLC 이름 대신 원격 주소로 기록된다.
- HTTP Passive 클라이언트는 Heartbeat / Ack 를 보내지 않는다(라우트 없음). HTTP Active 의 `GET .../command` 204 폴링은 로그에 남기지 않는다.
- JSONL 로그는 날짜별 파일로 계속 쌓이며 자동 삭제하지 않는다.
- 실제 PLC(TCON/TSEND/TRCV) 와의 통신은 이 PC 에서 검증하지 않았다(시뮬레이터 + 인프로세스 테스트만).
