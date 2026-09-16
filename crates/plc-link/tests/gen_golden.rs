//! `plc_link::codegen`: fixture goldens (`tests/golden/gen`, LF without BOM, refresh with `UPDATE_GOLDEN=1`), output
//! encodings, determinism, error reporting, and generation for the real GR2 contract against the PLC runtime.

mod common;

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use common::*;
use plc_link::codegen::{BlockKind, GenOpts, GenOutput, Target, generate};
use plc_link::{ErrCode, RegistryDoc};

/// Json_W* / Json_R* runtime functions (siemens docs/link/plc-blocks.md sections 3-4).
const RUNTIME_PRIMITIVES: &[&str] = &[
    "Json_WObjBegin",
    "Json_WObjEnd",
    "Json_WArrBegin",
    "Json_WArrEnd",
    "Json_WComma",
    "Json_WNull",
    "Json_WRaw",
    "Json_WBool",
    "Json_WByte",
    "Json_WWord",
    "Json_WDWord",
    "Json_WLWord",
    "Json_WSInt",
    "Json_WInt",
    "Json_WDInt",
    "Json_WLInt",
    "Json_WUSInt",
    "Json_WUInt",
    "Json_WUDInt",
    "Json_WULInt",
    "Json_WReal",
    "Json_WLReal",
    "Json_WChar",
    "Json_WStr",
    "Json_WTime",
    "Json_WDtl",
    "Json_WDigits",
    "Json_WUDigits",
    "Json_WPad",
    "Json_WFix4",
    "Json_WEsc",
    "Json_RWs",
    "Json_RPrev",
    "Json_RObjBegin",
    "Json_RNextKey",
    "Json_RSkip",
    "Json_RArrBegin",
    "Json_RArrNext",
    "Json_RArrEnd",
    "Json_RNum",
    "Json_RLit",
    "Json_RIntRange",
    "Json_RUIntRange",
    "Json_RBool",
    "Json_RByte",
    "Json_RWord",
    "Json_RDWord",
    "Json_RLWord",
    "Json_RSInt",
    "Json_RInt",
    "Json_RDInt",
    "Json_RLInt",
    "Json_RUSInt",
    "Json_RUInt",
    "Json_RUDInt",
    "Json_RULInt",
    "Json_RReal",
    "Json_RLReal",
    "Json_RChar",
    "Json_RTime",
    "Json_RDtl",
    "Json_RStr",
];

/// Names and data types of the hand-written placeholder siemens export/GR2_PLC/tags/Const/LNK_Const_Gen.xml.
const PLACEHOLDER_CONSTS: &[(&str, &str)] = &[
    ("LNK_MAGIC", "Word"),
    ("LNK_VERSION", "USInt"),
    ("LNK_FMT_JSON", "USInt"),
    ("LNK_FMT_BIN", "USInt"),
    ("LNK_HDR_LEN", "DInt"),
    ("LNK_MAX_PAYLOAD", "DInt"),
    ("LNK_MSG_HELLO", "UInt"),
    ("LNK_MSG_HEARTBEAT", "UInt"),
    ("LNK_MSG_MEASLOG", "UInt"),
    ("LNK_MSG_STATUS", "UInt"),
    ("LNK_MSG_COMMAND", "UInt"),
    ("LNK_MSG_COMMANDRESULT", "UInt"),
    ("LNK_MSG_ACK", "UInt"),
    ("LNK_MSG_TRACECFG", "UInt"),
    ("LNK_MSG_TRACE", "UInt"),
    ("LNK_SIG_HELLO", "DWord"),
    ("LNK_SIG_MEASLOG", "DWord"),
    ("LNK_SIG_STATUS", "DWord"),
    ("LNK_SIG_COMMAND", "DWord"),
    ("LNK_SIG_COMMANDRESULT", "DWord"),
    ("LNK_SIG_ACK", "DWord"),
    ("LNK_SIG_TRACECFG", "DWord"),
    ("LNK_SIG_TRACE", "DWord"),
    ("LNK_LEN_HELLO", "DInt"),
    ("LNK_LEN_MEASLOG", "DInt"),
    ("LNK_LEN_STATUS", "DInt"),
    ("LNK_LEN_COMMAND", "DInt"),
    ("LNK_LEN_COMMANDRESULT", "DInt"),
    ("LNK_LEN_ACK", "DInt"),
    ("LNK_LEN_TRACECFG", "DInt"),
    ("LNK_LEN_TRACE", "DInt"),
    ("LNK_JMAX_HELLO", "DInt"),
    ("LNK_JMAX_MEASLOG", "DInt"),
    ("LNK_JMAX_STATUS", "DInt"),
    ("LNK_JMAX_COMMAND", "DInt"),
    ("LNK_JMAX_COMMANDRESULT", "DInt"),
    ("LNK_JMAX_ACK", "DInt"),
    ("LNK_JMAX_TRACECFG", "DInt"),
    ("LNK_REGISTRY_HASH", "DWord"),
    ("LNK_ERR_OK", "Int"),
    ("LNK_ERR_BAD_MAGIC", "Int"),
    ("LNK_ERR_BAD_VERSION", "Int"),
    ("LNK_ERR_UNKNOWN_TYPE", "Int"),
    ("LNK_ERR_SIG_MISMATCH", "Int"),
    ("LNK_ERR_BAD_LENGTH", "Int"),
    ("LNK_ERR_PARSE", "Int"),
    ("LNK_ERR_FORMAT_NOT_ALLOWED", "Int"),
    ("LNK_ERR_DIR_NOT_ALLOWED", "Int"),
    ("LNK_ERR_BUSY", "Int"),
    ("LNK_ERR_NO_PENDING", "Int"),
];

