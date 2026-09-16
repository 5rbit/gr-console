//! Code generator behind `gr-contract gen-link` (plan "PLC 측 생성 SCL 형태", wire-spec).
//!
//! From a contract and the resolved message registry it produces, deterministically (sorted collections, no
//! timestamps):
//! * `LNK_Const_Gen.xml`: constant table (message ids, signatures, sizes, worst-case JSON lengths, hash, errors)
//! * `JsonW_<Udt>` for the writer closure (messages the PLC sends, plus the payload of every envelope) and
//!   `JsonR_<Udt>` for the reader closure (messages the PLC receives), callees first
//! * `LnkJ_<Msg>` envelope writers for every generated message
//! * `LNK_TestVectors.db`: golden values, Serialize bytes and envelope text for the PLC self-test
//! * PC artifacts `registry.json`, `schema/<Msg>.schema.json`, `vectors/<Msg>.json`
//!
//! TIA sources are UTF-8 with BOM and CRLF, PC artifacts LF without BOM.

mod closure;
mod envelope;
mod names;
mod pc;
mod scl;
mod tags;
mod testvec;
mod text;

use std::collections::BTreeSet;

use serde::Serialize;

use plc_layout::Contract;
use plc_layout::ast::TypeRef;

use crate::error::{ErrCode, LinkError};
use crate::header::Format;
use crate::registry::{Direction, MessageSpec, Registry};
use crate::wire_json::check_supported;

pub use closure::{callees_first, direct_udts, udt_closure};
pub use names::{MAX_LIT_LEN, MAX_NAME_LEN, Names, member, scl_str};
pub use tags::ConstDef;
pub use testvec::{JSON_CHUNK, TEST_DB_MAX_BYTES, VECTOR_SEQ};
pub use text::{GENERATED_MARKER, has_generated_marker, normalize_for_compare};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenOpts {
    /// PLC name (registry.json, schema ids).
    pub plc: String,
    /// Every registry message must be available; otherwise unavailable messages are skipped with a warning.
    pub strict: bool,
}

/// Output directory class of a generated file.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Target {
    /// PLC blocks (`--out-scl`, `blocks/700. Communication/Socket/Gen`)
    Scl,
    /// PLC constant tables (`--out-tags`, `tags/Const`)
    Tags,
    /// PC artifacts (`--out-pc`, `plc/generated/link/<PLC>`)
    Pc,
}

