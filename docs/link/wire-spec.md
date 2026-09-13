# PLC link wire specification (v1)

Normative contract between the PLC runtime (SCL, siemens repo `export/GR2_PLC/.../700. Communication/Socket`)
and the PC side (`crates/plc-link`, `apps/plc-link-server`, generator `gr-contract gen-link`).
Both sides MUST produce byte-identical output for the same value. Golden vectors are generated from this spec.
Plan with block list and numbers: `C:\Users\hmx2210256\.claude\plans\smooth-wobbling-bubble.md`.

## 1. Transport and framings
One TCP connection per link slot. PLC role Active (connects) or Passive (listens, single peer).
Framing is fixed per slot (PC server sniffs it from the first bytes):

| Framing | First bytes | Message boundary |
|---|---|---|
| FRAME  | `47 53` | 16-byte header + `Length` payload bytes |
| NDJSON | `{` | one JSON envelope per line, terminated by `\n` (a preceding `\r` is stripped; empty lines ignored) |
| HTTP   | `GET ` / `POST` | HTTP/1.1 request/response, body length = `Content-Length` |

### 1.1 FRAME header (16 B, big endian)
| Offset | Size | Field | Value |
|---|---|---|---|
| 0 | 2 | Magic | `0x4753` |
| 2 | 1 | Version | `1` |
| 3 | 1 | Format | `1` JSON, `2` BIN |
| 4 | 2 | MsgType | message id (section 3) |
| 6 | 2 | Seq | section 4 |
| 8 | 4 | LayoutSig | BIN: UDT signature of the payload type; JSON: `0` |
| 12 | 4 | Length | payload byte count. PC max 65535; PLC receive max 8176 |

- Bad magic / version → framing error: send Ack/Error if possible, then close (no resync).
- Length above the receiver maximum → `BAD_LENGTH`: send Ack/Error if possible, then close (both sides; the PLC
  cannot skip an oversized frame).
- FRAME+JSON payload = the envelope (section 2). Header MsgType/Seq are authoritative; if the envelope
  disagrees → `PARSE`.
- FRAME+BIN payload = the Serialize bytes of the message UDT (standard layout, crates/plc-layout rules).
  `Length` must equal the UDT size and `LayoutSig` must equal the UDT signature, else `BAD_LENGTH` / `SIG_MISMATCH`.
  Messages without a payload (Heartbeat): LayoutSig 0, Length 0.

### 1.2 NDJSON
JSON only. A BIN message cannot be carried (`FORMAT_NOT_ALLOWED`). Line max: PC 65535, PLC 8176 bytes.

### 1.3 HTTP/1.1 (subset)
- `Content-Length` is required for every body (request without it but with body → 411). Leading zeros allowed
  (the PLC writes exactly 5 digits, e.g. `Content-Length: 01234`).
- `Transfer-Encoding` present → 501. Header section max 1024 B on the PLC (431), 8 KB on the PC.
- Header names case-insensitive. `Expect: 100-continue` → server sends `HTTP/1.1 100 Continue\r\n\r\n` first.
- Keep-alive by default; `Connection: close` honoured. No pipelining guarantees on the PLC (one request at a time).
- JSON body: `Content-Type: application/json`, body = envelope.
- BIN body: `Content-Type: application/octet-stream` + headers `X-GR-Type: <Name>`, `X-GR-Seq: <u16>`,
  `X-GR-Sig: 0xHHHHHHHH` (8 upper-case hex digits); body = UDT bytes.
- Status line written by both sides: `HTTP/1.1 200 OK`, `204 No Content`, `400 Bad Request`, `404 Not Found`,
  `405 Method Not Allowed`, `409 Conflict`, `411 Length Required`, `413 Payload Too Large`,
  `431 Request Header Fields Too Large`, `501 Not Implemented`, `503 Service Unavailable`.