fn opts(plc: &str, strict: bool) -> GenOpts {
    GenOpts { plc: plc.to_string(), strict }
}

fn fixture_out() -> GenOutput {
    generate(&fixture_contract(), &fixture_registry(), &opts("FIXTURE", false)).expect("generate fixture")
}

/// File text without BOM and with LF line ends (golden form).
fn lf_text(bytes: &[u8]) -> String {
    let s = String::from_utf8(bytes.to_vec()).expect("UTF-8 output");
    s.strip_prefix('\u{feff}').unwrap_or(&s).replace("\r\n", "\n")
}

fn golden_root() -> PathBuf {
    crate_dir().join("tests/golden/gen")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, out);
        } else {
            out.push(p);
        }
    }
}

#[test]
fn fixture_output_matches_goldens() {
    let out = fixture_out();
    let root = golden_root();
    let update = std::env::var("UPDATE_GOLDEN").is_ok_and(|v| v == "1");
    if update {
        let _ = std::fs::remove_dir_all(&root);
    }
    let mut expected = BTreeSet::new();
    let mut problems = Vec::new();
    for f in &out.files {
        let rel = format!("{}/{}", f.target.name(), f.path);
        let path = root.join(&rel);
        let text = lf_text(&f.bytes);
        if update {
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            std::fs::write(&path, &text).unwrap();
        } else {
            match std::fs::read_to_string(&path) {
                Ok(g) if g.replace("\r\n", "\n") == text => {}
                Ok(_) => problems.push(format!("{rel} differs")),
                Err(_) => problems.push(format!("{rel} missing")),
            }
        }
        expected.insert(rel);
    }
    if !update {
        let mut files = Vec::new();
        walk(&root, &mut files);
        for p in files {
            let rel = p.strip_prefix(&root).unwrap().to_string_lossy().replace('\\', "/");
            if !expected.contains(&rel) {
                problems.push(format!("{rel} is not generated any more"));
            }
        }
    }
    assert!(problems.is_empty(), "goldens out of date (refresh with UPDATE_GOLDEN=1): {problems:#?}");
}

