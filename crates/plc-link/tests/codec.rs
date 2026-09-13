mod common;

use std::sync::Arc;

use common::*;
use plc_link::codec::next_seq;
use plc_link::framing::{ContentLength, FrameDecoder, Framing, HttpRequestDecoder, NdjsonDecoder, RawFrame, encode_frame, write_request};
use plc_link::vectors::golden_bytes;
use plc_link::{Codec, ErrCode, Format, Message, Role, Strictness};
use serde_json::json;

fn one_frame(bytes: &[u8]) -> RawFrame {
    let mut d = FrameDecoder::new(65535);
    d.feed(bytes);
    d.next().unwrap().unwrap()
}

fn one_request(bytes: &[u8]) -> RawFrame {
    let mut d = HttpRequestDecoder::pc();
    d.feed(bytes);
    d.next().unwrap().unwrap()
}

fn ndjson(text: &str) -> RawFrame {
    let mut d = NdjsonDecoder::new(65535);
    d.feed(text.as_bytes());
    d.feed(b"\n");
    d.next().unwrap().unwrap()
}

fn fixture_msg(codec: &Codec) -> Message {
    Message { id: 40, seq: 7, payload: golden_bytes(codec.contract(), "LNK_Fixture").unwrap() }
}

fn cmd_msg(codec: &Codec) -> Message {
    Message { id: 41, seq: 9, payload: golden_bytes(codec.contract(), "LNK_FixtureSub").unwrap() }
}

#[test]
fn round_trips_over_every_framing() {
    let codec = fixture_codec();
    let fx = fixture_msg(&codec);
    let cmd = cmd_msg(&codec);
    for format in [Format::Json, Format::Bin] {
        let f = one_frame(&codec.to_frame(&fx, format).unwrap());
        assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Strict).unwrap(), fx, "frame {format:?}");
        let r = one_request(&codec.to_http_request(&fx, format, "POST", "/api/plc/GR2/fixture", "h").unwrap());
        assert_eq!(codec.decode_raw(&r, Role::Pc, Strictness::Strict).unwrap(), fx, "http {format:?}");
        // PC → PLC direction
        let f = one_frame(&codec.to_frame(&cmd, format).unwrap());
        assert_eq!(codec.decode_raw(&f, Role::Plc, Strictness::Strict).unwrap(), cmd);
    }
    let n = ndjson(&codec.json_text(&fx).unwrap());
    assert_eq!(codec.decode_raw(&n, Role::Pc, Strictness::Strict).unwrap(), fx);
    let (m, report) = codec.decode_raw_report(&n, Role::Pc, Strictness::Strict).unwrap();
    assert_eq!(m, fx);
    assert!(report.is_clean());
}

