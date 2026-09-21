# opcua-cmd

S7-1500 OPC UA 서버(GRM_PLC, `opc.tcp://192.168.1.10:4840`)의 글로벌 DB `"OPCUA"` 안에 있는
명령 구조체 `"OPCUA"."GR"[2]."CMD"`(LGR_Interface_GR_Command)의 **리프 멤버**를 쓰는 클라이언트.
구조체 통째 쓰기는 가정하지 않는다. async-opcua 0.19 기반.

```rust
let (writer, mut state) = opcua_cmd::CmdWriter::spawn(cfg);      // tokio 런타임 안에서
state.wait_for(|s| matches!(s, OpcState::Ready { .. })).await?;
let hdr = writer.write_task(&members, cmd_code, src, dst, protocol).await?;
```

## OpcUaConfig

| 필드 | 기본값 | 설명 |
|---|---|---|
| `endpoint` | `opc.tcp://192.168.1.10:4840` | Discovery/세션 URL. GetEndpoints 결과의 호스트는 이 값으로 치환됨 |
| `security_policy` | `None` | `None` \| `Basic256Sha256` \| `Aes128_Sha256_RsaOaep` \| `Aes256_Sha256_RsaPss` (대소문자·`-`·`_` 무시) |
| `security_mode` | `None` | `None` \| `Sign` \| `SignAndEncrypt`. 정책 None ↔ 모드 None 이어야 함 |
| `auth` | `Anonymous` | `Auth::Anonymous` 또는 `Auth::UserPass { user, pass }` |
| `ns_hint` | `3` | 브라우즈 실패 시 문자열 NodeId 합성에 쓰는 네임스페이스 인덱스 |
| `db_name` | `OPCUA` | 글로벌 DB 이름 (`Objects/<PLC>/DataBlocksGlobal/<DB>` 에서 BrowseName 으로 찾음) |
| `root_path` | `GR[2].CMD` | DB 아래 명령 구조체까지의 경로. 배열 인덱스는 대괄호 |
| `connect_timeout_ms` | 5000 | GetEndpoints + CreateSession/ActivateSession, 브라우즈 호출 타임아웃 |
| `write_timeout_ms` | 3000 | Read/Write 서비스 타임아웃 (`OpcError::Timeout`) |
| `session_timeout_ms` | 30000 | 요청 세션 타임아웃. `keepalive_interval_ms` × 3 보다 작으면 그 값으로 올려 요청 (서버가 더 줄일 수 있음) |
| `channel_lifetime_ms` | 60000 | 요청 보안 채널 토큰 수명. 라이브러리가 수명의 75 % 가 지난 뒤 첫 요청 때 갱신 |
| `keepalive_interval_ms` | 5000 | 세션 상태 점검 주기 — 라이브러리 keep-alive 가 `Server_ServerStatus_State`(i=2259) 를 읽음 (타임아웃 = `write_timeout_ms`) |
| `keepalive_fail_limit` | 2 | keep-alive 연속 시간 초과 몇 번이면 세션이 죽은 것으로 보나 |
| `node_cache` | `None` | 노드맵 JSON 캐시 경로. 연결마다 로드 → `Header.Protocol` Read 로 검증 → 실패 시 재브라우즈 |
| `pki_dir` | `<temp>/gr-console-opcua-pki` | 클라이언트 인증서 저장소. 정책이 None 이 아니면 `own/cert.der`, `private/private.pem` 자동 생성 |
| `trust_server_cert` | `true` | 서버 인증서를 `pki_dir/trusted` 없이 신뢰 |
| `array_bases` | 비어 있음 | 배열 선언 하한 (`TaskData.Position` → 1, 인덱스 없는 루트 기준 경로). S7-1500 서버는 기본형 배열을 0 기준(`Position[0]..[3]`)으로, 구조체 배열은 PLC 인덱스(`GR[2]`)로 노출 → 브라우즈·캐시 직후 0 이 있는 배열만 `P[k]` → `P[k+lb]` 로 바꿔 PLC 인덱스로 조회. gr-console 은 GRM 계약에서 만든다 |

`OpcUaConfig`/`Auth` 는 `serde` 로 읽을 수 있고(`#[serde(default)]`), `Auth` 는 `{"kind":"anonymous"}` /
`{"kind":"user_pass","user":..,"pass":..}` 형태. `Debug` 출력에서 비밀번호는 가려진다.

