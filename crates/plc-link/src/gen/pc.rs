//! PC artifacts: `registry.json`, `schema/<Msg>.schema.json` (JSON Schema 2020-12) and `vectors/<Msg>.json`.
//!
//! JSON is written with an ordered builder (not `serde_json::Value`) so key order and formatting never depend on
//! crate features: declaration order everywhere, two-space indentation, short flat containers on one line.

use plc_layout::Contract;
use plc_layout::ast::{Field, Prim, TypeRef};
use plc_layout::layout::resolve_dims;

use crate::envelope::write_envelope;
use crate::error::LinkError;
use crate::framing::encode_frame;
use crate::header::{Format, HEADER_LEN, MAGIC};
use crate::registry::{Direction, MessageSpec, PlcRole, Registry};
use crate::vectors::golden_bytes;
use crate::wire_json::udt_bytes_to_json_text;

use super::closure::udt_closure;
use super::testvec::VECTOR_SEQ;
use super::{Block, GenOpts};

const INLINE_MAX: usize = 120;

/// Ordered JSON value.
enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    /// Already valid JSON text (wire value text).
    Raw(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

impl J {
    fn num(x: impl std::fmt::Display) -> J {
        J::Num(x.to_string())
    }

    fn s(x: impl Into<String>) -> J {
        J::Str(x.into())
    }

    fn opt_s(x: Option<&str>) -> J {
        x.map_or(J::Null, J::s)
    }

    fn obj(pairs: Vec<(&str, J)>) -> J {
        J::Obj(pairs.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
    }

    fn is_container(&self) -> bool {
        match self {
            J::Arr(v) => !v.is_empty(),
            J::Obj(v) => !v.is_empty(),
            _ => false,
        }
    }

    fn write_inline(&self, out: &mut String) {
        match self {
            J::Null => out.push_str("null"),
            J::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
            J::Num(s) | J::Raw(s) => out.push_str(s),
            J::Str(s) => out.push_str(&serde_json::to_string(s).expect("string")),
            J::Arr(v) => {
                out.push('[');
                for (i, x) in v.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    x.write_inline(out);
                }
                out.push(']');
            }
            J::Obj(v) => {
                out.push('{');
                for (i, (k, x)) in v.iter().enumerate() {
                    if i > 0 {
                        out.push_str(", ");
                    }
                    out.push_str(&serde_json::to_string(k).expect("key"));
                    out.push_str(": ");
                    x.write_inline(out);
                }
                out.push('}');
            }
        }
    }

    fn write(&self, out: &mut String, indent: usize) {
        let nested = match self {
            J::Arr(v) => v.iter().any(J::is_container),
            J::Obj(v) => v.iter().any(|(_, x)| x.is_container()),
            _ => false,
        };
        if !nested {
            let mut line = String::new();
            self.write_inline(&mut line);
            if !self.is_container() || indent + line.len() <= INLINE_MAX {
                out.push_str(&line);
                return;
            }
        }
        let pad = " ".repeat(indent + 2);
        match self {
            J::Arr(v) => {
                out.push_str("[\n");
                for (i, x) in v.iter().enumerate() {
                    out.push_str(&pad);
                    x.write(out, indent + 2);
                    out.push_str(if i + 1 < v.len() { ",\n" } else { "\n" });
                }
                out.push_str(&" ".repeat(indent));
                out.push(']');
            }
            J::Obj(v) => {
                out.push_str("{\n");
                for (i, (k, x)) in v.iter().enumerate() {
                    out.push_str(&pad);
                    out.push_str(&serde_json::to_string(k).expect("key"));
                    out.push_str(": ");
                    x.write(out, indent + 2);
                    out.push_str(if i + 1 < v.len() { ",\n" } else { "\n" });
                }
                out.push_str(&" ".repeat(indent));
                out.push('}');
            }
            other => other.write_inline(out),
        }
    }

    fn render(&self) -> String {
        let mut s = String::new();
        self.write(&mut s, 0);
        s.push('\n');
        s
    }
}

fn dir_name(d: Direction) -> &'static str {
    match d {
        Direction::Both => "both",
        Direction::PlcToPc => "plc_to_pc",
        Direction::PcToPlc => "pc_to_plc",
    }
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// `registry.json`: hash, header layout, messages (with the standard-layout member table), errors, routes and the
/// generated PLC blocks in manifest order.
pub(crate) fn registry_json(c: &Contract, r: &Registry, selected: &[&MessageSpec], opts: &GenOpts, blocks: &[Block]) -> Result<String, LinkError> {
    let d = &r.doc;
    let hash = r.hash();
    let field = |name: &str, offset: usize, size: usize, value: &str| J::obj(vec![("name", J::s(name)), ("offset", J::num(offset)), ("size", J::num(size)), ("value", J::s(value))]);
    let header = J::obj(vec![
        ("length", J::num(HEADER_LEN)),
        ("byte_order", J::s("big_endian")),
        (
            "fields",
            J::Arr(vec![
                field("Magic", 0, 2, &format!("0x{MAGIC:04X}")),
                field("Version", 2, 1, &d.proto_version.to_string()),
                field("Format", 3, 1, "1 JSON, 2 BIN"),
                field("MsgType", 4, 2, "message id"),
                field("Seq", 6, 2, "sender sequence number (0 = none)"),
                field("LayoutSig", 8, 4, "BIN: UDT signature, JSON: 0"),
                field("Length", 12, 4, "payload byte count"),
            ]),
        ),
    ]);

    let mut messages = Vec::new();
    for m in &r.messages {
        let generated = selected.iter().any(|s| s.id == m.id);
        let mut members = Vec::new();
        if let (Some(u), true) = (&m.udt, generated) {
            for x in c.layout_udt(u)?.members {
                members.push(J::obj(vec![("path", J::s(x.path)), ("offset", J::num(x.offset)), ("bit", x.bit.map_or(J::Null, J::num)), ("type", J::s(x.prim.name())), ("size", J::num(x.size))]));
            }
        }
        let reply = m.reply.and_then(|id| r.by_id(id)).map(|x| x.name.as_str());
        messages.push(J::obj(vec![
            ("id", J::num(m.id)),
            ("name", J::s(&m.name)),
            ("udt", J::opt_s(m.udt.as_deref())),
            ("dir", J::s(dir_name(m.dir))),
            ("formats", J::Arr(m.formats.iter().map(|f| J::s(f.name())).collect())),
            ("ack", J::Bool(m.ack)),
            ("reply", J::opt_s(reply)),
            ("sig", J::num(m.sig)),
            ("sig_hex", J::s(format!("0x{:08X}", m.sig))),
            ("size", J::num(m.size)),
            ("json_max", J::num(m.json_max)),
            ("available", J::Bool(m.available)),
            ("generated", J::Bool(generated)),
            ("members", J::Arr(members)),
        ]));
    }
    let errors = d.errors.iter().map(|e| J::obj(vec![("code", J::num(e.code)), ("name", J::s(&e.name)), ("runtime", J::Bool(e.runtime))])).collect();
    let routes = d
        .routes
        .iter()
        .map(|rt| {
            J::obj(vec![
                ("role", J::s(if rt.role == PlcRole::Active { "active" } else { "passive" })),
                ("method", J::s(&rt.method)),
                ("path", J::s(&rt.path)),
                ("msgs", J::Arr(rt.msgs.iter().map(J::s).collect())),
                ("response", J::opt_s(rt.response.as_deref())),
                ("empty_status", rt.empty_status.map_or(J::Null, J::num)),
            ])
        })
        .collect();
    let plc_blocks =
        blocks.iter().map(|b| J::obj(vec![("name", J::s(&b.name)), ("kind", J::s(b.kind.name())), ("target", J::s(b.target.name())), ("file", J::s(&b.file)), ("from", J::s(&b.from))])).collect();
    let root = J::obj(vec![
        ("generator", J::s("gr-contract gen-link")),
        ("plc", J::s(&opts.plc)),
        ("registry_hash", J::num(hash)),
        ("registry_hash_hex", J::s(format!("0x{hash:08X}"))),
        ("hash_text", J::s(r.hash_text())),
        ("magic", J::num(d.magic)),
        ("proto_version", J::num(d.proto_version)),
        ("max_payload", J::num(d.max_payload)),
        ("plc_max_payload", J::num(d.plc_max_payload)),
        ("heartbeat_ms", J::num(d.heartbeat_ms)),
        (
            "names",
            J::obj(vec![("const_prefix", J::s(&d.const_prefix)), ("writer_prefix", J::s(&d.writer_prefix)), ("reader_prefix", J::s(&d.reader_prefix)), ("envelope_prefix", J::s(&d.envelope_prefix))]),
        ),
        ("header", header),
        ("formats", J::obj(vec![("json", J::num(Format::Json.code())), ("bin", J::num(Format::Bin.code()))])),
        ("messages", J::Arr(messages)),
        ("errors", J::Arr(errors)),
        ("routes", J::Arr(routes)),
        ("plc_blocks", J::Arr(plc_blocks)),
    ]);
    Ok(root.render())
}

fn int_schema(min: impl std::fmt::Display, max: impl std::fmt::Display) -> Vec<(&'static str, J)> {
    vec![("type", J::s("integer")), ("minimum", J::num(min)), ("maximum", J::num(max))]
}

fn prim_schema(p: Prim) -> Vec<(&'static str, J)> {
    match p {
        Prim::Bool => vec![("type", J::s("boolean"))],
        Prim::Byte | Prim::USInt => int_schema(0, u8::MAX),
        Prim::SInt => int_schema(i8::MIN, i8::MAX),
        Prim::Word | Prim::UInt => int_schema(0, u16::MAX),
        Prim::Int => int_schema(i16::MIN, i16::MAX),
        Prim::DWord | Prim::UDInt => int_schema(0, u32::MAX),
        Prim::DInt => int_schema(i32::MIN, i32::MAX),
        Prim::LInt => int_schema(i64::MIN, i64::MAX),
        Prim::ULInt | Prim::LWord => int_schema(0, u64::MAX),
        Prim::Time => {
            let mut v = int_schema(i32::MIN, i32::MAX);
            v.push(("description", J::s("Time in milliseconds")));
            v
        }
        Prim::Real | Prim::LReal => {
            vec![("type", J::Arr(vec![J::s("number"), J::s("null")])), ("description", J::s("4 fraction digits, exponent form from 1e9; writers send null for NaN/Inf, PLC readers reject null"))]
        }
        Prim::Char => vec![("type", J::s("string")), ("minLength", J::num(1)), ("maxLength", J::num(1))],
        Prim::String(n) => vec![("type", J::s("string")), ("maxLength", J::num(n)), ("description", J::s("Latin-1 characters (U+0000..U+00FF)"))],
        Prim::Dtl => vec![
            ("type", J::Arr(vec![J::s("string"), J::s("null")])),
            ("pattern", J::s("^$|^[0-9]{4}-[0-9]{2}-[0-9]{2}[T ][0-9]{2}:[0-9]{2}:[0-9]{2}(\\.[0-9]{0,9})?$")),
            ("description", J::s("DTL; writers send YYYY-MM-DDTHH:MM:SS.mmm, \"\" / null read as 1970-01-01T00:00:00.000")),
        ],
        Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime => vec![],
    }
}

fn ty_schema(c: &Contract, ty: &TypeRef) -> Result<Vec<(&'static str, J)>, LinkError> {
    Ok(match ty {
        TypeRef::Prim(p) => prim_schema(*p),
        TypeRef::Udt(n) => vec![("$ref", J::s(format!("#/$defs/{n}")))],
        TypeRef::Struct(fields) => fields_schema(c, fields)?,
        TypeRef::Array { dims, elem } => {
            let dims = resolve_dims(c, dims)?;
            let mut inner = ty_schema(c, elem)?;
            for (lo, hi) in dims.iter().rev() {
                let n = (hi - lo + 1).max(0);
                inner =
                    vec![("type", J::s("array")), ("maxItems", J::num(n)), ("description", J::s(format!("PLC index {lo}..{hi}; missing trailing elements read as zero"))), ("items", J::obj(inner))];
            }
            inner
        }
    })
}

fn fields_schema(c: &Contract, fields: &[Field]) -> Result<Vec<(&'static str, J)>, LinkError> {
    let mut props = Vec::with_capacity(fields.len());
    for f in fields {
        let mut s = ty_schema(c, &f.ty)?;
        if !f.comment.is_empty() {
            s.retain(|(k, _)| *k != "description");
            s.push(("description", J::s(f.comment.trim())));
        }
        props.push((f.name.clone(), J::obj(s)));
    }
    Ok(vec![("type", J::s("object")), ("properties", J::Obj(props)), ("additionalProperties", J::Bool(false))])
}

/// `schema/<Msg>.schema.json`: the envelope with `data` referencing the payload UDT under `$defs`.
pub(crate) fn schema_json(c: &Contract, m: &MessageSpec, opts: &GenOpts) -> Result<String, LinkError> {
    let mut defs = Vec::new();
    if let Some(u) = &m.udt {
        for name in udt_closure(c, [u.as_str()])? {
            let decl = c.udt(&name)?;
            let mut s = fields_schema(c, &decl.fields)?;
            s.insert(0, ("title", J::s(format!("UDT \"{name}\""))));
            defs.push((name, J::obj(s)));
        }
    }
    let data = match &m.udt {
        Some(u) => J::obj(vec![("$ref", J::s(format!("#/$defs/{u}")))]),
        None => J::obj(vec![("type", J::s("null"))]),
    };
    let sig_values = if m.sig == 0 { vec![J::num(0)] } else { vec![J::num(0), J::num(m.sig)] };
    let required = if m.udt.is_some() { vec![J::s("type"), J::s("data")] } else { vec![J::s("type")] };
    let description = match &m.udt {
        Some(u) => format!(
            "Link message {} (id {}, {}), JSON envelope (docs/link/wire-spec.md section 2). data = UDT \"{u}\" (sig 0x{:08X}, {} B standard layout)",
            m.name,
            m.id,
            dir_name(m.dir),
            m.sig,
            m.size
        ),
        None => format!("Link message {} (id {}, {}), JSON envelope without payload", m.name, m.id, dir_name(m.dir)),
    };
    let mut root = vec![
        ("$schema", J::s("https://json-schema.org/draft/2020-12/schema")),
        ("$id", J::s(format!("urn:gr-link:{}:{}", opts.plc, m.name))),
        (
            "title",
            J::s(match &m.udt {
                Some(u) => format!("{} ({u})", m.name),
                None => m.name.clone(),
            }),
        ),
        ("description", J::s(description)),
        ("type", J::s("object")),
        (
            "properties",
            J::obj(vec![
                (
                    "type",
                    J::obj(vec![
                        ("description", J::s("message name (readers also accept the numeric id)")),
                        ("anyOf", J::Arr(vec![J::obj(vec![("const", J::s(&m.name))]), J::obj(vec![("const", J::num(m.id))])])),
                    ]),
                ),
                ("seq", J::obj(int_schema(0, u16::MAX))),
                ("sig", J::obj(vec![("description", J::s("UDT signature (0 = not checked)")), ("enum", J::Arr(sig_values))])),
                ("data", data),
            ]),
        ),
        ("required", J::Arr(required)),
    ];
    if !defs.is_empty() {
        root.push(("$defs", J::Obj(defs)));
    }
    Ok(J::obj(root).render())
}

/// `vectors/<Msg>.json`: golden value (seq 1) as wire text, UDT bytes and every framing.
pub(crate) fn vector_json(c: &Contract, m: &MessageSpec) -> Result<String, LinkError> {
    let (value, bytes) = match &m.udt {
        Some(u) => {
            let b = golden_bytes(c, u)?;
            (udt_bytes_to_json_text(c, u, &b)?, b)
        }
        None => ("null".to_string(), Vec::new()),
    };
    let env = write_envelope(&m.name, VECTOR_SEQ, m.sig, m.udt.as_ref().map(|_| value.as_str()));
    let json_ok = m.allows(Format::Json);
    let bin_ok = m.allows(Format::Bin);
    let j = J::obj(vec![
        ("message", J::s(&m.name)),
        ("id", J::num(m.id)),
        ("udt", J::opt_s(m.udt.as_deref())),
        ("seq", J::num(VECTOR_SEQ)),
        ("sig", J::num(m.sig)),
        ("size", J::num(m.size)),
        ("value", J::Raw(value)),
        ("json_text", J::s(env.clone())),
        ("bin_hex", J::s(hex(&bytes))),
        ("frame_json_hex", if json_ok { J::s(hex(&encode_frame(Format::Json, m.id, VECTOR_SEQ, 0, env.as_bytes()))) } else { J::Null }),
        ("frame_bin_hex", if bin_ok { J::s(hex(&encode_frame(Format::Bin, m.id, VECTOR_SEQ, m.sig, &bytes))) } else { J::Null }),
        ("ndjson_text", if json_ok { J::s(format!("{env}\n")) } else { J::Null }),
    ]);
    Ok(j.render())
}