impl Target {
    pub fn name(self) -> &'static str {
        match self {
            Target::Scl => "scl",
            Target::Tags => "tags",
            Target::Pc => "pc",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockKind {
    TagTable,
    WriterFc,
    ReaderFc,
    EnvelopeFc,
    GlobalDb,
}

impl BlockKind {
    pub fn name(self) -> &'static str {
        match self {
            BlockKind::TagTable => "tag_table",
            BlockKind::WriterFc => "writer_fc",
            BlockKind::ReaderFc => "reader_fc",
            BlockKind::EnvelopeFc => "envelope_fc",
            BlockKind::GlobalDb => "global_db",
        }
    }
}

/// A generated PLC object, in manifest order (every block only depends on blocks listed before it).
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Block {
    pub name: String,
    pub kind: BlockKind,
    pub target: Target,
    /// File name relative to the target directory.
    pub file: String,
    /// UDT, message or `registry` the block is generated from.
    pub from: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenFile {
    pub target: Target,
    /// Path relative to the target directory (`/` separated).
    pub path: String,
    /// Exact file content (encoding included).
    pub bytes: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GenOutput {
    pub blocks: Vec<Block>,
    pub files: Vec<GenFile>,
    pub constants: Vec<ConstDef>,
    pub warnings: Vec<String>,
}

impl GenOutput {
    pub fn file(&self, target: Target, path: &str) -> Option<&GenFile> {
        self.files.iter().find(|f| f.target == target && f.path == path)
    }

    fn add(&mut self, block: Block, bytes: Vec<u8>) {
        self.files.push(GenFile { target: block.target, path: block.file.clone(), bytes });
        self.blocks.push(block);
    }
}

fn block(name: &str, kind: BlockKind, target: Target, file: String, from: &str) -> Block {
    Block { name: name.to_string(), kind, target, file, from: from.to_string() }
}

/// Generates every artifact. Pure: nothing is read or written.
pub fn generate(c: &Contract, r: &Registry, opts: &GenOpts) -> Result<GenOutput, LinkError> {
    let names = Names::new(&r.doc);
    let mut warnings = Vec::new();
    let mut msgs: Vec<&MessageSpec> = Vec::new();
    for m in &r.messages {
        if !m.available {
            let why = m.unavailable_reason.clone().unwrap_or_default();
            if opts.strict {
                return Err(LinkError::new(ErrCode::UnknownType, format!("message {} unavailable: {why}", m.name)));
            }
            warnings.push(format!("message {} skipped (not available: {why})", m.name));
            continue;
        }
        if let Some(u) = &m.udt {
            check_supported(c, &TypeRef::Udt(u.clone())).map_err(|e| LinkError { msg: format!("message {} (\"{u}\"): {}", m.name, e.msg), ..e })?;
            if m.dir != Direction::PlcToPc && m.json_max > r.doc.plc_max_payload as usize {
                warnings.push(format!("message {}: worst-case JSON envelope {} B exceeds the PLC receive maximum {} B", m.name, m.json_max, r.doc.plc_max_payload));
            }
        }
        msgs.push(m);
    }
    // A message that does not allow JSON gets no envelope and no JsonW_ / JsonR_ closure: its payload only ever
    // travels as Serialize bytes (Trace, whose chunk is a 2032-element DWord array).
    let json_msgs: Vec<&MessageSpec> = msgs.iter().copied().filter(|m| m.allows(Format::Json)).collect();
    let payload_udts = |keep: &dyn Fn(Direction) -> bool| -> Vec<&str> { json_msgs.iter().filter(|m| keep(m.dir)).filter_map(|m| m.udt.as_deref()).collect() };
    // Every message with an envelope gets an envelope writer, so the writer closure also covers PC → PLC payloads.
    let writers = udt_closure(c, payload_udts(&|_| true))?;
    let readers = udt_closure(c, payload_udts(&|d| d != Direction::PlcToPc))?;

    let mut out = GenOutput { blocks: Vec::new(), files: Vec::new(), constants: tags::constants(r, &msgs), warnings };
    let table = names.table();
    let xml = tags::render_xml(&table, &out.constants);
    out.add(block(&table, BlockKind::TagTable, Target::Tags, format!("{table}.xml"), "registry"), text::tia_bytes(&xml));
    for u in callees_first(c, &writers)? {
        let name = names.writer(&u)?;
        let src = scl::writer_fc(c, &names, &u, &name)?;
        out.add(block(&name, BlockKind::WriterFc, Target::Scl, format!("{name}.scl"), &u), text::tia_bytes(&src));
    }
    for u in callees_first(c, &readers)? {
        let name = names.reader(&u)?;
        let src = scl::reader_fc(c, &names, &u, &name)?;
        out.add(block(&name, BlockKind::ReaderFc, Target::Scl, format!("{name}.scl"), &u), text::tia_bytes(&src));
    }
    for m in &json_msgs {
        let name = names.envelope(&m.name)?;
        let src = envelope::envelope_fc(&names, m, &name)?;
        out.add(block(&name, BlockKind::EnvelopeFc, Target::Scl, format!("{name}.scl"), &m.name), text::tia_bytes(&src));
    }
    if json_msgs.iter().any(|m| m.udt.is_some()) {
        let db = names.test_db();
        let src = testvec::render(c, &json_msgs, &db, r.hash())?;
        out.add(block(&db, BlockKind::GlobalDb, Target::Scl, format!("{db}.db"), "registry"), text::tia_bytes(&src));
    }
    let mut seen = BTreeSet::new();
    for b in &out.blocks {
        if !seen.insert(b.name.to_ascii_lowercase()) {
            return Err(LinkError::parse(format!("generated block name \"{}\" is not unique (TIA names are case-insensitive; check [alias])", b.name)));
        }
    }

    let registry = pc::registry_json(c, r, &msgs, opts, &out.blocks)?;
    out.files.push(GenFile { target: Target::Pc, path: "registry.json".to_string(), bytes: text::pc_bytes(&registry) });
    for m in &json_msgs {
        let schema = pc::schema_json(c, m, opts)?;
        out.files.push(GenFile { target: Target::Pc, path: format!("schema/{}.schema.json", m.name), bytes: text::pc_bytes(&schema) });
        let vector = pc::vector_json(c, m)?;
        out.files.push(GenFile { target: Target::Pc, path: format!("vectors/{}.json", m.name), bytes: text::pc_bytes(&vector) });
    }
    Ok(out)
}
