use plc_layout::{Contract, Value};
use serde_json::json;

const CONSTS: &[(&str, i64)] = &[("X", 1), ("Y", 2), ("Z", 3), ("G", 4), ("MEAS_LOG_LAST", 199)];

fn contract() -> Contract {
    let mut c = Contract::new();
    for (k, v) in CONSTS {
        c.consts.insert((*k).into(), *v);
    }
    c.add_udt_source(
        r#"TYPE "LGR_Command_Header"
   STRUCT
      Protocol : Byte;
      CMD_ID : Byte;
      CMD : Byte;
      SRC : Word;
      DST : Word;
      SEQ : Word;
   END_STRUCT;
END_TYPE"#,
    )
    .unwrap();
    c.add_udt_source(
        r#"TYPE "LGR_Stock_Item"
   STRUCT
      Code : UDInt := 0;
      Count : USInt := 0;
      InnerDiameter : Real := 0.0;
      OuterDiameter : Real := 0.0;
      LowerBidHeight : Real := 0.0;
      UpperBidHeight : Real := 0.0;
      Height : Real := 0.0;
      DeflectionFactor : Real := 0.0;
   END_STRUCT;
END_TYPE"#,
    )
    .unwrap();
    c.add_udt_source(
        r#"TYPE "LGR_Cell_Info"
   STRUCT
      Use : Bool;
      BlendUse : Bool;
      Id : UInt;
      Section : UInt;
      Row : UInt;
      Col : UInt;
      Lenth : Real;
      Width : Real;
      Position : Array["X".."Z"] of Real;
   END_STRUCT;
END_TYPE"#,
    )
    .unwrap();
    c.add_udt_source(
        r#"TYPE "LGR_Task_Data"
   STRUCT
      WorkId : UDInt;
      TaskId : UDInt;
      TaskType : Byte;
      Position : Array["X".."G"] of Real;
      Item : "LGR_Stock_Item";
      Cell : "LGR_Cell_Info";
      BlendUpDistance : UInt;
      BlendDownDistance : UInt;
      DragOutHeight : UInt;
      DragOutDist : UInt;
      DragOutDir : Byte;
      DragInHeight : UInt;
      DragInDist : UInt;
      DragInDir : Byte;
      LiftUpCreepDistance : UInt;
      LiftDownCreepDistance : UInt;
      LiftUpHeight : UInt;
      PreGripDelta : UInt;
      GripBackDelta : UInt;
      GripHeight : UInt;
      UseDragOut : Bool;
      UseDragIn : Bool;
      LiftUpAfterComplete : Bool;
      LiftUpPartial : Bool;
      MeasureFloor : Bool;
      MeasureItem : Bool;
      MeasureSku : Bool;
      AdjustCenter : Bool;
      FindStationItem : Bool;
      Avoid : Bool;
      Outbound : Bool;
   END_STRUCT;
END_TYPE"#,
    )
    .unwrap();
    c.add_udt_source(
        r#"TYPE "LGR_Command_Command"
   STRUCT
      Stop : Byte;
      Common : Byte;
      Task : Struct
         Complete : Struct
            WorkId : UDInt;
            TaskId : UDInt;
         END_STRUCT;
         Delete : Struct
            WorkId : UDInt;
            TaskId : UDInt;
         END_STRUCT;
      END_STRUCT;
      Jog : Byte;
      B3_Spare : Byte;
      Home : Byte;
      MaintPos : Byte;
      CellOrigin : Byte;
      ChangeMode : Byte;
   END_STRUCT;
END_TYPE"#,
    )
    .unwrap();
    c.add_udt_source(
        r#"TYPE "LGR_Command_Response"
   STRUCT
      Header : "LGR_Command_Header";
      Command : "LGR_Command_Command";
      Task : "LGR_Task_Data";
      Data : Array[0..15] of Byte;
   END_STRUCT;
END_TYPE"#,
    )
    .unwrap();
    c.add_udt_source(
        r#"TYPE "LGR_MeasureLog"
   STRUCT
      TimeStamp {InstructionName := 'DTL'; LibVersion := '1.0'} : DTL;
      Seq : UDInt;
      Kind : USInt;
      Status : UInt;
      Cmd : "LGR_Task_Data";
      Data : Array[0..19] of Real;
      Delta : Struct
         InnerDia : Real;
         Height : Real;
         Z : Real;
         Offset : Real;
         Count : Real;
      END_STRUCT;
   END_STRUCT;
END_TYPE"#,
    )
    .unwrap();
    c.add_db_source(
        r#"DATA_BLOCK "HIST"
{ S7_Optimized_Access := 'FALSE' }
NON_RETAIN
   VAR RETAIN
      LayoutSig : DWord;
      Head : Int;
      Flags : Array[0..9] of Bool;
      Small : Array[0..2] of Byte;
      Entry : Array[0.."MEAS_LOG_LAST"] of "LGR_MeasureLog";
   END_VAR
BEGIN
END_DATA_BLOCK"#,
    )
    .unwrap();
    c
}

