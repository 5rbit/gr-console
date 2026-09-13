//! JSON envelope `{"type":"<Name>","seq":<u16>,"sig":<u32>,"data":<value>}` (wire-spec section 2).

use serde_json::Value;

use crate::error::LinkError;
use crate::wire_json::push_name;

/// Writes the envelope with no whitespace, keys in spec order. `data_text` is already wire JSON text;
/// `None` writes `null` (Heartbeat).
pub fn write_envelope(name: &str, seq: u16, sig: u32, data_text: Option<&str>) -> String {
    let data = data_text.unwrap_or("null");
    let mut s = String::with_capacity(40 + name.len() + data.len());
    s.push_str("{\"type\":\"");
    push_name(&mut s, name);
    s.push_str("\",\"seq\":");
    s.push_str(&seq.to_string());
    s.push_str(",\"sig\":");
    s.push_str(&sig.to_string());
    s.push_str(",\"data\":");
    s.push_str(data);
    s.push('}');
    s
}

/// Worst-case envelope length for a message name and a worst-case data text length.
pub fn envelope_max_len(name: &str, data_max: usize) -> usize {
    let mut n = String::new();
    push_name(&mut n, name);
    // {"type":"  ","seq":  ,"sig":  ,"data":  }
    9 + n.len() + 8 + 5 + 7 + 10 + 8 + data_max + 1
}

/// A parsed envelope. `type` is either a name (`type_name`) or a numeric id (`type_id`).
#[derive(Clone, Debug, PartialEq)]
pub struct Envelope {
    pub type_name: Option<String>,
    pub type_id: Option<u16>,
    pub seq: u16,
    pub sig: u32,
    pub data: Value,
}

fn uint(m: &serde_json::Map<String, Value>, key: &str, max: u64) -> Result<u64, LinkError> {
    match m.get(key) {
        None | Some(Value::Null) => Ok(0),
        Some(Value::Number(n)) => n.as_u64().filter(|x| *x <= max).ok_or_else(|| LinkError::at(crate::ErrCode::Parse, key, format!("\"{key}\" must be an integer 0..{max}"))),
        Some(other) => Err(LinkError::at(crate::ErrCode::Parse, key, format!("\"{key}\" must be a number, got {other}"))),
    }
}

/// Parses an envelope: any key order, unknown keys ignored, `type` as string or number, missing
/// `seq`/`sig` → 0, missing `data` → null. Leading/trailing whitespace (incl. `\r`) is allowed.
pub fn parse_envelope(bytes: &[u8]) -> Result<Envelope, LinkError> {
    let v: Value = serde_json::from_slice(bytes).map_err(|e| LinkError::parse(format!("envelope is not valid JSON: {e}")))?;
    let Value::Object(mut m) = v else {
        return Err(LinkError::parse("envelope is not a JSON object"));
    };
    let (type_name, type_id) = match m.get("type") {
        Some(Value::String(s)) => (Some(s.clone()), None),
        Some(Value::Number(n)) => {
            let id = n.as_u64().filter(|x| *x <= u16::MAX as u64).ok_or_else(|| LinkError::at(crate::ErrCode::Parse, "type", "numeric type id out of range"))?;
            (None, Some(id as u16))
        }
        Some(other) => return Err(LinkError::at(crate::ErrCode::Parse, "type", format!("\"type\" must be a name or id, got {other}"))),
        None => return Err(LinkError::parse("envelope without \"type\"")),
    };
    let seq = uint(&m, "seq", u16::MAX as u64)? as u16;
    let sig = uint(&m, "sig", u32::MAX as u64)? as u32;
    let data = m.remove("data").unwrap_or(Value::Null);
    Ok(Envelope { type_name, type_id, seq, sig, data })
}
