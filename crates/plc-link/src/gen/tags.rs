//! `LNK_Const_Gen` PLC constant table (SimaticML tag table, same shape and cultures as a TIA export).

use serde::Serialize;

use crate::error::ErrCode;
use crate::header::{Format, HEADER_LEN, MAGIC, VERSION};
use crate::registry::{MessageSpec, Registry};

/// One user constant of the generated table.
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct ConstDef {
    pub name: String,
    pub data_type: &'static str,
    /// SCL literal as written to the table (`16#4753`, `8176`).
    pub value: String,
    pub comment_en: String,
    pub comment_ko: String,
}

fn def(name: String, data_type: &'static str, value: String, en: impl Into<String>, ko: impl Into<String>) -> ConstDef {
    ConstDef { name, data_type, value, comment_en: en.into(), comment_ko: ko.into() }
}

fn err_text(name: &str) -> (&str, &str) {
    match name {
        "OK" => ("OK", "정상"),
        "BAD_MAGIC" => ("bad FRAME magic", "FRAME 매직 불일치"),
        "BAD_VERSION" => ("bad FRAME version", "FRAME 버전 불일치"),
        "UNKNOWN_TYPE" => ("unknown message type", "알 수 없는 메시지"),
        "SIG_MISMATCH" => ("BIN layout signature mismatch", "BIN 시그니처 불일치"),
        "BAD_LENGTH" => ("bad length", "길이 오류"),
        "PARSE" => ("parse error", "파싱 오류"),
        "FORMAT_NOT_ALLOWED" => ("format not allowed (BIN over NDJSON)", "허용되지 않는 포맷 (NDJSON 위 BIN)"),
        "DIR_NOT_ALLOWED" => ("direction not allowed", "허용되지 않는 방향"),
        "BUSY" => ("busy", "처리 중"),
        "NO_PENDING" => ("nothing pending", "대기 항목 없음"),
        other => (other, other),
    }
}

/// Constants in table order: header, message ids (all registry messages), then signature / size / worst-case JSON
/// length of every generated payload message, registry hash, wire error codes (runtime codes >= 100 excluded).
pub(crate) fn constants(r: &Registry, msgs: &[&MessageSpec]) -> Vec<ConstDef> {
    let d = &r.doc;
    let p = &d.const_prefix;
    let mut out = vec![
        def(format!("{p}MAGIC"), "Word", format!("16#{MAGIC:04X}"), "FRAME header magic 'GS'", "FRAME 헤더 매직 'GS'"),
        def(format!("{p}VERSION"), "USInt", VERSION.to_string(), "FRAME header version", "FRAME 헤더 버전"),
        def(format!("{p}FMT_JSON"), "USInt", Format::Json.code().to_string(), "Payload format JSON", "페이로드 포맷 JSON"),
        def(format!("{p}FMT_BIN"), "USInt", Format::Bin.code().to_string(), "Payload format BIN (Serialize)", "페이로드 포맷 BIN (Serialize)"),
        def(format!("{p}HDR_LEN"), "DInt", HEADER_LEN.to_string(), "FRAME header length (bytes)", "FRAME 헤더 길이 (B)"),
        def(format!("{p}MAX_PAYLOAD"), "DInt", d.plc_max_payload.to_string(), "PLC receive payload / line max (bytes)", "PLC 수신 페이로드 / 줄 최대 (B)"),
    ];
    for m in &r.messages {
        let (en, ko) = match &m.udt {
            Some(u) => (format!("Message id {} ({u})", m.name), format!("메시지 ID {}", m.name)),
            None => (format!("Message id {} (no payload)", m.name), format!("메시지 ID {} (페이로드 없음)", m.name)),
        };
        out.push(def(d.msg_const(&m.name), "UInt", m.id.to_string(), en, ko));
    }
    let payload: Vec<(&MessageSpec, &str)> = msgs.iter().filter_map(|m| m.udt.as_deref().map(|u| (*m, u))).collect();
    let upper = |m: &MessageSpec| m.name.to_ascii_uppercase();
    for (m, u) in &payload {
        out.push(def(format!("{p}SIG_{}", upper(m)), "DWord", format!("16#{:08X}", m.sig), format!("UDT signature {u}"), format!("UDT 시그니처 {u}")));
    }
    for (m, u) in &payload {
        out.push(def(format!("{p}LEN_{}", upper(m)), "DInt", m.size.to_string(), format!("Serialized size {u} (bytes)"), format!("직렬화 크기 {u} (B)")));
    }
    for (m, _) in &payload {
        out.push(def(format!("{p}JMAX_{}", upper(m)), "DInt", m.json_max.to_string(), format!("Max JSON envelope length {} (bytes)", m.name), format!("JSON 엔벨로프 최대 길이 {} (B)", m.name)));
    }
    out.push(def(format!("{p}REGISTRY_HASH"), "DWord", format!("16#{:08X}", r.hash()), "Message registry hash (crc32, wire-spec 5)", "메시지 레지스트리 해시 (crc32)"));
    let mut errors: Vec<(i16, String)> =
        if d.errors.is_empty() { ErrCode::ALL.iter().map(|e| (e.code(), e.name().to_string())).collect() } else { d.errors.iter().filter(|e| !e.runtime).map(|e| (e.code, e.name.clone())).collect() };
    errors.sort_by_key(|e| e.0);
    for (code, name) in errors {
        let (en, ko) = err_text(&name);
        out.push(def(d.err_const(&name), "Int", code.to_string(), format!("Link error: {en}"), format!("링크 오류: {ko}")));
    }
    out
}