#[test]
fn reference_sizes() {
    let c = contract();
    assert_eq!(c.size_of_udt("LGR_Command_Header").unwrap(), 10);
    assert_eq!(c.size_of_udt("LGR_Stock_Item").unwrap(), 30);
    assert_eq!(c.size_of_udt("LGR_Cell_Info").unwrap(), 30);
    assert_eq!(c.size_of_udt("LGR_Task_Data").unwrap(), 116);
    assert_eq!(c.size_of_udt("LGR_Command_Command").unwrap(), 24);
    assert_eq!(c.size_of_udt("LGR_Command_Response").unwrap(), 166);
    assert_eq!(c.size_of_udt("LGR_MeasureLog").unwrap(), 236);
}

#[test]
fn offsets_and_bool_packing() {
    let c = contract();
    let l = c.layout_udt("LGR_Task_Data").unwrap();
    let m = |p: &str| l.find(p).unwrap_or_else(|| panic!("{p}"));
    assert_eq!(m("TaskType").offset, 8);
    assert_eq!(m("Position[1]").offset, 10);
    assert_eq!(m("Position[4]").offset, 22);
    assert_eq!(m("Item.Code").offset, 26);
    assert_eq!(m("Item.Count").offset, 30);
    assert_eq!(m("Item.InnerDiameter").offset, 32); // word aligned after USInt
    assert_eq!(m("Cell.Use").offset, 56);
    assert_eq!(m("Cell.Use").bit, Some(0));
    assert_eq!(m("Cell.BlendUse").bit, Some(1));
    assert_eq!(m("Cell.Id").offset, 58);
    assert_eq!(m("Cell.Position[3]").offset, 82);
    assert_eq!(m("DragOutDir").offset, 94);
    assert_eq!(m("DragInHeight").offset, 96);
    assert_eq!(m("GripHeight").offset, 112);
    assert_eq!((m("UseDragOut").offset, m("UseDragOut").bit), (114, Some(0)));
    assert_eq!((m("Outbound").offset, m("Outbound").bit), (115, Some(2)));

    let h = c.layout_db("HIST").unwrap();
    let m = |p: &str| h.find(p).unwrap_or_else(|| panic!("{p}"));
    assert_eq!(m("LayoutSig").offset, 0);
    assert_eq!(m("Head").offset, 4);
    assert_eq!((m("Flags[0]").offset, m("Flags[9]").offset, m("Flags[9]").bit), (6, 7, Some(1)));
    assert_eq!(m("Small[0]").offset, 8); // array of bool padded to word, then array of byte word aligned
    assert_eq!(m("Small[2]").offset, 10);
    assert_eq!(m("Entry[0].Seq").offset, 12 + 12);
    assert_eq!(m("Entry[1].TimeStamp").offset, 12 + 236);
    assert_eq!(h.size, 12 + 200 * 236);
    assert_eq!(h.range_of("Entry[3]"), Some((12 + 3 * 236, 12 + 4 * 236)));
}

