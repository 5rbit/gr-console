//! Message registry (`plc/link/messages.toml`): TOML model, validation, resolution against a contract.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;
use std::str::FromStr;

use serde::{Deserialize, Serialize};

use plc_layout::Contract;
use plc_layout::ast::TypeRef;

use crate::envelope::envelope_max_len;
use crate::error::{ErrCode, LinkError};
use crate::header::{Format, MAGIC, VERSION};
use crate::wire_json::{check_supported, max_json_len};

/// Message direction.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Direction {
    Both,
    PlcToPc,
    PcToPlc,
}

/// PLC socket role a route applies to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlcRole {
    Active,
    Passive,
}

fn default_formats() -> Vec<Format> {
    vec![Format::Json, Format::Bin]
}

fn default_plc_max() -> u32 {
    8176
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MessageDef {
    pub id: u16,
    pub name: String,
    /// UDT of the payload; empty = no payload (Heartbeat).
    #[serde(default)]
    pub udt: String,
    pub dir: Direction,
    #[serde(default = "default_formats")]
    pub formats: Vec<Format>,
    #[serde(default)]
    pub ack: bool,
    #[serde(default)]
    pub reply: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorDef {
    pub code: i16,
    pub name: String,
    /// PLC runtime-internal code (>= 100), not produced by the PC codec.
    #[serde(default)]
    pub runtime: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RouteDef {
    pub role: PlcRole,
    pub method: String,
    /// Path pattern with `{plc}` / `{msg}` placeholders.
    pub path: String,
    /// Messages accepted as request body (for `{msg}` routes: the allowed names).
    #[serde(default)]
    pub msgs: Vec<String>,
    /// Message carried by a 200 response.
    #[serde(default)]
    pub response: Option<String>,
    /// Status when there is nothing to return (e.g. 204).
    #[serde(default)]
    pub empty_status: Option<u16>,
}

/// The registry file as written.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RegistryDoc {
    pub magic: u16,
    pub proto_version: u8,
    pub max_payload: u32,
    #[serde(default = "default_plc_max")]
    pub plc_max_payload: u32,
    pub heartbeat_ms: u32,
    pub const_prefix: String,
    pub writer_prefix: String,
    pub reader_prefix: String,
    pub envelope_prefix: String,
    #[serde(default, rename = "message")]
    pub messages: Vec<MessageDef>,
    #[serde(default, rename = "error")]
    pub errors: Vec<ErrorDef>,
    #[serde(default, rename = "route")]
    pub routes: Vec<RouteDef>,
    #[serde(default)]
    pub alias: BTreeMap<String, String>,
}

fn reg_err(msg: impl Into<String>) -> LinkError {
    LinkError::parse(format!("registry: {}", msg.into()))
}

fn is_ident(s: &str) -> bool {
    let mut ch = s.chars();
    ch.next().is_some_and(|c| c.is_ascii_alphabetic()) && ch.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

impl FromStr for RegistryDoc {
    type Err = LinkError;

    /// Parses and validates registry TOML.
    fn from_str(text: &str) -> Result<Self, LinkError> {
        let doc: RegistryDoc = toml::from_str(text).map_err(|e| reg_err(e.to_string()))?;
        doc.validate()?;
        Ok(doc)
    }
}

impl RegistryDoc {
    pub fn load(path: &Path) -> Result<Self, LinkError> {
        let text = std::fs::read_to_string(path).map_err(|e| reg_err(format!("{}: {e}", path.display())))?;
        text.parse()
    }

    /// Constant name of a message id (`LNK_MSG_MEASLOG`).
    pub fn msg_const(&self, name: &str) -> String {
        format!("{}MSG_{}", self.const_prefix, name.to_ascii_uppercase())
    }

    /// Constant name of an error code (`LNK_ERR_BAD_MAGIC`).
    pub fn err_const(&self, name: &str) -> String {
        format!("{}ERR_{}", self.const_prefix, name.to_ascii_uppercase())
    }

    pub fn validate(&self) -> Result<(), LinkError> {
        if self.magic != MAGIC {
            return Err(reg_err(format!("magic 0x{:04X} != 0x{MAGIC:04X}", self.magic)));
        }
        if self.proto_version != VERSION {
            return Err(reg_err(format!("proto_version {} != {VERSION}", self.proto_version)));
        }
        if self.max_payload == 0 || self.plc_max_payload == 0 || self.heartbeat_ms == 0 {
            return Err(reg_err("max_payload, plc_max_payload and heartbeat_ms must be > 0"));
        }
        for (k, v) in [("const_prefix", &self.const_prefix), ("writer_prefix", &self.writer_prefix), ("reader_prefix", &self.reader_prefix), ("envelope_prefix", &self.envelope_prefix)] {
            if !is_ident(v) {
                return Err(reg_err(format!("{k} {v:?} is not an identifier")));
            }
        }
        let mut ids = HashSet::new();
        let mut names = HashSet::new();
        let mut consts = HashSet::new();
        for m in &self.messages {
            if m.id == 0 {
                return Err(reg_err(format!("message {}: id 0 is reserved", m.name)));
            }
            if !ids.insert(m.id) {
                return Err(reg_err(format!("duplicate message id {}", m.id)));
            }
            if !is_ident(&m.name) {
                return Err(reg_err(format!("message name {:?} is not an identifier", m.name)));
            }
            if !names.insert(m.name.to_ascii_lowercase()) {
                return Err(reg_err(format!("duplicate message name {} (names are case-insensitive)", m.name)));
            }
            if !consts.insert(self.msg_const(&m.name)) {
                return Err(reg_err(format!("duplicate constant {}", self.msg_const(&m.name))));
            }
            if !m.udt.is_empty() && m.udt.trim() != m.udt {
                return Err(reg_err(format!("message {}: udt {:?} has surrounding whitespace", m.name, m.udt)));
            }
            if m.formats.is_empty() {
                return Err(reg_err(format!("message {}: formats is empty", m.name)));
            }
            if m.formats.iter().collect::<HashSet<_>>().len() != m.formats.len() {
                return Err(reg_err(format!("message {}: duplicate format", m.name)));
            }
        }
        for m in &self.messages {
            if let Some(r) = &m.reply
                && !self.messages.iter().any(|x| x.name == *r)
            {
                return Err(reg_err(format!("message {}: reply {r} is not a message", m.name)));
            }
        }
        let mut codes = HashSet::new();
        for e in &self.errors {
            if !codes.insert(e.code) {
                return Err(reg_err(format!("duplicate error code {}", e.code)));
            }
            if !is_ident(&e.name) || !consts.insert(self.err_const(&e.name)) {
                return Err(reg_err(format!("error name {:?} is not a unique identifier", e.name)));
            }
            if let Some(known) = ErrCode::from_code(e.code)
                && known.name() != e.name
            {
                return Err(reg_err(format!("error code {} must be named {}, not {}", e.code, known.name(), e.name)));
            }
        }
        for known in ErrCode::ALL {
            if !self.errors.is_empty() && !self.errors.iter().any(|e| e.code == known.code()) {
                return Err(reg_err(format!("error table misses {} ({})", known.name(), known.code())));
            }
        }
        for r in &self.routes {
            if r.method != "GET" && r.method != "POST" {
                return Err(reg_err(format!("route {}: method {} (GET or POST)", r.path, r.method)));
            }
            if !r.path.starts_with('/') {
                return Err(reg_err(format!("route {}: path must start with /", r.path)));
            }
            for n in r.msgs.iter().chain(r.response.iter()) {
                if !self.messages.iter().any(|x| x.name == *n) {
                    return Err(reg_err(format!("route {}: {n} is not a message", r.path)));
                }
            }
        }
        for (long, short) in &self.alias {
            if long.is_empty() || !is_ident(short) {
                return Err(reg_err(format!("alias {long:?} = {short:?}: short name must be an identifier")));
            }
        }
        Ok(())
    }

    /// Resolves every message against a contract. A message whose UDT (or a nested UDT) is missing from the
    /// contract is `available = false`; unsupported member types are an error.
    pub fn resolve(&self, c: &Contract) -> Result<Registry, LinkError> {
        let mut messages = Vec::with_capacity(self.messages.len());
        for m in &self.messages {
            let mut spec = MessageSpec {
                id: m.id,
                name: m.name.clone(),
                udt: (!m.udt.is_empty()).then(|| m.udt.clone()),
                dir: m.dir,
                formats: m.formats.clone(),
                ack: m.ack,
                reply: m.reply.as_ref().and_then(|r| self.messages.iter().find(|x| x.name == *r)).map(|x| x.id),
                sig: 0,
                size: 0,
                json_max: envelope_max_len(&m.name, 4),
                available: true,
                unavailable_reason: None,
            };
            if !m.udt.is_empty() {
                let ty = TypeRef::Udt(m.udt.clone());
                match check_supported(c, &ty) {
                    Ok(()) => {
                        spec.sig = c.udt_sig(&m.udt).map_err(LinkError::from)?;
                        spec.size = c.size_of_udt(&m.udt).map_err(LinkError::from)?;
                        spec.json_max = envelope_max_len(&m.name, max_json_len(c, &ty)?);
                    }
                    Err(e) if e.code == ErrCode::UnknownType => {
                        spec.available = false;
                        spec.unavailable_reason = Some(format!("{}: {}", m.udt, e));
                    }
                    Err(e) => return Err(LinkError { msg: format!("message {} ({}): {}", m.name, m.udt, e.msg), ..e }),
                }
            }
            messages.push(spec);
        }
        messages.sort_by_key(|m| m.id);
        Ok(Registry { doc: self.clone(), messages })
    }

    /// Like [`RegistryDoc::resolve`] but every message must be available.
    pub fn resolve_strict(&self, c: &Contract) -> Result<Registry, LinkError> {
        let r = self.resolve(c)?;
        if let Some(m) = r.messages.iter().find(|m| !m.available) {
            return Err(LinkError::new(ErrCode::UnknownType, format!("message {} unavailable: {}", m.name, m.unavailable_reason.clone().unwrap_or_default())));
        }
        Ok(r)
    }
}

/// A message resolved against a contract.
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MessageSpec {
    pub id: u16,
    pub name: String,
    pub udt: Option<String>,
    pub dir: Direction,
    pub formats: Vec<Format>,
    pub ack: bool,
    /// Id of the reply message.
    pub reply: Option<u16>,
    /// UDT signature (0 without payload or when unavailable).
    pub sig: u32,
    /// UDT size in bytes (0 without payload or when unavailable).
    pub size: u32,
    /// Worst-case JSON envelope length.
    pub json_max: usize,
    pub available: bool,
    pub unavailable_reason: Option<String>,
}

impl MessageSpec {
    pub fn has_payload(&self) -> bool {
        self.udt.is_some()
    }

    /// Lower-case name used in HTTP routes.
    pub fn http_name(&self) -> String {
        self.name.to_ascii_lowercase()
    }

    pub fn allows(&self, f: Format) -> bool {
        self.formats.contains(&f)
    }

    /// Whether a receiver in `role` may accept this message.
    pub fn receivable_by(&self, role: crate::codec::Role) -> bool {
        matches!((self.dir, role), (Direction::Both, _) | (Direction::PlcToPc, crate::codec::Role::Pc) | (Direction::PcToPlc, crate::codec::Role::Plc))
    }
}

/// Registry resolved against a contract (messages sorted by id).
#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Registry {
    pub doc: RegistryDoc,
    pub messages: Vec<MessageSpec>,
}

impl Registry {
    /// `crc32` over `"{id}:{name}:{sig:08X}:{size};"` of all messages sorted by id (wire-spec section 5).
    pub fn hash(&self) -> u32 {
        crc32fast::hash(self.hash_text().as_bytes())
    }

    pub fn hash_text(&self) -> String {
        let mut s = String::new();
        for m in &self.messages {
            let (sig, size) = if m.available { (m.sig, m.size) } else { (0, 0) };
            s.push_str(&format!("{}:{}:{sig:08X}:{size};", m.id, m.name));
        }
        s
    }

    pub fn by_id(&self, id: u16) -> Option<&MessageSpec> {
        self.messages.iter().find(|m| m.id == id)
    }

    /// Case-insensitive name lookup.
    pub fn by_name(&self, name: &str) -> Option<&MessageSpec> {
        self.messages.iter().find(|m| m.name.eq_ignore_ascii_case(name))
    }

    /// Lookup by the lower-case HTTP route name.
    pub fn by_http_name(&self, http: &str) -> Option<&MessageSpec> {
        self.messages.iter().find(|m| m.http_name() == http)
    }

    /// Name or decimal id.
    pub fn lookup(&self, name_or_id: &str) -> Option<&MessageSpec> {
        let t = name_or_id.trim();
        t.parse::<u16>().ok().and_then(|id| self.by_id(id)).or_else(|| self.by_name(t))
    }

    pub fn max_payload(&self) -> u32 {
        self.doc.max_payload
    }

    pub fn heartbeat_ms(&self) -> u32 {
        self.doc.heartbeat_ms
    }
}
