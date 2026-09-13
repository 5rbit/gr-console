//! The wire JSON reader must produce exactly the bytes plc-layout's encoder produces for the same value, and
//! the writer must round-trip them to the same JSON value.

mod common;

use common::*;
use plc_layout::Contract;
use plc_layout::ast::TypeRef;
use plc_layout::layout::Cursor;
use plc_link::vectors::{golden_bytes, golden_value, zero_value};
use plc_link::wire_json::{Strictness, json_to_udt_bytes, max_json_len, udt_bytes_to_json_text, zero_udt_bytes};
use serde_json::Value;

fn cross_check(c: &Contract, udt: &str) {
    let ty = TypeRef::Udt(udt.to_string());
    let size = c.size_of_udt(udt).unwrap() as usize;
    let v = golden_value(c, udt).unwrap();

    let mut a = vec![0u8; size];
    plc_layout::encode::encode_tree(c, &ty, &v, &mut a, &mut Cursor::default(), "").unwrap();
    let (b, report) = json_to_udt_bytes(c, udt, &v, Strictness::Strict).unwrap();
    assert!(report.is_clean(), "{udt}: {report:?}");
    assert_eq!(b.len(), size, "{udt}");
    assert_eq!(a, b, "{udt}: plc-layout encode != wire_json bytes");
    assert_eq!(golden_bytes(c, udt).unwrap(), b);

    let t = udt_bytes_to_json_text(c, udt, &b).unwrap();
    assert!(t.is_ascii() && !t.contains(' ') && !t.contains('\n'), "{udt}: {t}");
    let back: Value = serde_json::from_str(&t).unwrap();
    assert_eq!(back, v, "{udt}: text round trip");
    let max = max_json_len(c, &ty).unwrap();
    assert!(t.len() <= max, "{udt}: {} > {max}", t.len());
    // declaration order: top-level keys appear in field order
    let fields = &c.udt(udt).unwrap().fields;
    let mut last = 0;
    for f in fields {
        let pos = t[last..].find(&format!("\"{}\":", f.name)).map(|p| p + last).unwrap_or_else(|| panic!("{udt}: key {} out of order", f.name));
        last = pos;
    }
    // re-encoding the text gives the same bytes
    assert_eq!(json_to_udt_bytes(c, udt, &back, Strictness::Strict).unwrap().0, b);

    // zero value
    let z = zero_value(c, udt).unwrap();
    let (zb, zr) = json_to_udt_bytes(c, udt, &z, Strictness::Strict).unwrap();
    assert!(zr.is_clean());
    assert_eq!(zb, zero_udt_bytes(c, udt).unwrap(), "{udt}: zero bytes");
    assert_eq!(serde_json::from_str::<Value>(&udt_bytes_to_json_text(c, udt, &zb).unwrap()).unwrap(), z, "{udt}: zero text");
    let (eb, er) = json_to_udt_bytes(c, udt, &serde_json::json!({}), Strictness::Strict).unwrap();
    assert_eq!(eb, zb, "{udt}: {{}} == zero value");
    assert_eq!(er.defaulted.len(), fields.len());
}

#[test]
fn fixture_udts() {
    let c = fixture_contract();
    for u in ["LNK_Fixture", "LNK_FixtureSub", "LNK_Hello", "LNK_Ack"] {
        cross_check(&c, u);
    }
}

#[test]
fn gr2_message_udts() {
    let c = gr2_contract();
    for u in ["LGR_MeasureLog", "LGR_Interface_GR_Command", "LGR_Command_Response", "LGR_Task_Data", "LGR_Command_Command"] {
        cross_check(&c, u);
    }
    assert_eq!(c.size_of_udt("LGR_MeasureLog").unwrap(), 236);
}

#[test]
fn golden_values_are_deterministic_and_varied() {
    let c = fixture_contract();
    let v = golden_value(&c, "LNK_Fixture").unwrap();
    assert_eq!(v, golden_value(&c, "LNK_Fixture").unwrap());
    assert_eq!(v["Ts"], "2026-09-13T10:11:12.345");
    let bools: Vec<bool> = (0..9).map(|i| v[format!("B{i}")].as_bool().unwrap()).collect();
    assert!(bools.contains(&true) && bools.contains(&false), "{bools:?}");
    let s = v["Str"].as_str().unwrap();
    assert!(s.starts_with('S') && s.len() == 8);
    let r = v["R"].as_f64().unwrap();
    assert!(r.abs() <= 10000.0 && (r * 4.0).fract() == 0.0);
    assert!(v["Tm"].as_i64().unwrap().abs() <= 1_000_000);
    assert_ne!(v["Subs"][0], v["Subs"][1]);
}
