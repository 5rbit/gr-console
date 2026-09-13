mod common;

use common::*;
use plc_layout::ast::TypeRef;
use plc_layout::encode::dtl_bytes;
use plc_link::ErrCode;
use plc_link::vectors::golden_value;
use plc_link::wire_json::*;
use serde_json::{Value, json};

#[test]
fn real_text_follows_spec() {
    let cases: &[(f64, &str)] = &[
        (3.0, "3.0"),
        (12.25, "12.25"),
        (-0.5, "-0.5"),
        (0.00004, "0.0"),
        (-0.00004, "0.0"),
        (0.0, "0.0"),
        (-0.0, "0.0"),
        (1.5e9, "1.5e9"),
        (-2.0e12, "-2.0e12"),
        (1.0e9, "1.0e9"),
        (999_999_999.999_94, "999999999.9999"),
        (123.45678, "123.4568"),
        (-7.00006, "-7.0001"),
        (1.23456789e20, "1.2346e20"),
        (9.99996e9, "1.0e10"),
        (f64::NAN, "null"),
        (f64::INFINITY, "null"),
        (f64::NEG_INFINITY, "null"),
        (0.1f32 as f64, "0.1"),
        (f32::MAX as f64, "3.4028e38"),
    ];
    for (x, t) in cases {
        assert_eq!(fmt_real(*x), *t, "fmt_real({x:e})");
    }
}

#[test]
fn dtl_text_and_parsing() {
    assert_eq!(fmt_dtl(&dtl_bytes(2026, 9, 13, 10, 11, 12, 345_678_901)), "2026-09-13T10:11:12.345");
    assert_eq!(fmt_dtl(&dtl_bytes(1970, 1, 1, 0, 0, 0, 7_000_000)), "1970-01-01T00:00:00.007");
    assert_eq!(fmt_dtl(&[0; 12]), "0000-00-00T00:00:00.000");
    let d = parse_dtl_text("2026-09-13 10:11:12").unwrap();
    assert_eq!(d, dtl_bytes(2026, 9, 13, 10, 11, 12, 0));
    assert_eq!(d[4], 1, "Sunday");
    assert_eq!(parse_dtl_text("2024-02-29T23:59:59.123456789").unwrap(), dtl_bytes(2024, 2, 29, 23, 59, 59, 123_456_789));
    assert_eq!(parse_dtl_text("2026-09-13T10:11:12.5").unwrap(), dtl_bytes(2026, 9, 13, 10, 11, 12, 500_000_000));
    for bad in ["2026-02-29T00:00:00", "2026-09-13T24:00:00", "1969-12-31T23:59:59", "2026-09-13T10:11:12.1234567890", "2026-09-13X10:11:12", "2026-9-13T10:11:12", "2026-09-13T10:11:12Z", ""] {
        assert!(parse_dtl_text(bad).is_none(), "{bad:?}");
    }
    assert_eq!(dtl_epoch(), dtl_bytes(1970, 1, 1, 0, 0, 0, 0));
}

#[test]
fn string_escapes() {
    assert_eq!(escape_str(b"a\"b\\c\x01\x1f\x7f\xe9 ~"), "a\\\"b\\\\c\\u0001\\u001F\\u007F\\u00E9 ~");
    assert_eq!(escape_str(b""), "");
}

fn sub_bytes() -> Vec<u8> {
    // Id UInt @0, Name String[4] @2, On Bool @8.0, Pos Array["X".."G"] of Real @10, size 26
    let mut b = vec![0u8; 26];
    b[0..2].copy_from_slice(&513u16.to_be_bytes());
    b[2] = 4;
    b[3] = 3;
    b[4..7].copy_from_slice(b"\"\\\xff");
    b[8] = 1;
    for (i, x) in [3.0f32, 12.25, -0.5, f32::NAN].iter().enumerate() {
        b[10 + 4 * i..14 + 4 * i].copy_from_slice(&x.to_bits().to_be_bytes());
    }
    b
}

