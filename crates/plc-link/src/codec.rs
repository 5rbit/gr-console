//! Frames <-> canonical in-memory messages. Every received message is normalized to its UDT bytes.

use std::sync::Arc;

use serde::Serialize;
use serde_json::{Value, json};

use plc_layout::Contract;

use crate::envelope::{parse_envelope, write_envelope};
use crate::error::{ErrCode, LinkError};
use crate::framing::http::mime;
use crate::framing::{ContentLength, Framing, RawFrame, encode_frame, encode_ndjson, write_request, write_response};
use crate::header::Format;
use crate::registry::{MessageSpec, Registry};
use crate::wire_json::{Report, Strictness, json_to_udt_bytes, udt_bytes_to_json_text};

/// The receiving side of a message (for direction checks).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Role {
    Pc,
    Plc,
}

/// Canonical message: id, sequence number and the UDT bytes (empty for messages without payload).
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
pub struct Message {
    pub id: u16,
    pub seq: u16,
    pub payload: Vec<u8>,
}

/// HTTP header lines (name, value) and body.
pub type HttpParts = (Vec<(String, String)>, Vec<u8>);

/// Next sender sequence number: 1, 2, … 65535, 1 (0 = none).
pub fn next_seq(seq: u16) -> u16 {
    if seq == u16::MAX { 1 } else { seq + 1 }
}

#[derive(Clone, Debug)]
pub struct Codec {
    contract: Arc<Contract>,
    registry: Arc<Registry>,
    content_length: ContentLength,
}

fn unknown(msg: impl Into<String>) -> LinkError {
    LinkError::new(ErrCode::UnknownType, msg)
}

impl Codec {
    pub fn new(contract: Arc<Contract>, registry: Arc<Registry>) -> Self {
        Codec { contract, registry, content_length: ContentLength::Plain }
    }

    /// Content-Length style of the HTTP writers (`Pad5` = exactly what the PLC writes).
    pub fn with_content_length(mut self, cl: ContentLength) -> Self {
        self.content_length = cl;
        self
    }