#[test]
fn fixture_manifest_is_callee_first_and_skips_unavailable() {
    let out = fixture_out();
    let names: Vec<(&str, BlockKind)> = out.blocks.iter().map(|b| (b.name.as_str(), b.kind)).collect();
    assert_eq!(
        names,
        vec![
            ("LNK_Const_Gen", BlockKind::TagTable),
            ("JsonW_LNK_Ack", BlockKind::WriterFc),
            ("JsonW_LNK_FixtureSub", BlockKind::WriterFc),
            ("JsonW_LNK_Fixture", BlockKind::WriterFc),
            ("JsonW_LNK_Hello", BlockKind::WriterFc),
            ("JsonR_LNK_Ack", BlockKind::ReaderFc),
            ("JsonR_LNK_FixtureSub", BlockKind::ReaderFc),
            ("JsonR_LNK_Hello", BlockKind::ReaderFc),
            ("LnkJ_Hello", BlockKind::EnvelopeFc),
            ("LnkJ_Heartbeat", BlockKind::EnvelopeFc),
            ("LnkJ_Ack", BlockKind::EnvelopeFc),
            ("LnkJ_Fixture", BlockKind::EnvelopeFc),
            ("LnkJ_FixtureCmd", BlockKind::EnvelopeFc),
            ("LnkJ_JsonOnly", BlockKind::EnvelopeFc),
            ("LNK_TestVectors", BlockKind::GlobalDb),
        ]
    );
    assert_eq!(out.warnings.len(), 1, "{:?}", out.warnings);
    assert!(out.warnings[0].contains("Missing"));
    assert!(out.file(Target::Pc, "vectors/Missing.json").is_none());
    assert!(out.file(Target::Pc, "vectors/Heartbeat.json").is_some());

    let err = generate(&fixture_contract(), &fixture_registry(), &opts("FIXTURE", true)).unwrap_err();
    assert_eq!(err.code, ErrCode::UnknownType);
    assert!(err.msg.contains("Missing"), "{}", err.msg);
}

#[test]
fn raw_output_is_bom_crlf_for_tia_and_plain_lf_for_pc() {
    for f in &fixture_out().files {
        let b = &f.bytes;
        match f.target {
            Target::Scl | Target::Tags => {
                assert!(b.starts_with(&[0xEF, 0xBB, 0xBF]), "{}: BOM", f.path);
                assert!(b.ends_with(b"\r\n") && !b.ends_with(b"\r\n\r\n"), "{}: one trailing CRLF", f.path);
                for (i, &x) in b.iter().enumerate() {
                    if x == b'\n' {
                        assert_eq!(b[i - 1], b'\r', "{}: bare LF at byte {i}", f.path);
                    }
                }
            }
            Target::Pc => {
                assert!(!b.starts_with(&[0xEF, 0xBB, 0xBF]), "{}: no BOM", f.path);
                assert!(!b.contains(&b'\r'), "{}: LF only", f.path);
                assert!(b.ends_with(b"\n"), "{}: trailing LF", f.path);
            }
        }
    }
}

#[test]
fn generation_is_deterministic() {
    assert_eq!(fixture_out(), fixture_out());
    let c = gr2_contract();
    let r = repo_registry_doc().resolve_strict(&c).unwrap();
    assert_eq!(generate(&c, &r, &opts("GR2_PLC", true)).unwrap(), generate(&c, &r, &opts("GR2_PLC", true)).unwrap());
}

#[test]
fn unsupported_member_type_fails_with_member_path() {
    let r = fixture_registry();
    let mut c = fixture_contract();
    c.add_udt_source(
        "TYPE \"LNK_FixtureSub\"\nVERSION : 0.1\n   STRUCT\n      Id : UInt;\n      Inner : Struct\n         When : Array[0..1] of Time_Of_Day;\n      END_STRUCT;\n   END_STRUCT;\n\nEND_TYPE\n",
    )
    .unwrap();
    let err = generate(&c, &r, &opts("FIXTURE", false)).unwrap_err();
    let path = err.path.clone().unwrap_or_default();
    assert!(path.ends_with("Inner.When[]"), "path {path:?}: {err}");
    assert!(err.msg.contains("Tod") && err.msg.contains("not supported"), "{err}");
}

#[test]
fn too_long_block_name_suggests_alias() {
    let long = format!("LNK_{}", "X".repeat(112));
    let mut c = fixture_contract();
    c.add_udt_source(&format!("TYPE \"{long}\"\nVERSION : 0.1\n   STRUCT\n      A : Int;\n   END_STRUCT;\n\nEND_TYPE\n")).unwrap();
    let text =
        std::fs::read_to_string(fixture_dir().join("messages.toml")).unwrap().replace("[alias]", "") + &format!("\n[[message]]\nid = 60\nname = \"Long\"\nudt = \"{long}\"\ndir = \"plc_to_pc\"\n");
    let doc: RegistryDoc = text.parse().unwrap();
    let mut r = doc.resolve(&c).unwrap();
    let writer = format!("JsonW_{long}");

    let err = generate(&c, &r, &opts("FIXTURE", false)).unwrap_err();
    assert!(err.msg.contains("[alias]") && err.msg.contains(&writer) && err.msg.contains("122"), "{}", err.msg);

    r.doc.alias.insert(writer, "JsonW_LongShort".to_string());
    let out = generate(&c, &r, &opts("FIXTURE", false)).unwrap();
    assert!(out.blocks.iter().any(|b| b.name == "JsonW_LongShort" && b.kind == BlockKind::WriterFc));
    let env = lf_text(&out.file(Target::Scl, "LnkJ_Long.scl").unwrap().bytes);
    assert!(env.contains("\"JsonW_LongShort\"(Buf := #Buf, Pos := #Pos, Val := #Val);"), "{env}");
}

