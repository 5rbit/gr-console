mod common;

use std::str::FromStr;

use common::*;
use plc_layout::ast::TypeRef;
use plc_link::envelope::envelope_max_len;
use plc_link::wire_json::max_json_len;
use plc_link::{Direction, ErrCode, Format, RegistryDoc};

fn fixture_text() -> String {
    std::fs::read_to_string(fixture_dir().join("messages.toml")).unwrap()
}

fn parse_err(text: &str) -> String {
    RegistryDoc::from_str(text).expect_err("should be rejected").msg
}

#[test]
fn fixture_resolves_with_sig_size_and_availability() {
    let c = fixture_contract();
    let r = fixture_registry();
    let ids: Vec<u16> = r.messages.iter().map(|m| m.id).collect();
    assert_eq!(ids, vec![1, 2, 30, 40, 41, 42, 50]);

    let hello = r.by_name("hello").unwrap();
    assert_eq!(hello.id, 1);
    assert_eq!(hello.sig, c.udt_sig("LNK_Hello").unwrap());
    assert_eq!(hello.size, 28); // String[16] 18 + USInt + Byte + DWord + UDInt
    assert_eq!(hello.reply, Some(1));
    assert_eq!(hello.json_max, envelope_max_len("Hello", max_json_len(&c, &TypeRef::Udt("LNK_Hello".into())).unwrap()));

    let hb = r.by_id(2).unwrap();
    assert!(hb.available && !hb.has_payload());
    assert_eq!((hb.sig, hb.size), (0, 0));
    assert_eq!(hb.json_max, envelope_max_len("Heartbeat", 4));

    let ack = r.by_http_name("ack").unwrap();
    assert_eq!(ack.size, 48);

    let fixture = r.lookup("40").unwrap();
    assert_eq!(fixture.name, "Fixture");
    assert_eq!(fixture.dir, Direction::PlcToPc);
    assert!(fixture.ack);
    assert_eq!(r.by_name("FIXTURECMD").unwrap().reply, Some(40));
    assert_eq!(r.by_name("JsonOnly").unwrap().formats, vec![Format::Json]);

    let missing = r.by_name("Missing").unwrap();
    assert!(!missing.available);
    assert!(missing.unavailable_reason.as_deref().unwrap().contains("LNK_DoesNotExist"));
    assert_eq!((missing.sig, missing.size), (0, 0));
    assert!(r.by_name("nope").is_none());

    let err = fixture_doc().resolve_strict(&c).unwrap_err();
    assert_eq!(err.code, ErrCode::UnknownType);
}

#[test]
fn hash_follows_spec_text_and_is_stable() {
    let c = fixture_contract();
    let r = fixture_registry();
    let expected = format!(
        "1:Hello:{:08X}:28;2:Heartbeat:00000000:0;30:Ack:{:08X}:48;40:Fixture:{:08X}:{};41:FixtureCmd:{:08X}:{};42:JsonOnly:{:08X}:{};50:Missing:00000000:0;",
        c.udt_sig("LNK_Hello").unwrap(),
        c.udt_sig("LNK_Ack").unwrap(),
        c.udt_sig("LNK_Fixture").unwrap(),
        c.size_of_udt("LNK_Fixture").unwrap(),
        c.udt_sig("LNK_FixtureSub").unwrap(),
        c.size_of_udt("LNK_FixtureSub").unwrap(),
        c.udt_sig("LNK_FixtureSub").unwrap(),
        c.size_of_udt("LNK_FixtureSub").unwrap(),
    );
    assert_eq!(r.hash_text(), expected);
    assert_eq!(r.hash(), crc32fast::hash(expected.as_bytes()));
    assert_eq!(fixture_registry().hash(), r.hash());
    // declaration order in the file does not matter (sorted by id)
    let text = fixture_text();
    let (head, rest) = text.split_once("[[message]]").unwrap();
    let mut blocks: Vec<&str> = rest.split("[[message]]").collect();
    blocks.reverse();
    let reordered = format!("{head}[[message]]{}", blocks.join("[[message]]"));
    let r2 = RegistryDoc::from_str(&reordered).unwrap().resolve(&c).unwrap();
    assert_eq!(r2.hash(), r.hash());
    // a layout change changes the hash
    let mut c2 = fixture_contract();
    c2.add_udt_source("TYPE \"LNK_Ack\"\n   STRUCT\n      RefType : UInt;\n      RefSeq : UInt;\n      Code : Int;\n      Text : String[39];\n   END_STRUCT;\nEND_TYPE\n").unwrap();
    assert_ne!(fixture_doc().resolve(&c2).unwrap().hash(), r.hash());
}