    pub fn contract(&self) -> &Contract {
        &self.contract
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    fn available<'r>(&self, spec: Option<&'r MessageSpec>, what: &str) -> Result<&'r MessageSpec, LinkError> {
        let spec = spec.ok_or_else(|| unknown(format!("unknown message type {what}")))?;
        if !spec.available {
            return Err(unknown(format!("message {} is not available: {}", spec.name, spec.unavailable_reason.clone().unwrap_or_default())));
        }
        Ok(spec)
    }

    /// Available message by id.
    pub fn spec(&self, id: u16) -> Result<&MessageSpec, LinkError> {
        self.available(self.registry.by_id(id), &id.to_string())
    }

    /// Available message by name (case-insensitive).
    pub fn spec_by_name(&self, name: &str) -> Result<&MessageSpec, LinkError> {
        self.available(self.registry.by_name(name), name)
    }

    fn check_route(spec: &MessageSpec, receiver: Role, format: Format) -> Result<(), LinkError> {
        if !spec.receivable_by(receiver) {
            return Err(LinkError::new(ErrCode::DirNotAllowed, format!("{} ({:?}) is not accepted by the {receiver:?}", spec.name, spec.dir)));
        }
        if !spec.allows(format) {
            return Err(LinkError::new(ErrCode::FormatNotAllowed, format!("{} does not allow format {}", spec.name, format.name())));
        }
        Ok(())
    }

    /// Type named by `X-GR-Type` (name or id), if any.
    fn http_type(&self, raw: &RawFrame) -> Result<Option<&MessageSpec>, LinkError> {
        match raw.http.as_ref().and_then(|h| h.header("X-GR-Type")) {
            Some(t) => Ok(Some(self.available(self.registry.lookup(t), t)?)),
            None => Ok(None),
        }
    }

    pub fn decode_raw(&self, raw: &RawFrame, receiver: Role, strictness: Strictness) -> Result<Message, LinkError> {
        self.decode_raw_report(raw, receiver, strictness).map(|(m, _)| m)
    }

    /// Validates a raw frame (type, direction, format, signature, length, header/envelope consistency) and
    /// converts it to a [`Message`]; the report lists defaulted members and lenient coercions of JSON input.
    pub fn decode_raw_report(&self, raw: &RawFrame, receiver: Role, strictness: Strictness) -> Result<(Message, Report), LinkError> {
        if raw.framing == Framing::Ndjson && raw.format == Format::Bin {
            return Err(LinkError::new(ErrCode::FormatNotAllowed, "NDJSON carries JSON only"));
        }
        match raw.format {
            Format::Bin => {
                let spec = match raw.msg_type {
                    Some(id) => self.spec(id)?,
                    None => match self.http_type(raw)? {
                        Some(s) => s,
                        None => {
                            let seg = raw.http.as_ref().and_then(|h| h.path.as_deref()).map(|p| p.split('?').next().unwrap_or(p).rsplit('/').next().unwrap_or(""));
                            let seg = seg.unwrap_or("");
                            self.available(self.registry.by_http_name(seg), if seg.is_empty() { "(none)" } else { seg })?
                        }
                    },
                };
                Self::check_route(spec, receiver, Format::Bin)?;
                let sig = raw.sig.unwrap_or(0);
                if sig != spec.sig {
                    return Err(LinkError::new(ErrCode::SigMismatch, format!("{}: signature 0x{sig:08X}, expected 0x{:08X}", spec.name, spec.sig)));
                }
                if raw.payload.len() != spec.size as usize {
                    return Err(LinkError::new(ErrCode::BadLength, format!("{}: {} payload bytes, expected {}", spec.name, raw.payload.len(), spec.size)));
                }
                Ok((Message { id: spec.id, seq: raw.seq.unwrap_or(0), payload: raw.payload.clone() }, Report::default()))
            }
            Format::Json => {
                let env = parse_envelope(&raw.payload)?;
                let spec = match (&env.type_name, env.type_id) {
                    (Some(n), _) => self.available(self.registry.by_name(n), n)?,
                    (None, Some(id)) => self.spec(id)?,
                    (None, None) => return Err(unknown("envelope without type")),
                };
                if let Some(t) = raw.msg_type
                    && t != spec.id
                {
                    return Err(LinkError::parse(format!("header type {t} but envelope type {} ({})", spec.name, spec.id)));
                }
                if let Some(h) = self.http_type(raw)?
                    && h.id != spec.id
                {
                    return Err(LinkError::parse(format!("X-GR-Type {} but envelope type {}", h.name, spec.name)));
                }
                if let Some(s) = raw.seq
                    && s != env.seq
                {
                    return Err(LinkError::parse(format!("header seq {s} but envelope seq {}", env.seq)));
                }
                Self::check_route(spec, receiver, Format::Json)?;
                let mut report = Report::default();
                if env.sig != 0 && env.sig != spec.sig {
                    let msg = format!("{}: envelope sig {} but expected {}", spec.name, env.sig, spec.sig);
                    if strictness == Strictness::Strict {
                        return Err(LinkError::new(ErrCode::SigMismatch, msg));
                    }
                    report.warnings.push(msg);
                }
                let payload = self.data_bytes(spec, &env.data, strictness, &mut report)?;
                Ok((Message { id: spec.id, seq: env.seq, payload }, report))
            }
        }
    }

    fn data_bytes(&self, spec: &MessageSpec, data: &Value, strictness: Strictness, report: &mut Report) -> Result<Vec<u8>, LinkError> {
        match &spec.udt {
            Some(udt) => {
                let (bytes, r) = json_to_udt_bytes(&self.contract, udt, data, strictness)?;
                report.defaulted.extend(r.defaulted);
                report.warnings.extend(r.warnings);
                Ok(bytes)
            }
            None => {
                if !data.is_null() {
                    if strictness == Strictness::Strict {
                        return Err(LinkError::at(ErrCode::Parse, "data", format!("{} has no payload, data must be null", spec.name)));
                    }
                    report.warnings.push(format!("data: {} has no payload, data ignored", spec.name));
                }
                Ok(Vec::new())
            }
        }
    }

    fn check_payload(spec: &MessageSpec, m: &Message) -> Result<(), LinkError> {
        if m.payload.len() != spec.size as usize {
            return Err(LinkError::new(ErrCode::BadLength, format!("{}: {} payload bytes, expected {}", spec.name, m.payload.len(), spec.size)));
        }
        Ok(())
    }

    /// Wire JSON text of the message data (`null` without payload).
    pub fn data_json_text(&self, m: &Message) -> Result<String, LinkError> {
        let spec = self.spec(m.id)?;
        Self::check_payload(spec, m)?;
        match &spec.udt {
            Some(udt) => udt_bytes_to_json_text(&self.contract, udt, &m.payload),
            None => Ok("null".to_string()),
        }
    }

    /// Message data as a JSON value.
    pub fn data_value(&self, m: &Message) -> Result<Value, LinkError> {
        serde_json::from_str(&self.data_json_text(m)?).map_err(|e| LinkError::parse(e.to_string()))
    }

    /// The envelope text.
    pub fn json_text(&self, m: &Message) -> Result<String, LinkError> {
        let spec = self.spec(m.id)?;
        let data = self.data_json_text(m)?;
        Ok(write_envelope(&spec.name, m.seq, spec.sig, Some(&data)))
    }

    /// Message → FRAME bytes (JSON: header sig 0, payload = envelope; BIN: header sig = UDT signature).
    pub fn to_frame(&self, m: &Message, format: Format) -> Result<Vec<u8>, LinkError> {
        let spec = self.spec(m.id)?;
        if !spec.allows(format) {
            return Err(LinkError::new(ErrCode::FormatNotAllowed, format!("{} does not allow format {}", spec.name, format.name())));
        }
        match format {
            Format::Json => Ok(encode_frame(Format::Json, m.id, m.seq, 0, self.json_text(m)?.as_bytes())),
            Format::Bin => {
                Self::check_payload(spec, m)?;
                Ok(encode_frame(Format::Bin, m.id, m.seq, spec.sig, &m.payload))
            }
        }
    }

    /// Message → NDJSON line.
    pub fn to_ndjson(&self, m: &Message) -> Result<Vec<u8>, LinkError> {
        let spec = self.spec(m.id)?;
        if !spec.allows(Format::Json) {
            return Err(LinkError::new(ErrCode::FormatNotAllowed, format!("{} does not allow JSON", spec.name)));
        }
        Ok(encode_ndjson(&self.json_text(m)?))
    }

    /// Headers (in PLC order) and body of an HTTP message.
    pub fn http_parts(&self, m: &Message, format: Format) -> Result<HttpParts, LinkError> {
        let spec = self.spec(m.id)?;
        if !spec.allows(format) {
            return Err(LinkError::new(ErrCode::FormatNotAllowed, format!("{} does not allow format {}", spec.name, format.name())));
        }
        let mut headers = vec![("Content-Type".to_string(), mime(format).to_string())];
        let body = match format {
            Format::Json => self.json_text(m)?.into_bytes(),
            Format::Bin => {
                Self::check_payload(spec, m)?;
                headers.push(("X-GR-Type".to_string(), spec.name.clone()));
                headers.push(("X-GR-Seq".to_string(), m.seq.to_string()));
                headers.push(("X-GR-Sig".to_string(), format!("0x{:08X}", spec.sig)));
                m.payload.clone()
            }
        };
        Ok((headers, body))
    }

    pub fn to_http_request(&self, m: &Message, format: Format, method: &str, path: &str, host: &str) -> Result<Vec<u8>, LinkError> {
        let (headers, body) = self.http_parts(m, format)?;
        let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
        Ok(write_request(method, path, host, &h, Some(&body), self.content_length))
    }

    /// Response with an optional message body (`None` → no body, e.g. 204).
    pub fn to_http_response(&self, status: u16, m: Option<&Message>, format: Format) -> Result<Vec<u8>, LinkError> {
        match m {
            Some(m) => {
                let (headers, body) = self.http_parts(m, format)?;
                let h: Vec<(&str, &str)> = headers.iter().map(|(k, v)| (k.as_str(), v.as_str())).collect();
                Ok(write_response(status, &h, Some(&body), self.content_length))
            }
            None => Ok(write_response(status, &[], None, self.content_length)),
        }
    }

    /// Message (seq 0) from a JSON data value.
    pub fn from_json_data(&self, name: &str, data: &Value, strictness: Strictness) -> Result<(Message, Report), LinkError> {
        let spec = self.spec_by_name(name)?;
        let mut report = Report::default();
        let payload = self.data_bytes(spec, data, strictness, &mut report)?;
        Ok((Message { id: spec.id, seq: 0, payload }, report))
    }

    fn require_members(&self, spec: &MessageSpec, members: &[&str]) -> Result<(), LinkError> {
        let udt = spec.udt.as_deref().ok_or_else(|| unknown(format!("{} has no payload UDT", spec.name)))?;
        let decl = self.contract.udt(udt).map_err(|_| unknown(format!("UDT {udt} missing")))?;
        for k in members {
            if !decl.fields.iter().any(|f| f.name == *k) {
                return Err(unknown(format!("UDT {udt} has no member {k}")));
            }
        }
        Ok(())
    }

    /// Ack message (`RefType`, `RefSeq`, `Code`, `Text` truncated to the declared length). Requires an
    /// available `Ack` message.
    pub fn ack(&self, ref_type: u16, ref_seq: u16, code: i16, text: &str, seq: u16) -> Result<Message, LinkError> {
        let spec = self.spec_by_name("Ack")?;
        self.require_members(spec, &["RefType", "RefSeq", "Code", "Text"])?;
        let v = json!({"RefType": ref_type, "RefSeq": ref_seq, "Code": code, "Text": text});
        let (mut m, _) = self.from_json_data("Ack", &v, Strictness::Lenient)?;
        m.seq = seq;
        Ok(m)
    }

    /// Hello message carrying this registry's hash and heartbeat interval. `formats`: bit0 JSON, bit1 BIN.
    pub fn hello(&self, plc: &str, formats: u8, seq: u16) -> Result<Message, LinkError> {
        let spec = self.spec_by_name("Hello")?;
        self.require_members(spec, &["Plc", "Proto", "Formats", "RegistryHash", "HeartbeatMs"])?;
        let doc = &self.registry.doc;
        let v = json!({"Plc": plc, "Proto": doc.proto_version, "Formats": formats, "RegistryHash": self.registry.hash(), "HeartbeatMs": doc.heartbeat_ms});
        let (mut m, _) = self.from_json_data("Hello", &v, Strictness::Lenient)?;
        m.seq = seq;
        Ok(m)
    }

    /// Heartbeat message (no payload).
    pub fn heartbeat(&self, seq: u16) -> Result<Message, LinkError> {
        let spec = self.spec_by_name("Heartbeat")?;
        Ok(Message { id: spec.id, seq, payload: Vec::new() })
    }
}