#[test]
fn writer_is_byte_exact() {
    let c = fixture_contract();
    assert_eq!(c.size_of_udt("LNK_FixtureSub").unwrap(), 26);
    let raw = udt_bytes_to_json_text(&c, "LNK_FixtureSub", &sub_bytes()).unwrap();
    assert!(raw.is_ascii() && raw.contains("\\\"\\\\\\u00FF\""), "{raw}");
    let t = raw.replace("\\u00FF", "\u{ff}");
    assert_eq!(t, r#"{"Id":513,"Name":"\"\\ÿ","On":true,"Pos":[3.0,12.25,-0.5,null]}"#);
    // braces 2 + commas 3 + keys 23 + UInt 5 + String[4] 26 + Bool 5 + 4 Reals (2 + 4*15 + 3) 65
    assert_eq!(max_json_len(&c, &TypeRef::Udt("LNK_FixtureSub".into())).unwrap(), 129);
    // short buffer
    let e = udt_bytes_to_json_text(&c, "LNK_FixtureSub", &sub_bytes()[..20]).unwrap_err();
    assert_eq!((e.code, e.path.as_deref()), (ErrCode::BadLength, Some("Pos[3]")));

    // reading the text back: NaN became null → strict error, lenient 0.0
    let v: Value = serde_json::from_str(&t).unwrap();
    let e = json_to_udt_bytes(&c, "LNK_FixtureSub", &v, Strictness::Strict).unwrap_err();
    assert_eq!((e.code, e.path.as_deref()), (ErrCode::Parse, Some("Pos[4]")));
    let (b, r) = json_to_udt_bytes(&c, "LNK_FixtureSub", &v, Strictness::Lenient).unwrap();
    assert_eq!(&b[..22], &sub_bytes()[..22]);
    assert_eq!(&b[22..], &[0, 0, 0, 0]);
    assert_eq!(r.warnings.len(), 1);
    assert!(r.defaulted.is_empty());
}

#[test]
fn fixture_layout_offsets() {
    let c = fixture_contract();
    let l = c.layout_udt("LNK_Fixture").unwrap();
    let at = |p: &str| {
        let m = l.find(p).unwrap_or_else(|| panic!("{p}"));
        (m.offset, m.bit)
    };
    assert_eq!(at("B7"), (0, Some(7)));
    assert_eq!(at("B8"), (1, Some(0)));
    assert_eq!(at("By").0, 2);
    assert_eq!(at("Si").0, 3);
    assert_eq!(at("I").0, 4);
    assert_eq!(at("Lr").0, 20);
    assert_eq!(at("Ch").0, 36);
    assert_eq!(at("Str").0, 38);
    assert_eq!(at("Ts").0, 48);
    assert_eq!(at("Tm").0, 60);
    assert_eq!(at("Flags[2]"), (64, Some(2)));
    assert_eq!(at("Axis[1]").0, 66);
    assert_eq!(at("Grid[2][1]").0, 84);
    assert_eq!(at("Outer.A").0, 86);
    assert_eq!(at("Outer.Inner.Flag"), (88, Some(0)));
    assert_eq!(at("Outer.Inner.Val").0, 90);
    assert_eq!(at("Sub.Id").0, 92);
    assert_eq!(at("Subs[1].Id").0, 144);
    assert_eq!(l.size, 170);
}

fn golden() -> Value {
    golden_value(&fixture_contract(), "LNK_Fixture").unwrap()
}

fn strict_err(mutate: impl FnOnce(&mut Value)) -> (ErrCode, String) {
    let mut v = golden();
    mutate(&mut v);
    let e = json_to_udt_bytes(&fixture_contract(), "LNK_Fixture", &v, Strictness::Strict).unwrap_err();
    (e.code, e.path.unwrap_or_default())
}

#[test]
fn strict_errors_carry_paths() {
    let p = |code, path: &str| (code, path.to_string());
    assert_eq!(strict_err(|v| v["Outer"]["Inner"]["Extra"] = json!(1)), p(ErrCode::Parse, "Outer.Inner.Extra"));
    assert_eq!(strict_err(|v| v["I"] = json!("5")), p(ErrCode::Parse, "I"));
    assert_eq!(strict_err(|v| v["B3"] = json!(1)), p(ErrCode::Parse, "B3"));
    assert_eq!(strict_err(|v| v["By"] = json!(256)), p(ErrCode::Parse, "By"));
    assert_eq!(strict_err(|v| v["Si"] = json!(-129)), p(ErrCode::Parse, "Si"));
    assert_eq!(strict_err(|v| v["Udi"] = json!(-1)), p(ErrCode::Parse, "Udi"));
    assert_eq!(strict_err(|v| v["W"] = json!(1.5)), p(ErrCode::Parse, "W"));
    assert_eq!(strict_err(|v| v["Str"] = json!("123456789")), p(ErrCode::Parse, "Str"));
    assert_eq!(strict_err(|v| v["Str"] = json!("\u{ac00}")), p(ErrCode::Parse, "Str"));
    assert_eq!(strict_err(|v| v["Ch"] = json!("\u{ac00}")), p(ErrCode::Parse, "Ch"));
    assert_eq!(strict_err(|v| v["Ch"] = json!("AB")), p(ErrCode::Parse, "Ch"));
    assert_eq!(strict_err(|v| v["Flags"] = json!([true, false, true, false])), p(ErrCode::Parse, "Flags"));
    assert_eq!(strict_err(|v| v["Grid"] = json!([[1, 2, 3], [4, 5]])), p(ErrCode::Parse, "Grid[1]"));
    assert_eq!(strict_err(|v| v["Grid"] = json!([1, 2, 3, 4])), p(ErrCode::Parse, "Grid"));
    assert_eq!(strict_err(|v| v["Grid"] = json!([1, 2])), p(ErrCode::Parse, "Grid[1]"));
    assert_eq!(strict_err(|v| v["Ts"] = json!("2026-13-01T00:00:00")), p(ErrCode::Parse, "Ts"));
    assert_eq!(strict_err(|v| v["R"] = json!(null)), p(ErrCode::Parse, "R"));
    assert_eq!(strict_err(|v| v["R"] = json!(1e39)), p(ErrCode::Parse, "R"));
    assert_eq!(strict_err(|v| v["Tm"] = json!(2_147_483_648i64)), p(ErrCode::Parse, "Tm"));
    assert_eq!(strict_err(|v| v["Subs"][1]["Pos"][1] = json!("x")), p(ErrCode::Parse, "Subs[1].Pos[2]"));
    assert_eq!(strict_err(|v| *v = json!(5)), p(ErrCode::Parse, "<root>"));
    // accepted in strict mode
    let c = fixture_contract();
    let mut v = golden();
    v["Lr"] = json!(1e39);
    v["W"] = json!(7.0);
    v["Ts"] = json!("");
    v["Tm"] = json!(-5);
    v["Ch"] = json!("\u{fc}");
    assert!(json_to_udt_bytes(&c, "LNK_Fixture", &v, Strictness::Strict).is_ok());
}

#[test]
fn missing_members_are_defaulted() {
    let c = fixture_contract();
    let mut v = golden();
    let o = v.as_object_mut().unwrap();
    o.remove("Sub");
    o.remove("Tm");
    o.remove("Ts");
    v["Axis"] = json!([1.0, 2.0]);
    let (b, r) = json_to_udt_bytes(&c, "LNK_Fixture", &v, Strictness::Strict).unwrap();
    assert_eq!(r.defaulted, vec!["Ts", "Tm", "Axis[3..3]", "Sub"]);
    assert!(r.warnings.is_empty());
    let back: Value = serde_json::from_str(&udt_bytes_to_json_text(&c, "LNK_Fixture", &b).unwrap()).unwrap();
    assert_eq!(back["Sub"], json!({"Id": 0, "Name": "", "On": false, "Pos": [0.0, 0.0, 0.0, 0.0]}));
    assert_eq!(back["Tm"], json!(0));
    assert_eq!(back["Ts"], json!(DTL_EPOCH_TEXT));
    assert_eq!(back["Axis"], json!([1.0, 2.0, 0.0]));
    assert_eq!(b[94], 4, "String[4] max length byte of the defaulted Sub.Name");
}

#[test]
fn lenient_coerces_and_warns() {
    let c = fixture_contract();
    let mut v = golden();
    v["I"] = json!("5");
    v["By"] = json!(300);
    v["Str"] = json!("abcdefghijk");
    v["Ch"] = json!("\u{ac00}");
    v["R"] = json!(null);
    v["B0"] = json!(1);
    v["Nope"] = json!(true);
    v["Flags"] = json!([true, true, true, true]);
    let (b, r) = json_to_udt_bytes(&c, "LNK_Fixture", &v, Strictness::Lenient).unwrap();
    assert_eq!(r.warnings.len(), 8, "{:?}", r.warnings);
    let back: Value = serde_json::from_str(&udt_bytes_to_json_text(&c, "LNK_Fixture", &b).unwrap()).unwrap();
    assert_eq!((back["I"].clone(), back["By"].clone(), back["Str"].clone(), back["Ch"].clone()), (json!(5), json!(255), json!("abcdefgh"), json!("?")));
    assert_eq!((back["R"].clone(), back["B0"].clone(), back["Flags"].clone()), (json!(0.0), json!(true), json!([true, true, true])));
}

#[test]
fn unsupported_types_fail_with_path() {
    let mut c = fixture_contract();
    c.add_udt_source("TYPE \"Bad\"\n   STRUCT\n      Ok : Int;\n      S : Struct\n         When : Time_Of_Day;\n      END_STRUCT;\n   END_STRUCT;\nEND_TYPE\n").unwrap();
    let ty = TypeRef::Udt("Bad".into());
    for e in [
        json_to_udt_bytes(&c, "Bad", &json!({}), Strictness::Lenient).unwrap_err(),
        udt_bytes_to_json_text(&c, "Bad", &[0; 8]).unwrap_err(),
        max_json_len(&c, &ty).unwrap_err(),
        check_supported(&c, &ty).unwrap_err(),
    ] {
        assert_eq!(e.path.as_deref(), Some("S.When"), "{e}");
    }
}
