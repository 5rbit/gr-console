//! A `Contract` is the set of UDTs, DB declarations, constants and DB numbers loaded from a TIA export
//! directory (`types/**/*.udt`, `blocks/**/*.db`, `blocks/**/*.xml` for numbers, `tags/**/*.xml` for constants).

use std::collections::HashMap;
use std::path::Path;

use serde_json::Value as Json;

use crate::LayoutError;
use crate::ast::{Bound, DbDecl, Field, TypeRef, UdtDecl};
use crate::decode::{Value, decode_fields, decode_member};
use crate::encode::{encode_fields, encode_member};
use crate::layout::{Cursor, Layout, Member, layout_of_fields, resolve_dims, size_of, walk};
use crate::parse::{parse_db, parse_udt};
use crate::signature;

#[derive(Clone, Debug, Default)]
pub struct Contract {
    pub udts: HashMap<String, UdtDecl>,
    pub dbs: HashMap<String, DbDecl>,
    pub consts: HashMap<String, i64>,
    pub db_numbers: HashMap<String, u16>,
    /// Source labels (file names) for diagnostics.
    pub sources: Vec<String>,
    /// Files that failed to parse (unrelated blocks may use unsupported types); diagnostics only.
    pub skipped: Vec<String>,
}

impl Contract {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_udt_source(&mut self, src: &str) -> Result<String, LayoutError> {
        let u = parse_udt(src)?;
        let name = u.name.clone();
        self.udts.insert(name.clone(), u);
        Ok(name)
    }

    pub fn add_db_source(&mut self, src: &str) -> Result<String, LayoutError> {
        let d = parse_db(src)?;
        let name = d.name.clone();
        self.dbs.insert(name.clone(), d);
        Ok(name)
    }

    pub fn add_consts_xml(&mut self, xml: &str) -> Result<usize, LayoutError> {
        let list = crate::consts::parse_const_xml(xml)?;
        let n = list.len();
        for (name, v, _) in list {
            self.consts.insert(name, v);
        }
        Ok(n)
    }

    /// Loads an export-style directory: `<root>/types/**/*.udt`, `<root>/blocks/**/*.db` (+ sibling `.xml`
    /// for the DB number), `<root>/tags/**/*.xml` constants. Missing sub-dirs are skipped; sources that
    /// fail to parse are recorded in `skipped` (they only matter if a needed type refers to them).
    pub fn load_dir(root: &Path) -> Result<Self, LayoutError> {
        let mut c = Contract::new();
        for f in walk_files(&root.join("tags"), "xml") {
            let text = std::fs::read_to_string(&f)?;
            if text.contains("PlcUserConstant") {
                match c.add_consts_xml(&text) {
                    Ok(_) => c.sources.push(f.display().to_string()),
                    Err(e) => c.skipped.push(format!("{}: {e}", f.display())),
                }
            }
        }
        for f in walk_files(&root.join("types"), "udt") {
            let text = std::fs::read_to_string(&f)?;
            match c.add_udt_source(&text) {
                Ok(_) => c.sources.push(f.display().to_string()),
                Err(e) => c.skipped.push(format!("{}: {e}", f.display())),
            }
        }
        for f in walk_files(&root.join("blocks"), "db") {
            let text = std::fs::read_to_string(&f)?;
            let name = match c.add_db_source(&text) {
                Ok(n) => n,
                Err(e) => {
                    c.skipped.push(format!("{}: {e}", f.display()));
                    continue;
                }
            };
            c.sources.push(f.display().to_string());
            let xml = f.with_extension("xml");
            if let Ok(x) = std::fs::read_to_string(&xml)
                && let Some(n) = db_number_from_xml(&x)
            {
                c.db_numbers.insert(name, n);
            }
        }
        Ok(c)
    }

    pub fn udt(&self, name: &str) -> Result<&UdtDecl, LayoutError> {
        self.udts.get(name).ok_or_else(|| LayoutError::UnknownUdt(name.to_string()))
    }

    pub fn db(&self, name: &str) -> Result<&DbDecl, LayoutError> {
        self.dbs.get(name).ok_or_else(|| LayoutError::UnknownDb(name.to_string()))
    }

    pub fn db_number(&self, name: &str) -> Option<u16> {
        self.db_numbers.get(name).copied()
    }