- Exact header lines written by the PLC (in this order, `\r\n` line ends):
  - Request: `POST <path> HTTP/1.1`, `Host: <a.b.c.d>:<port>`, `Content-Type: ...`, [`X-GR-Type/Seq/Sig`],
    `Content-Length: NNNNN`, blank line. `GET <path> HTTP/1.1`, `Host: ...`, `Accept: application/json` or
    `application/octet-stream`, blank line.
  - Response: `HTTP/1.1 <code> <text>`, [`Content-Type: ...`, `X-GR-*`], `Content-Length: NNNNN`, blank line
    (the PLC also writes `Content-Length: 00000` on 204 / bodyless responses; the PC omits Content-Length on
    1xx/204/304; readers ignore it on those statuses).

Routes:
| PLC role | Method + path | Meaning |
|---|---|---|
| Active (PC is server) | `POST /api/plc/<Plc>/<msg>` | PLC sends `hello`, `heartbeat`, `measlog`, `status`, `commandresult`, `ack`. Response 200 body = Ack envelope (same format as request) |
| Active | `GET /api/plc/<Plc>/command` | 200 + Command, or 204 when nothing is queued |
| Passive (PLC is server) | `POST /api/command` | body Command → 200 CommandResult, 200 Ack(rc) for a rejected command, 400 Ack(read error), 503 Ack(BUSY after CmdWaitMs) |
| Passive | `GET /api/status` | 200 Status |
| Passive | `GET /api/measlog/last` | 200 MeasLog, or 204 if none yet |
| Passive | `GET /api/hello` | 200 Hello |

`<Plc>` = `SOCK_CFG.PlcName` (e.g. `GR2`). `<msg>` = lower-case message name.

## 2. JSON envelope and value text
Envelope, no whitespace anywhere, keys in this order:
`{"type":"<Name>","seq":<u16>,"sig":<u32 decimal>,"data":<value>}`
- `type` = message name (readers also accept the numeric id). `sig` = UDT signature (0 for no payload).
- `data` = the UDT value as a JSON object, or `null` for Heartbeat.

Value text (writers, byte-exact; readers accept any valid JSON unless stated):
| Type | Text |
|---|---|
| UDT / Struct | `{` + `"Member":value` joined by `,` in declaration order + `}` |
| Array[lo..hi] | `[` elements `,` `]`; multi-dim = nested arrays (row-major, first dim outermost) |
| Bool | `true` / `false` |
| Byte, Word, DWord, LWord, USInt, UInt, UDInt, ULInt, SInt, Int, DInt, LInt | decimal integer, `-` for negatives, no leading zeros |
| Char | one-character JSON string (escaping as String) |
| String[n] | JSON string. Escapes: `"`→`\"`, `\`→`\\`, bytes `< 0x20` or `>= 0x7F` → `\u00XX` (upper-case hex). Output is pure ASCII. Reader: `\"` `\\` `\/` `\b` `\f` `\n` `\r` `\t` `\u00XX`; code points > 0xFF or length > n → `PARSE` |
| Time | signed decimal milliseconds |
| DTL | `"YYYY-MM-DDTHH:MM:SS.mmm"` (mmm = NANOSECOND div 1_000_000, zero padded). Reader: `T` or space, 0–9 fraction digits; `""`/`null` → `1970-01-01T00:00:00.000` |
| Real, LReal | see below. Reader: any JSON number; `null` → PC 0.0 (lenient) / PLC `PARSE` |
| Date, TOD, DATE_AND_TIME, LTime, LTOD, LDT | not supported (generator fails with the member path) |

Real/LReal writer algorithm (both sides identical; work in f64, Real converted exactly to f64 first):
1. NaN or ±Inf → `null`.
2. If `|x| < 1e9`: `n = round_half_away_from_zero(x * 10000)` as i64. If `n == 0` → `0.0`.
   Sign = `-` if n < 0. `ip = |n| / 10000`, `fp = |n| % 10000`. Text = sign + `ip` + `.` + `fp` as 4 digits with
   trailing zeros removed but at least one digit (`12.25` → `12.25`, `3` → `3.0`, `0.00004` → `0.0`, `-0.5` → `-0.5`).