fn esc(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            c => out.push(c),
        }
    }
    out
}

/// SimaticML document of the table (IDs sequential hex, comment cultures en-US / hu-HU / ko-KR).
pub(crate) fn render_xml(table: &str, consts: &[ConstDef]) -> String {
    let mut s = String::new();
    s.push_str("<?xml version=\"1.0\" encoding=\"utf-8\"?>\n<Document>\n  <Engineering version=\"V20\" />\n  <SW.Tags.PlcTagTable ID=\"0\">\n    <AttributeList>\n");
    s.push_str(&format!("      <Name>{}</Name>\n", esc(table)));
    s.push_str("    </AttributeList>\n    <ObjectList>\n");
    let mut id = 1u32;
    for k in consts {
        s.push_str(&format!("      <SW.Tags.PlcUserConstant ID=\"{id:X}\" CompositionName=\"UserConstants\">\n        <AttributeList>\n"));
        s.push_str(&format!("          <DataTypeName>{}</DataTypeName>\n", k.data_type));
        s.push_str(&format!("          <Name>{}</Name>\n", esc(&k.name)));
        s.push_str(&format!("          <Value>{}</Value>\n", esc(&k.value)));
        s.push_str("        </AttributeList>\n        <ObjectList>\n");
        s.push_str(&format!("          <MultilingualText ID=\"{:X}\" CompositionName=\"Comment\">\n            <ObjectList>\n", id + 1));
        for (i, (culture, text)) in [("en-US", k.comment_en.as_str()), ("hu-HU", ""), ("ko-KR", k.comment_ko.as_str())].into_iter().enumerate() {
            s.push_str(&format!("              <MultilingualTextItem ID=\"{:X}\" CompositionName=\"Items\">\n                <AttributeList>\n", id + 2 + i as u32));
            s.push_str(&format!("                  <Culture>{culture}</Culture>\n"));
            if text.is_empty() {
                s.push_str("                  <Text />\n");
            } else {
                s.push_str(&format!("                  <Text>{}</Text>\n", esc(text)));
            }
            s.push_str("                </AttributeList>\n              </MultilingualTextItem>\n");
        }
        s.push_str("            </ObjectList>\n          </MultilingualText>\n        </ObjectList>\n      </SW.Tags.PlcUserConstant>\n");
        id += 5;
    }
    s.push_str("    </ObjectList>\n  </SW.Tags.PlcTagTable>\n</Document>\n");
    s
}