## 동작

1. **연결**: GetEndpoints → 모든 엔드포인트를 `tracing::info` 로 기록(`CmdWriter::endpoints()` 로도 조회) →
   설정한 정책/모드와 일치하는 엔드포인트 선택 → 사용자 토큰 종류 확인 → CreateSession/ActivateSession.
2. **노드맵**: 캐시 검증 → 브라우즈 → 문자열 NodeId 합성 순으로 시도. 결과는 `node_cache` 에 저장.
   - 브라우즈: `ObjectsFolder` 에서 깊이 ≤ 4 BFS 로 BrowseName == `db_name` 인 노드를 찾고(ns=0 하위는 건너뜀),
     `root_path` 세그먼트를 따라 내려간 뒤(배열 요소 BrowseName 은 `GR[2]`/`[2]`/`2` 모두 허용)
     루트 하위 전체를 브라우즈해 자식이 없는 Variable 을 리프로 기록. 경로는 `A.B[2].C` 로 정규화.
   - 각 리프의 `DataType` 속성을 한 번 읽어 종류(Byte/UInt/UDInt/Int/Real/Bool/String…)를 저장.
3. **쓰기**: `PlcValue` 를 멤버 종류에 맞는 `Variant` 로 강제 변환(폭 넓히기·정수→실수·0/1→Bool 허용,
   범위 초과나 타입 불일치는 `OpcError::Config`). 하나의 WriteRequest 로 전송, 노드 순서 유지.
   서버가 Bad 상태를 돌려주면 `OpcError::Status { path, code }` (예: `BadTypeMismatch` = `0x80740000`).
4. **재연결**: 연결 끊김/실패 시 1s → 2s → … → 30s 백오프. 상태는 watch 채널로
   `Disconnected → Connecting → Browsing → Ready{node_count, ns}` / `Failed{error, retry_in_ms}`.
   라이브러리 안의 재연결은 끄고(`session_retry_limit(0)`) 이벤트 루프(`SessionEventLoop::enter`)를 직접 돌린다.
5. **죽은 세션**: 서버가 채널은 둔 채 세션만 없애면(PLC 다운로드·서버 재시작·세션 만료) 채널이 살아 있어
   이벤트 루프가 끝나지 않는다 — async-opcua 는 keep-alive 실패를 로그로만 남긴다(`max_failed_keep_alive_count` 기본 0).
   그래서 keep-alive 결과를 직접 본다:
   - `BadSessionIdInvalid` / `BadSessionClosed` / `BadSessionNotActivated` / `BadSecureChannelIdInvalid` /
     `BadSecureChannelClosed` / `BadConnectionClosed` / `BadNotConnected` → 즉시,
   - 시간 초과(`BadTimeout`) `keepalive_fail_limit` 번 연속 → 세션 죽음.
   - Read/Write 요청이 위 코드로 실패해도 같다(점검 주기를 기다리지 않음). 그 요청은
     `OpcError::Transport("세션 무효 (…) — 다시 연결 (요청 실패)")` 로 실패한다.

   죽음을 보면 활성 세션을 바로 거두고(`write_*` → `NotReady`, 게이트 막힘) 상태를
   `Failed { error: "세션 무효 (BadSessionIdInvalid) — 다시 연결", retry_in_ms: 1000 }` 로 바꾼 뒤, 옛 이벤트 루프를
   버리고(전송 닫힘 → 라이브러리 반복 로그 멈춤) 최소 백오프(1 s)로 다시 연결한다. 로그는 경고 한 줄
   `session invalid, reconnecting` (그 앞에 라이브러리의 첫 실패 로그 2~3 줄).

### `write_task` 순서
1. `task_members` + 모든 `Command.*` 리프 0 + `Data[0..15]` 0 을 **한 요청**으로 (호출자가 준 멤버가 우선).
2. `Header.{Protocol, CMD_ID, CMD, SRC, DST, SEQ}` 를 **두 번째 요청**으로. `cmd_id` 1..=255, `seq` 1..=65535 는
   0을 건너뛰며 순환하고 시작값은 현재 시각에서 유도(프로세스 재시작 후 같은 헤더 반복 방지). 내부 뮤텍스로 직렬화.

## 시도하는 NodeId 형태