#[test]
fn envelope_text_is_exact() {
    let codec = fixture_codec();
    let ack = codec.ack(40, 7, -3, "bad \"x\"", 3).unwrap();
    let sig = codec.registry().by_name("Ack").unwrap().sig;
    assert_eq!(codec.json_text(&ack).unwrap(), format!(r#"{{"type":"Ack","seq":3,"sig":{sig},"data":{{"RefType":40,"RefSeq":7,"Code":-3,"Text":"bad \"x\""}}}}"#));
    let hb = codec.heartbeat(5).unwrap();
    assert_eq!(codec.json_text(&hb).unwrap(), r#"{"type":"Heartbeat","seq":5,"sig":0,"data":null}"#);
    assert_eq!(codec.data_json_text(&hb).unwrap(), "null");
    let hello = codec.hello("GR2", 3, 1).unwrap();
    let v = codec.data_value(&hello).unwrap();
    assert_eq!(v, json!({"Plc": "GR2", "Proto": 1, "Formats": 3, "RegistryHash": codec.registry().hash(), "HeartbeatMs": 5000}));
    assert_eq!(plc_link::write_envelope("X", 0, 0, None), r#"{"type":"X","seq":0,"sig":0,"data":null}"#);
}

#[test]
fn signature_and_length_checks() {
    let codec = fixture_codec();
    let fx = fixture_msg(&codec);
    let sig = codec.registry().by_id(40).unwrap().sig;
    let bad_sig = one_frame(&encode_frame(Format::Bin, 40, 7, sig ^ 1, &fx.payload));
    assert_eq!(codec.decode_raw(&bad_sig, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::SigMismatch);
    let short = one_frame(&encode_frame(Format::Bin, 40, 7, sig, &fx.payload[..169]));
    assert_eq!(codec.decode_raw(&short, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::BadLength);
    let wrong = Message { payload: fx.payload[..100].to_vec(), ..fx.clone() };
    assert_eq!(codec.to_frame(&wrong, Format::Bin).unwrap_err().code, ErrCode::BadLength);
    assert_eq!(codec.json_text(&wrong).unwrap_err().code, ErrCode::BadLength);

    // JSON: envelope sig 0 = unknown (accepted), a different non-zero sig is rejected in strict mode
    let text = codec.json_text(&fx).unwrap();
    let zero = text.replacen(&format!("\"sig\":{sig}"), "\"sig\":0", 1);
    assert_eq!(codec.decode_raw(&ndjson(&zero), Role::Pc, Strictness::Strict).unwrap(), fx);
    let other = text.replacen(&format!("\"sig\":{sig}"), "\"sig\":12345", 1);
    assert_eq!(codec.decode_raw(&ndjson(&other), Role::Pc, Strictness::Strict).unwrap_err().code, ErrCode::SigMismatch);
    let (m, r) = codec.decode_raw_report(&ndjson(&other), Role::Pc, Strictness::Lenient).unwrap();
    assert_eq!((m, r.warnings.len()), (fx, 1));

    // HTTP BIN: type from the route when X-GR-Type is missing; X-GR-Sig required
    let body = golden_bytes(codec.contract(), "LNK_Fixture").unwrap();
    let sig_hdr = format!("0x{sig:08X}");
    let req = write_request("POST", "/api/plc/GR2/fixture", "h", &[("Content-Type", "application/octet-stream"), ("X-GR-Seq", "3"), ("X-GR-Sig", &sig_hdr)], Some(&body), ContentLength::Pad5);
    let m = codec.decode_raw(&one_request(&req), Role::Pc, Strictness::Strict).unwrap();
    assert_eq!((m.id, m.seq), (40, 3));
    let req = write_request("POST", "/api/plc/GR2/fixture", "h", &[("Content-Type", "application/octet-stream")], Some(&body), ContentLength::Plain);
    assert_eq!(codec.decode_raw(&one_request(&req), Role::Pc, Strictness::Strict).unwrap_err().code, ErrCode::SigMismatch);
}

#[test]
fn unknown_and_unavailable_types() {
    let codec = fixture_codec();
    let f = one_frame(&encode_frame(Format::Bin, 99, 1, 0, &[]));
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::UnknownType);
    let n = ndjson(r#"{"type":"Nope","seq":1,"sig":0,"data":null}"#);
    assert_eq!(codec.decode_raw(&n, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::UnknownType);
    let n = ndjson(r#"{"type":"Missing","seq":1,"sig":0,"data":{}}"#);
    assert_eq!(codec.decode_raw(&n, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::UnknownType);
    let f = one_frame(&encode_frame(Format::Bin, 50, 1, 0, &[]));
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::UnknownType);
    assert_eq!(codec.from_json_data("Missing", &json!({}), Strictness::Lenient).unwrap_err().code, ErrCode::UnknownType);
}

#[test]
fn direction_and_format_checks() {
    let codec = fixture_codec();
    let fx = fixture_msg(&codec);
    let cmd = cmd_msg(&codec);
    let f = one_frame(&codec.to_frame(&fx, Format::Json).unwrap());
    assert_eq!(codec.decode_raw(&f, Role::Plc, Strictness::Lenient).unwrap_err().code, ErrCode::DirNotAllowed);
    let f = one_frame(&codec.to_frame(&cmd, Format::Bin).unwrap());
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::DirNotAllowed);

    let json_only = Message { id: 42, ..cmd.clone() };
    assert_eq!(codec.to_frame(&json_only, Format::Bin).unwrap_err().code, ErrCode::FormatNotAllowed);
    let sub_sig = codec.registry().by_id(42).unwrap().sig;
    let f = one_frame(&encode_frame(Format::Bin, 42, 1, sub_sig, &cmd.payload));
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::FormatNotAllowed);
    let f = one_frame(&codec.to_frame(&json_only, Format::Json).unwrap());
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Strict).unwrap(), json_only);

    let raw = RawFrame { framing: Framing::Ndjson, format: Format::Bin, msg_type: None, seq: None, sig: None, payload: vec![], http: None };
    assert_eq!(codec.decode_raw(&raw, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::FormatNotAllowed);
}

#[test]
fn header_envelope_consistency() {
    let codec = fixture_codec();
    let hello = codec.hello("GR2", 1, 7).unwrap();
    let text = codec.json_text(&hello).unwrap();
    let f = one_frame(&encode_frame(Format::Json, 40, 7, 0, text.as_bytes()));
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::Parse);
    let f = one_frame(&encode_frame(Format::Json, 1, 8, 0, text.as_bytes()));
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::Parse);
    let f = one_frame(&encode_frame(Format::Json, 1, 7, 0, text.as_bytes()));
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Strict).unwrap(), hello);
    // HTTP JSON with a contradicting X-GR-Type
    let req = write_request("POST", "/api/plc/GR2/hello", "h", &[("Content-Type", "application/json"), ("X-GR-Type", "Ack")], Some(text.as_bytes()), ContentLength::Plain);
    assert_eq!(codec.decode_raw(&one_request(&req), Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::Parse);
    // garbage envelope
    let f = one_frame(&encode_frame(Format::Json, 1, 7, 0, b"{not json"));
    assert_eq!(codec.decode_raw(&f, Role::Pc, Strictness::Lenient).unwrap_err().code, ErrCode::Parse);
}

#[test]
fn envelope_reader_is_flexible() {
    let codec = fixture_codec();
    let cmd = cmd_msg(&codec);
    let data = codec.data_json_text(&cmd).unwrap();
    let text = format!(r#"{{ "data" : {data}, "extra": [1], "sig": 0, "seq": 9, "type": 42 }}"#);
    let m = codec.decode_raw(&ndjson(&text), Role::Plc, Strictness::Strict).unwrap();
    assert_eq!(m, Message { id: 42, ..cmd });
    let env = plc_link::parse_envelope(text.as_bytes()).unwrap();
    assert_eq!((env.type_name, env.type_id, env.seq, env.sig), (None, Some(42), 9, 0));
    assert_eq!(plc_link::parse_envelope(br#"{"seq":1}"#).unwrap_err().code, ErrCode::Parse);
    assert_eq!(plc_link::parse_envelope(br#"{"type":"A","seq":70000}"#).unwrap_err().code, ErrCode::Parse);
    assert_eq!(plc_link::parse_envelope(b"[1]").unwrap_err().code, ErrCode::Parse);

    let hb = ndjson(r#"{"type":"heartbeat","seq":2,"sig":0,"data":{"x":1}}"#);
    assert_eq!(codec.decode_raw(&hb, Role::Pc, Strictness::Strict).unwrap_err().path.as_deref(), Some("data"));
    let (m, r) = codec.decode_raw_report(&hb, Role::Pc, Strictness::Lenient).unwrap();
    assert_eq!((m.id, m.seq, m.payload.len(), r.warnings.len()), (2, 2, 0, 1));
}

#[test]
fn json_data_errors_carry_paths() {
    let codec = fixture_codec();
    let e = codec.from_json_data("fixturecmd", &json!({"Id": 70000}), Strictness::Strict).unwrap_err();
    assert_eq!((e.code, e.path.as_deref()), (ErrCode::Parse, Some("Id")));
    let (m, r) = codec.from_json_data("FixtureCmd", &json!({"Id": 5}), Strictness::Strict).unwrap();
    assert_eq!((m.id, m.seq, m.payload.len()), (41, 0, 26));
    assert_eq!(r.defaulted, vec!["Name", "On", "Pos"]);
}

#[test]
fn ack_requires_an_available_ack_message() {
    let codec = fixture_codec();
    let long = "x".repeat(50);
    let ack = codec.ack(21, 65535, 104, &long, 9).unwrap();
    let v = codec.data_value(&ack).unwrap();
    assert_eq!((v["RefType"].clone(), v["RefSeq"].clone(), v["Code"].clone()), (json!(21), json!(65535), json!(104)));
    assert_eq!(v["Text"].as_str().unwrap().len(), 40);
    assert_eq!((ack.id, ack.seq), (30, 9));

    let mut c = fixture_contract();
    c.udts.remove("LNK_Ack");
    let r = fixture_doc().resolve(&c).unwrap();
    let no_ack = Codec::new(Arc::new(c), Arc::new(r));
    assert_eq!(no_ack.ack(1, 1, 0, "", 1).unwrap_err().code, ErrCode::UnknownType);
}

#[test]
fn seq_wraps_to_one() {
    assert_eq!((next_seq(0), next_seq(1), next_seq(65534), next_seq(65535)), (1, 2, 65535, 1));
}