    pub fn bound(&self, b: &Bound) -> Result<i64, LayoutError> {
        match b {
            Bound::Int(i) => Ok(*i),
            Bound::Const(c) => self.consts.get(c).copied().ok_or_else(|| LayoutError::UnknownConst(c.clone())),
        }
    }

    pub fn layout_db(&self, name: &str) -> Result<Layout, LayoutError> {
        let d = self.db(name)?;
        layout_of_fields(self, name, &d.fields)
    }

    pub fn layout_udt(&self, name: &str) -> Result<Layout, LayoutError> {
        let u = self.udt(name)?;
        layout_of_fields(self, name, &u.fields)
    }

    pub fn size_of_udt(&self, name: &str) -> Result<u32, LayoutError> {
        size_of(self, &TypeRef::Udt(name.to_string()))
    }

    pub fn size_of_db(&self, name: &str) -> Result<u32, LayoutError> {
        Ok(self.layout_db(name)?.size)
    }

    /// Member lookup by path within a DB layout (computes the layout; cache the Layout for hot paths).
    pub fn resolve(&self, db: &str, path: &str) -> Result<Member, LayoutError> {
        self.layout_db(db)?.find(path).cloned().ok_or_else(|| LayoutError::UnknownPath(format!("{db}.{path}")))
    }

    /// Decodes a whole DB buffer into nested JSON.
    pub fn decode_db(&self, db: &str, buf: &[u8]) -> Result<Json, LayoutError> {
        let d = self.db(db)?;
        let mut cur = Cursor::default();
        decode_fields(self, &d.fields, buf, &mut cur)
    }

    /// Decodes a UDT-typed region starting at `offset`.
    pub fn decode_udt_at(&self, udt: &str, buf: &[u8], offset: u32) -> Result<Json, LayoutError> {
        let u = self.udt(udt)?;
        let mut cur = Cursor { byte: offset, bit: 0 };
        decode_fields(self, &u.fields, buf, &mut cur)
    }

    /// Decodes the sub-tree at a member path (struct/array/scalar) of a DB buffer.
    pub fn decode_path(&self, db: &str, path: &str, buf: &[u8]) -> Result<Json, LayoutError> {
        let d = self.db(db)?;
        let (ty, offset) = self.locate(&d.fields, path)?;
        let mut cur = Cursor { byte: offset, bit: 0 };
        match &ty {
            TypeRef::Prim(_) => {
                let m = self.resolve(db, path)?;
                Ok(decode_member(buf, &m)?.to_json())
            }
            other => crate::decode::decode_tree(self, other, buf, &mut cur),
        }
    }

    /// Encodes JSON into an existing DB buffer at a member path (struct/array/scalar).
    pub fn encode_path(&self, db: &str, path: &str, json: &Json, buf: &mut [u8]) -> Result<(), LayoutError> {
        let d = self.db(db)?;
        let (ty, offset) = self.locate(&d.fields, path)?;
        match &ty {
            TypeRef::Prim(_) => {
                let m = self.resolve(db, path)?;
                let v = match json {
                    Json::Bool(b) => Value::Bool(*b),
                    Json::Number(n) => n
                        .as_u64()
                        .map(Value::U64)
                        .or_else(|| n.as_i64().map(Value::I64))
                        .or_else(|| n.as_f64().map(Value::F64))
                        .ok_or_else(|| LayoutError::Value { path: path.into(), msg: "bad number".into() })?,
                    Json::String(s) => Value::Str(s.clone()),
                    other => return Err(LayoutError::Value { path: path.into(), msg: format!("cannot encode {other}") }),
                };
                encode_member(buf, &m, &v)
            }
            other => {
                let mut cur = Cursor { byte: offset, bit: 0 };
                crate::encode::encode_tree(self, other, json, buf, &mut cur, path)
            }
        }
    }

    /// Encodes a whole DB JSON object into `buf`.
    pub fn encode_db(&self, db: &str, json: &Json, buf: &mut [u8]) -> Result<(), LayoutError> {
        let d = self.db(db)?;
        let mut cur = Cursor::default();
        encode_fields(self, &d.fields, json, buf, &mut cur, "")
    }

    pub fn layout_sig(&self, db: &str) -> Result<u32, LayoutError> {
        signature::layout_sig(self, db)
    }