/// Callee names of `"Name"(` calls outside string literals and comments.
fn calls(scl: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in scl.lines() {
        let code = line.split("//").next().unwrap_or("");
        let mut clean = String::new();
        let mut in_lit = false;
        for ch in code.chars() {
            if ch == '\'' {
                in_lit = !in_lit;
            } else if !in_lit {
                clean.push(ch);
            }
        }
        let mut rest = clean.as_str();
        while let Some(end) = rest.find("\"(") {
            if let Some(start) = rest[..end].rfind('"') {
                out.push(rest[start + 1..end].to_string());
            }
            rest = &rest[end + 2..];
        }
    }
    out
}

/// Text of the parameter sections (VAR_INPUT / VAR_OUTPUT / VAR_IN_OUT) of an FC source.
fn parameter_sections(scl: &str) -> String {
    let mut out = String::new();
    let mut inside = false;
    for line in scl.lines() {
        let t = line.trim();
        if t == "VAR_INPUT" || t == "VAR_OUTPUT" || t == "VAR_IN_OUT" {
            inside = true;
        } else if t == "END_VAR" {
            inside = false;
        } else if inside {
            out.push_str(t);
            out.push('\n');
        }
    }
    out
}

fn count_words(scl: &str, word: &str) -> usize {
    scl.lines().map(|l| l.split("//").next().unwrap_or("")).flat_map(|l| l.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))).filter(|w| *w == word).count()
}