3. Else (exponent form): `e` and `10^e` by repeated ×10 from 1.0 while `10^(e+1) <= |x|`
   (`p = 1.0; e = 0; while p * 10.0 <= |x| and e < 400: p = p * 10.0; e = e + 1`), `m = |x| / p`,
   `k = round_half_away_from_zero(m * 10000)`; if `k >= 100000` then `k = k / 10` (integer), `e = e + 1`.
   Text = sign + `k/10000` + `.` + (`k%10000` 4 digits, trailing zeros trimmed, min one digit) + `e` + decimal `e`
   (no plus sign, no padding). Example `1.5e9`, `-2.0e12`. PC and PLC use the same power loop (same `p` on both
   sides). Golden vectors stay below 1e9.

## 3. Messages
| Id | Name | UDT | Direction | Ack |
|---|---|---|---|---|
| 1 | Hello | `LNK_Hello` | both | no (peer answers with its own Hello) |
| 2 | Heartbeat | — | both | no |
| 10 | MeasLog | `LGR_MeasureLog` | PLC→PC | yes |
| 11 | Status | `LNK_GR_Status` | PLC→PC | no |
| 20 | Command | `LGR_Interface_GR_Command` | PC→PLC | reply = CommandResult (or Ack with error) |
| 21 | CommandResult | `LGR_Command_Response` | PLC→PC | yes |
| 30 | Ack | `LNK_Ack` | both | no |

```
TYPE "LNK_Hello"  STRUCT Plc : String[16]; Proto : USInt; Formats : Byte; RegistryHash : DWord; HeartbeatMs : UDInt; END_STRUCT END_TYPE
TYPE "LNK_Ack"    STRUCT RefType : UInt; RefSeq : UInt; Code : Int; Text : String[40]; END_STRUCT END_TYPE
```
`Formats` bit0 JSON, bit1 BIN. `Proto` = 1. `LNK_GR_Status` is defined in the siemens export (types/#Interface_Link).

Error / Ack codes: 0 OK, 1 BAD_MAGIC, 2 BAD_VERSION, 3 UNKNOWN_TYPE, 4 SIG_MISMATCH, 5 BAD_LENGTH, 6 PARSE,
7 FORMAT_NOT_ALLOWED, 8 DIR_NOT_ALLOWED, 9 BUSY, 10 NO_PENDING. PLC runtime-internal codes are >= 100 and may also
appear in `Ack.Code` for rejected commands (100 CMD_DISABLED, 101 HEADER, 102 SRC, 103 DST, 104 UNSUPPORTED, 105 MASKED).

## 4. Sequence numbers, session
- `Seq` u16 per sender, starts at 1, wraps 65535 → 1; 0 = none.
- CommandResult (and an Ack answering a Command) carries the Command's seq in `seq` and in `Ack.RefSeq`.
- Ack for any message: `RefType` = acked message id, `RefSeq` = its seq, own `seq` = sender's next seq.
- The side that opened the TCP connection sends Hello first; the other answers with Hello. HTTP-active PLC POSTs
  hello after each (re)connect.
- Hello check: `RegistryHash` mismatch → PC marks link `contract_mismatch`, refuses BIN, still accepts JSON.
- Heartbeat after `HeartbeatMs` (default 5000) without sending; drop the link after 3 × HeartbeatMs without receiving.

## 5. Signatures and registry hash
- UDT signature = `crc32(canonical_fields(udt))` with the plc-layout canonical form (`signature.rs`), i.e. the same
  value a DB with exactly those fields would get.
- Registry hash = `crc32` of the concatenation, over messages sorted by id, of `"{id}:{name}:{sig:08X}:{size};"`
  (sig/size = 0 for Heartbeat).