    /// Finds the type and byte offset of a (struct/array/scalar) member path.
    pub fn locate(&self, fields: &[Field], path: &str) -> Result<(TypeRef, u32), LayoutError> {
        let mut cur = Cursor::default();
        let mut ty = TypeRef::Struct(fields.to_vec());
        let segments = split_path(path);
        if segments.is_empty() {
            return Ok((ty, 0));
        }
        for seg in &segments {
            match seg {
                Seg::Field(name) => {
                    let fields = match &ty {
                        TypeRef::Struct(f) => f.clone(),
                        TypeRef::Udt(u) => self.udt(u)?.fields.clone(),
                        _ => return Err(LayoutError::UnknownPath(path.to_string())),
                    };
                    cur.align_word();
                    let mut found = None;
                    for f in &fields {
                        if f.name == *name {
                            align_for(&f.ty, &mut cur);
                            found = Some(f.ty.clone());
                            break;
                        }
                        skip(self, &f.ty, &mut cur)?;
                    }
                    ty = found.ok_or_else(|| LayoutError::UnknownPath(path.to_string()))?;
                }
                Seg::Index(idx) => {
                    let TypeRef::Array { dims, elem } = &ty else { return Err(LayoutError::UnknownPath(path.to_string())) };
                    cur.align_word();
                    let dims_r = resolve_dims(self, dims)?;
                    let (lo, hi) = dims_r[0];
                    if *idx < lo || *idx > hi {
                        return Err(LayoutError::UnknownPath(format!("{path}: index {idx} out of {lo}..{hi}")));
                    }
                    let inner: TypeRef = if dims_r.len() > 1 { TypeRef::Array { dims: dims[1..].to_vec(), elem: elem.clone() } } else { (**elem).clone() };
                    for _ in lo..*idx {
                        skip(self, &inner, &mut cur)?;
                    }
                    align_for(&inner, &mut cur);
                    ty = inner;
                }
            }
        }
        Ok((ty, cur.byte))
    }
}

fn align_for(ty: &TypeRef, cur: &mut Cursor) {
    match ty {
        TypeRef::Prim(p) => match p.align() {
            0 => {}
            1 => cur.align_byte(),
            _ => cur.align_word(),
        },
        TypeRef::Udt(_) | TypeRef::Struct(_) | TypeRef::Array { .. } => cur.align_word(),
    }
}

/// Advances the cursor over a type (same as walking it without recording members).
fn skip(c: &Contract, ty: &TypeRef, cur: &mut Cursor) -> Result<(), LayoutError> {
    let mut sink = Vec::new();
    walk(c, ty, "", cur, &mut sink)
}

#[derive(Debug, PartialEq)]
enum Seg {
    Field(String),
    Index(i64),
}

fn split_path(path: &str) -> Vec<Seg> {
    let mut out = Vec::new();
    for part in path.split('.') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let mut rest = part;
        if let Some(b) = rest.find('[') {
            if b > 0 {
                out.push(Seg::Field(rest[..b].to_string()));
            }
            rest = &rest[b..];
            while let Some(stripped) = rest.strip_prefix('[') {
                let end = stripped.find(']').unwrap_or(stripped.len());
                if let Ok(i) = stripped[..end].trim().parse::<i64>() {
                    out.push(Seg::Index(i));
                }
                rest = stripped.get(end + 1..).unwrap_or("");
            }
        } else {
            out.push(Seg::Field(rest.to_string()));
        }
    }
    out
}

fn walk_files(dir: &Path, ext: &str) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return out;
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case(ext)).unwrap_or(false) {
                out.push(p);
            }
        }
    }
    out.sort();
    out
}

fn db_number_from_xml(xml: &str) -> Option<u16> {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).ok()?;
    let block = doc.descendants().find(|n| n.has_tag_name("SW.Blocks.GlobalDB") || n.has_tag_name("SW.Blocks.InstanceDB"))?;
    let attrs = block.children().find(|c| c.has_tag_name("AttributeList"))?;
    attrs.children().find(|c| c.has_tag_name("Number"))?.text()?.trim().parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_path_handles_indices() {
        assert_eq!(
            split_path("STAT.Task.Queue[2].Cell.Position[3]"),
            vec![Seg::Field("STAT".into()), Seg::Field("Task".into()), Seg::Field("Queue".into()), Seg::Index(2), Seg::Field("Cell".into()), Seg::Field("Position".into()), Seg::Index(3)]
        );
    }
}