#[test]
fn gr2_contract_generates_against_the_plc_runtime() {
    let c = gr2_contract();
    let r = repo_registry_doc().resolve_strict(&c).expect("every message available in plc/contract/GR2_PLC");
    let out = generate(&c, &r, &opts("GR2_PLC", true)).expect("generate GR2_PLC");

    let generated: BTreeSet<&str> = out.blocks.iter().map(|b| b.name.as_str()).collect();
    let mut earlier: BTreeSet<&str> = BTreeSet::new();
    for b in &out.blocks {
        if b.kind != BlockKind::TagTable {
            let text = lf_text(&out.file(b.target, &b.file).unwrap().bytes);
            for callee in calls(&text) {
                if RUNTIME_PRIMITIVES.contains(&callee.as_str()) {
                    continue;
                }
                assert!(generated.contains(callee.as_str()), "{} calls {callee}, neither a runtime primitive nor generated", b.name);
                assert!(earlier.contains(callee.as_str()), "{} calls {callee}, which is not earlier in the manifest", b.name);
            }
            if b.kind != BlockKind::GlobalDb {
                assert!(text.starts_with(&format!("FUNCTION \"{}\" : Void\n", b.name)), "{}", b.name);
                assert!(!parameter_sections(&text).contains("String["), "{}: String length in a parameter", b.name);
                for (open, close) in [("IF", "END_IF"), ("WHILE", "END_WHILE"), ("FOR", "END_FOR")] {
                    assert_eq!(count_words(&text, open), count_words(&text, close), "{}: {open} / {close}", b.name);
                }
                // loop bounds and index tests use resolved integer literals (TIA: DInt FOR variable vs. USInt constants)
                for line in text.lines().map(|l| l.split("//").next().unwrap_or("").trim()) {
                    if line.starts_with("FOR #i") || line.starts_with("IF #i") || (line.starts_with("#i") && !line.contains('+')) {
                        assert!(!line.contains('"'), "{}: constant in loop statement `{line}`", b.name);
                    }
                }
            }
        }
        earlier.insert(&b.name);
    }

    let of_kind = |k: BlockKind| out.blocks.iter().filter(|b| b.kind == k).map(|b| b.from.as_str()).collect::<BTreeSet<_>>();
    assert_eq!(of_kind(BlockKind::EnvelopeFc), BTreeSet::from(["Hello", "Heartbeat", "MeasLog", "Status", "Command", "CommandResult", "Ack", "TraceCfg"]));
    assert_eq!(
        of_kind(BlockKind::ReaderFc),
        BTreeSet::from([
            "LNK_Hello",
            "LNK_Ack",
            "LNK_TraceCfg",
            "LNK_TraceChan",
            "LGR_Interface_GR_Command",
            "LGR_Command_Header",
            "LGR_Command_Command",
            "LGR_Task_Data",
            "LGR_Stock_Item",
            "LGR_Cell_Info"
        ])
    );
    // Trace is BIN only: no envelope, no JSON closure for its payload, no golden vector text.
    let trace = r.by_name("Trace").unwrap();
    assert!(!trace.allows(plc_link::Format::Json) && trace.json_max == 0);
    for kind in [BlockKind::EnvelopeFc, BlockKind::WriterFc, BlockKind::ReaderFc] {
        assert!(!of_kind(kind).contains("Trace") && !of_kind(kind).contains("LNK_Trace"), "{kind:?} generated for Trace");
    }
    assert!(!lf_text(&out.file(Target::Scl, "LNK_TestVectors.db").unwrap().bytes).contains("Trace_Val"));
    assert!(out.file(Target::Pc, "schema/Trace.schema.json").is_none() && out.file(Target::Pc, "vectors/Trace.json").is_none());
    for u in ["LNK_Hello", "LNK_Ack", "LNK_GR_Status", "LGR_Equip_Status", "LGR_MeasureLog", "LGR_Command_Response", "LGR_Task_Data"] {
        assert!(of_kind(BlockKind::WriterFc).contains(u), "writer for {u}");
    }

    // FB_LnkSession call shapes
    let session = |file: &str| lf_text(&out.file(Target::Scl, file).unwrap().bytes);
    assert!(session("LnkJ_Heartbeat.scl").contains("      Seq : UInt;") && !session("LnkJ_Heartbeat.scl").contains("      Val :"));
    assert!(session("LnkJ_Ack.scl").contains("      Val : \"LNK_Ack\";"));
    assert!(session("JsonR_LGR_Interface_GR_Command.scl").contains("      Val : \"LGR_Interface_GR_Command\";"));

    // constant table: the placeholder's names and types, real values
    let consts: Vec<(&str, &str)> = out.constants.iter().map(|k| (k.name.as_str(), k.data_type)).collect();
    assert_eq!(consts, PLACEHOLDER_CONSTS);
    let value = |n: &str| out.constants.iter().find(|k| k.name == n).unwrap().value.clone();
    assert_eq!(value("LNK_MAGIC"), "16#4753");
    assert_eq!(value("LNK_MAX_PAYLOAD"), "8176");
    assert_eq!(value("LNK_MSG_COMMANDRESULT"), "21");
    assert_eq!(value("LNK_SIG_MEASLOG"), format!("16#{:08X}", c.udt_sig("LGR_MeasureLog").unwrap()));
    assert_eq!(value("LNK_LEN_MEASLOG"), "236");
    assert_eq!(value("LNK_JMAX_STATUS"), r.by_name("Status").unwrap().json_max.to_string());
    assert_eq!(value("LNK_REGISTRY_HASH"), format!("16#{:08X}", r.hash()));
    let xml = lf_text(&out.file(Target::Tags, "LNK_Const_Gen.xml").unwrap().bytes);
    let parsed = plc_layout::consts::parse_const_xml(&xml).unwrap();
    assert_eq!(parsed.len(), PLACEHOLDER_CONSTS.len());

    // test vector DB: standard access, parses, fits
    let db = lf_text(&out.file(Target::Scl, "LNK_TestVectors.db").unwrap().bytes);
    let mut cc = c.clone();
    cc.add_db_source(&db).unwrap();
    let size = cc.size_of_db("LNK_TestVectors").unwrap();
    assert!(size < plc_link::codegen::TEST_DB_MAX_BYTES, "{size}");
    assert!(!cc.db("LNK_TestVectors").unwrap().optimized);
}