| 단계 | 형태 | 비고 |
|---|---|---|
| 브라우즈 | 서버가 돌려준 NodeId 그대로 | S7-1500: `ns=3;s="OPCUA"."GR"[2]."CMD"."Header"."CMD"` |
| 캐시 | `node_cache` JSON 의 `members[path]` | `Header.Protocol` Read 성공 시에만 신뢰 |
| 합성 A | `ns=<ns_hint>;s="OPCUA"."GR"[2]."CMD"."TaskData"."Position"[3]` | 인덱스를 따옴표 밖에 (S7 기본) |
| 합성 B | `ns=<ns_hint>;s="OPCUA"."GR[2]"."CMD"."TaskData"."Position[3]"` | 인덱스를 따옴표 안에 |

합성 시에는 `"…"."Header"."Protocol"` Read 가 성공하는 형태를 고르고, 알려진 멤버 목록(`browse::KNOWN_MEMBERS`)
중 Value Read 가 성공하는 것만 맵에 넣는다. 어떤 형태가 쓰였는지는 캐시 JSON 의 `source`
(`browse` / `cache` / `synth:quoted-index` / `synth:index-in-quotes`) 로 알 수 있다.

## 진단

**`Failed { error: "... policy X / mode Y not offered by ...; offered: [...] " }`**
PLC 가 해당 보안 정책을 켜지 않았다. `offered` 목록(`url policy=… mode=… tokens=[…] level=…`)에서
PLC 가 제공하는 조합을 골라 `security_policy`/`security_mode` 를 맞추거나, TIA Portal
(PLC 속성 → OPC UA → Server → Security → Secure channel) 에서 정책을 활성화한다.
`... does not offer a UserName user token` 이면 TIA 의 OPC UA "User authentication" 에서
사용자 이름/비밀번호 인증을 켜야 한다(또는 `Auth::Anonymous` 사용).

**`BadUserAccessDenied` / `BadIdentityTokenRejected` / `BadIdentityTokenInvalid`**
`Failed { error: "BadUserAccessDenied: server rejected the user identity token (...)" }` 로 표시된다.
- 사용자/비밀번호가 PLC 의 OPC UA 사용자 관리에 없거나 틀림.
- 정책 `None` 에서 비밀번호 토큰을 보내면 S7-1500 은 거부할 수 있음(암호화 정책 필요).
- 쓰기 자체가 `OpcError::Status { code: 0x801F0000 }` 로 실패하면 세션은 살아 있으나
  해당 사용자에게 DB 쓰기 권한이 없거나 TIA 의 OPC UA "Server interface" 에서 멤버가 쓰기 불가로 설정된 것.
  `Status { code: 0x803B0000 }`(BadNotWritable)도 같은 원인. `OpcError::status_name(code)` 로 이름 확인.

**인증서 오류** (`BadCertificateUntrusted`, `BadSecurityChecksFailed`)
PLC 가 클라이언트 인증서(`pki_dir/own/cert.der`)를 신뢰해야 한다(TIA: Certificate manager 에 가져오기 후
OPC UA server 의 trusted clients 에 추가). 서버 인증서를 검증하려면 `trust_server_cert=false` 로 두고
`pki_dir/trusted/` 에 서버 인증서를 넣는다.

**`data block "OPCUA" not found` / `array element [2] ... not found`**
TIA 의 OPC UA server interface 에 DB 가 노출되지 않았거나 "Export array members" 가 꺼져 있다.
이때는 문자열 NodeId 합성으로 넘어가며, 그것도 실패하면 `Failed` 에 두 원인이 함께 기록된다.

## 테스트

```
cargo test -p opcua-cmd
cargo clippy -p opcua-cmd --all-targets -- -D warnings
```

`tests/fake_server.rs` 는 async-opcua 의 서버(`server` feature)로 S7 유사 주소공간
(`Objects/PLC/DataBlocksGlobal/OPCUA/GR/GR[2]/CMD/...`)을 127.0.0.1 임의 포트에 띄워 쓰기 순서,
헤더 카운터, 타입 강제 변환, 오류 매핑, 캐시, 서버 재시작 후 복구, 정책 미제공 진단을 검증한다.
서버의 `max_session_timeout_ms` 를 짧게 두어 채널은 둔 채 세션이 만료되게 하고, keep-alive 로 감지하는 경우와
쓰기 요청으로 감지하는 경우 모두 `Ready → Failed(세션 무효) → Ready` 가 되는지도 본다.