#[test]
fn decode_encode_roundtrip_and_paths() {
    let c = contract();
    let l = c.layout_db("HIST").unwrap();
    let mut buf = vec![0u8; l.size as usize];
    let entry = json!({
        "TimeStamp": "2026-09-12 10:11:12.345", "Seq": 77, "Kind": 4, "Status": 2,
        "Cmd": {"WorkId": 5001, "TaskId": 3, "TaskType": 0x41, "Position": [12000.5, 3000.0, 1500.25, 300.0],
                 "Item": {"Code": 1001, "Count": 4, "InnerDiameter": 381.0, "Height": 240.0},
                 "Cell": {"Use": true, "BlendUse": false, "Id": 105, "Position": [1.0, 2.0, 3.0]},
                 "GripHeight": 40, "MeasureItem": true, "Outbound": true},
        "Data": [1.5, 2.5], "Delta": {"Z": -3.25}
    });
    c.encode_path("HIST", "Entry[3]", &entry, &mut buf).unwrap();
    c.encode_path("HIST", "Head", &json!(4), &mut buf).unwrap();
    c.encode_path("HIST", "Flags[9]", &json!(true), &mut buf).unwrap();

    let head = c.decode_path("HIST", "Head", &buf).unwrap();
    assert_eq!(head, json!(4));
    let e3 = c.decode_path("HIST", "Entry[3]", &buf).unwrap();
    assert_eq!(e3["Seq"], json!(77));
    assert_eq!(e3["TimeStamp"], json!("2026-09-12 10:11:12.345"));
    assert_eq!(e3["Cmd"]["Position"][2], json!(1500.25));
    assert_eq!(e3["Cmd"]["Cell"]["Use"], json!(true));
    assert_eq!(e3["Cmd"]["Cell"]["Id"], json!(105));
    assert_eq!(e3["Cmd"]["MeasureItem"], json!(true));
    assert_eq!(e3["Cmd"]["Outbound"], json!(true));
    assert_eq!(e3["Cmd"]["Avoid"], json!(false));
    assert_eq!(e3["Data"][1], json!(2.5));
    assert_eq!(e3["Delta"]["Z"], json!(-3.25));
    let whole = c.decode_db("HIST", &buf).unwrap();
    assert_eq!(whole["Flags"][9], json!(true));
    assert_eq!(whole["Entry"][2]["Seq"], json!(0));
    // member-level
    let m = c.resolve("HIST", "Entry[3].Cmd.Cell.Id").unwrap();
    assert_eq!(plc_layout::decode::decode_member(&buf, &m).unwrap(), Value::U64(105));
}

#[test]
fn signature_is_stable_and_sensitive() {
    let c = contract();
    let s1 = c.layout_sig("HIST").unwrap();
    let mut c2 = contract();
    // comment / attribute / init changes do not alter the signature
    c2.add_db_source("DATA_BLOCK \"HIST\"\n{ S7_Optimized_Access := 'FALSE' }\n   VAR RETAIN\n      LayoutSig { S7_SetPoint := 'True'} : DWord := 16#1234;   // x\n      Head : Int; // y\n      Flags : Array[0..9] of Bool;\n      Small : Array[0..2] of Byte;\n      Entry : Array[0..199] of \"LGR_MeasureLog\";\n   END_VAR\nBEGIN\n   Head := 9;\nEND_DATA_BLOCK\n").unwrap();
    assert_eq!(c2.layout_sig("HIST").unwrap(), s1);
    // renaming or reordering does
    let mut c3 = contract();
    c3.add_db_source("DATA_BLOCK \"HIST\"\n{ S7_Optimized_Access := 'FALSE' }\n   VAR RETAIN\n      LayoutSig : DWord;\n      Flags : Array[0..9] of Bool;\n      Head : Int;\n      Small : Array[0..2] of Byte;\n      Entry : Array[0..199] of \"LGR_MeasureLog\";\n   END_VAR\nEND_DATA_BLOCK\n").unwrap();
    assert_ne!(c3.layout_sig("HIST").unwrap(), s1);
}

#[test]
fn udt_sig_equals_db_with_same_fields() {
    let mut c = contract();
    c.add_db_source("DATA_BLOCK \"HDR\"\n{ S7_Optimized_Access := 'FALSE' }\n   VAR\n      Protocol : Byte;\n      CMD_ID : Byte;\n      CMD : Byte;\n      SRC : Word;\n      DST : Word;\n      SEQ : Word;\n   END_VAR\nBEGIN\nEND_DATA_BLOCK\n").unwrap();
    assert_eq!(c.udt_sig("LGR_Command_Header").unwrap(), c.layout_sig("HDR").unwrap());
    assert_eq!(plc_layout::signature::canonical_udt(&c, "LGR_Command_Header").unwrap(), "{Protocol:Byte;CMD_ID:Byte;CMD:Byte;SRC:Word;DST:Word;SEQ:Word;}");
    assert!(c.udt_sig("NOPE").is_err());
}