#[test]
fn validation_rejects_bad_registries() {
    let text = fixture_text();
    assert!(parse_err(&text.replacen("id = 30", "id = 1", 1)).contains("duplicate message id"));
    assert!(parse_err(&text.replacen("name = \"Ack\"", "name = \"HELLO\"", 1)).contains("duplicate message name"));
    assert!(parse_err(&text.replacen("reply = \"Fixture\"", "reply = \"Nope\"", 1)).contains("reply"));
    assert!(parse_err(&text.replacen("dir = \"both\"", "dir = \"sideways\"", 1)).contains("sideways"));
    assert!(parse_err(&text.replacen("formats = [\"json\"]", "formats = [\"xml\"]", 1)).contains("xml"));
    assert!(parse_err(&text.replacen("formats = [\"json\"]", "formats = []", 1)).contains("formats is empty"));
    assert!(parse_err(&text.replacen("ack = true", "ack = true\nextra = 1", 1)).contains("extra"));
    assert!(parse_err(&text.replacen("magic = 0x4753", "magic = 0x4754", 1)).contains("magic"));
    assert!(parse_err(&text.replacen("name = \"Hello\"", "name = \"Hel lo\"", 1)).contains("identifier"));
    assert!(parse_err(&text.replacen("id = 42", "id = 0", 1)).contains("reserved"));
    assert!(parse_err(&text.replacen("response = \"FixtureCmd\"", "response = \"Nope\"", 1)).contains("Nope"));
    let with_errors = format!("{text}\n[[error]]\ncode = 1\nname = \"BAD_MAGIX\"\n");
    assert!(parse_err(&with_errors).contains("BAD_MAGIC"));
}

#[test]
fn unsupported_member_type_is_an_error_with_path() {
    let mut c = fixture_contract();
    c.add_udt_source("TYPE \"LNK_Ack\"\n   STRUCT\n      RefType : UInt;\n      Nested : Struct\n         When : Date;\n      END_STRUCT;\n   END_STRUCT;\nEND_TYPE\n").unwrap();
    let e = fixture_doc().resolve(&c).unwrap_err();
    assert_eq!(e.path.as_deref(), Some("Nested.When"), "{e}");
}

#[test]
fn repo_registry_resolves_against_gr2_contract() {
    let doc = repo_registry_doc();
    assert_eq!((doc.magic, doc.proto_version, doc.max_payload, doc.heartbeat_ms), (0x4753, 1, 65535, 5000));
    assert_eq!((doc.const_prefix.as_str(), doc.writer_prefix.as_str(), doc.reader_prefix.as_str(), doc.envelope_prefix.as_str()), ("LNK_", "JsonW_", "JsonR_", "LnkJ_"));
    assert_eq!(doc.errors.iter().filter(|e| !e.runtime).count(), 11);
    assert_eq!(doc.errors.iter().filter(|e| e.runtime).count(), 8);
    let c = gr2_contract();
    let r = doc.resolve(&c).unwrap();
    let names: Vec<(u16, &str)> = r.messages.iter().map(|m| (m.id, m.name.as_str())).collect();
    assert_eq!(names, vec![(1, "Hello"), (2, "Heartbeat"), (10, "MeasLog"), (11, "Status"), (20, "Command"), (21, "CommandResult"), (30, "Ack"), (40, "TraceCfg"), (41, "Trace")]);
    // Trace carries one fixed-size chunk as Serialize bytes only: no JSON, so no worst-case envelope length
    let trace = r.by_name("Trace").unwrap();
    assert!(trace.available && !trace.ack && trace.allows(Format::Bin) && !trace.allows(Format::Json));
    assert_eq!((trace.size, trace.json_max), (c.size_of_udt("LNK_Trace").unwrap(), 0));
    assert_eq!(trace.size, 8160);
    assert!(r.by_name("TraceCfg").unwrap().json_max > 0);
    let meas = r.by_name("MeasLog").unwrap();
    assert!(meas.available && meas.ack);
    assert_eq!((meas.size, meas.sig), (236, c.udt_sig("LGR_MeasureLog").unwrap()));
    assert_eq!(r.by_name("Command").unwrap().reply, Some(21));
    // the real LGR_Command_Command uses 8-Bool structs (2 bytes each), so this is larger than the simplified
    // reference in plc-layout's tests
    assert_eq!(r.by_name("CommandResult").unwrap().size, c.size_of_udt("LGR_Command_Response").unwrap());
    assert_eq!(r.by_name("CommandResult").unwrap().size, 174);
    assert_eq!(r.hash(), crc32fast::hash(r.hash_text().as_bytes()));
}