#[test]
fn udt_sig_follows_nested_renames_but_not_comments() {
    let c = contract();
    let resp = c.udt_sig("LGR_Command_Response").unwrap();
    // comments, attributes and start values do not matter
    let mut c2 = contract();
    c2.add_udt_source("TYPE \"LGR_Command_Header\"\nVERSION : 0.1\n   STRUCT\n      Protocol { S7_SetPoint := 'False'} : Byte := 16#01;   // x\n      CMD_ID : Byte;\n      CMD : Byte;   // y\n      SRC : Word;\n      DST : Word;\n      SEQ : Word;\n   END_STRUCT;\nEND_TYPE\n").unwrap();
    assert_eq!(c2.udt_sig("LGR_Command_Response").unwrap(), resp);
    // renaming a member of a nested UDT does
    let mut c3 = contract();
    c3.add_udt_source("TYPE \"LGR_Command_Header\"\n   STRUCT\n      Protocol : Byte;\n      CMD_ID : Byte;\n      CMD : Byte;\n      Source : Word;\n      DST : Word;\n      SEQ : Word;\n   END_STRUCT;\nEND_TYPE\n").unwrap();
    assert_ne!(c3.udt_sig("LGR_Command_Response").unwrap(), resp);
}

#[test]
fn dtl_weekday_and_separators() {
    use plc_layout::Prim;
    use plc_layout::encode::{dtl_weekday, encode_prim};
    assert_eq!(dtl_weekday(1970, 1, 1), 5); // Thursday
    assert_eq!(dtl_weekday(2026, 9, 13), 1); // Sunday
    assert_eq!(dtl_weekday(2026, 9, 12), 7); // Saturday
    assert_eq!(dtl_weekday(2000, 2, 29), 3); // Tuesday
    assert_eq!(dtl_weekday(2026, 0, 1), 0);
    let mut b = [0u8; 12];
    encode_prim(&mut b, Prim::Dtl, 0, None, &Value::Str("2026-09-13T10:11:12.5".into()), "t").unwrap();
    assert_eq!(b, [0x07, 0xEA, 9, 13, 1, 10, 11, 12, 0x1D, 0xCD, 0x65, 0x00]);
    encode_prim(&mut b, Prim::Dtl, 0, None, &Value::Str("2026-09-12 10:11:12.345".into()), "t").unwrap();
    assert_eq!(b[4], 7);
    assert_eq!(u32::from_be_bytes([b[8], b[9], b[10], b[11]]), 345_000_000);
    encode_prim(&mut b, Prim::Dtl, 0, None, &Value::Str("2026-09-13T00:00:00.123456789".into()), "t").unwrap();
    assert_eq!(u32::from_be_bytes([b[8], b[9], b[10], b[11]]), 123_456_789);
    encode_prim(&mut b, Prim::Dtl, 0, None, &Value::Str("2026-09-13T00:00:00".into()), "t").unwrap();
    assert_eq!((b[4], u32::from_be_bytes([b[8], b[9], b[10], b[11]])), (1, 0));
    assert_eq!(plc_layout::decode::decode_prim(&b, Prim::Dtl, 0, None).unwrap(), Value::Str("2026-09-13 00:00:00.000".into()));
}

#[test]
fn string_and_char_are_latin1() {
    use plc_layout::Prim;
    use plc_layout::encode::encode_prim;
    let mut b = [0xFFu8; 10];
    encode_prim(&mut b, Prim::String(8), 0, None, &Value::Str("\u{e9}\u{ac00}A".into()), "s").unwrap();
    assert_eq!(b, [8, 3, 0xE9, b'?', b'A', 0, 0, 0, 0, 0]);
    let mut c = [0u8; 1];
    encode_prim(&mut c, Prim::Char, 0, None, &Value::Str("\u{fc}x".into()), "c").unwrap();
    assert_eq!(c[0], 0xFC);
    encode_prim(&mut c, Prim::Char, 0, None, &Value::Str("\u{ac00}".into()), "c").unwrap();
    assert_eq!(c[0], b'?');
}
