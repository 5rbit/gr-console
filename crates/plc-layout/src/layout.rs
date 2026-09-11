//! Standard-access layout computation: flattened member table with byte/bit offsets.

use serde::Serialize;

use crate::LayoutError;
use crate::ast::{Bound, Field, Prim, TypeRef};
use crate::contract::Contract;

#[derive(Clone, Debug, Serialize)]
pub struct Member {
    /// Dotted path, e.g. `STAT.Task.Queue[2].Cell.Id` (array indices are PLC indices).
    pub path: String,
    pub offset: u32,
    /// Bit number for Bool members.
    pub bit: Option<u8>,
    pub prim: Prim,
    pub size: u32,
}

#[derive(Clone, Debug, Serialize)]
pub struct Layout {
    pub name: String,
    pub size: u32,
    pub members: Vec<Member>,
}

impl Layout {
    pub fn find(&self, path: &str) -> Option<&Member> {
        self.members.iter().find(|m| m.path == path)
    }

    /// Byte range [start, end) covering every member whose path starts with `prefix` (a struct or array).
    pub fn range_of(&self, prefix: &str) -> Option<(u32, u32)> {
        let mut lo = u32::MAX;
        let mut hi = 0u32;
        for m in &self.members {
            if m.path == prefix || m.path.starts_with(prefix) && m.path[prefix.len()..].starts_with(['.', '[']) {
                lo = lo.min(m.offset);
                hi = hi.max(m.offset + m.size.max(1));
            }
        }
        (lo != u32::MAX).then(|| (lo, align2(hi)))
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Cursor {
    pub byte: u32,
    pub bit: u8,
}

impl Cursor {
    pub fn align_byte(&mut self) {
        if self.bit > 0 {
            self.byte += 1;
            self.bit = 0;
        }
    }
    pub fn align_word(&mut self) {
        self.align_byte();
        if self.byte % 2 == 1 {
            self.byte += 1;
        }
    }
}

fn align2(n: u32) -> u32 {
    n + (n % 2)
}

/// Resolved array dimension (lo..=hi).
pub fn resolve_dims(contract: &Contract, dims: &[(Bound, Bound)]) -> Result<Vec<(i64, i64)>, LayoutError> {
    dims.iter()
        .map(|(lo, hi)| Ok((contract.bound(lo)?, contract.bound(hi)?)))
        .collect()
}

/// Walks a type, appending members (with `prefix` path) and advancing the cursor.
pub fn walk(contract: &Contract, ty: &TypeRef, prefix: &str, cur: &mut Cursor, out: &mut Vec<Member>) -> Result<(), LayoutError> {
    match ty {
        TypeRef::Prim(p) => {
            match p.align() {
                0 => {}
                1 => cur.align_byte(),
                _ => cur.align_word(),
            }
            if *p == Prim::Bool {
                out.push(Member { path: prefix.to_string(), offset: cur.byte, bit: Some(cur.bit), prim: *p, size: 0 });
                cur.bit += 1;
                if cur.bit == 8 {
                    cur.bit = 0;
                    cur.byte += 1;
                }
            } else {
                out.push(Member { path: prefix.to_string(), offset: cur.byte, bit: None, prim: *p, size: p.size() });
                cur.byte += p.size();
            }
        }
        TypeRef::Udt(name) => {
            let udt = contract.udt(name)?;
            walk_fields(contract, &udt.fields, prefix, cur, out)?;
        }
        TypeRef::Struct(fields) => walk_fields(contract, fields, prefix, cur, out)?,
        TypeRef::Array { dims, elem } => {
            cur.align_word();
            let dims = resolve_dims(contract, dims)?;
            let mut idx: Vec<i64> = dims.iter().map(|(lo, _)| *lo).collect();
            let total: i64 = dims.iter().map(|(lo, hi)| (hi - lo + 1).max(0)).product();
            for _ in 0..total {
                let suffix: String = idx.iter().map(|i| format!("[{i}]")).collect();
                walk(contract, elem, &format!("{prefix}{suffix}"), cur, out)?;
                // increment multi-dim index, last dimension fastest
                for d in (0..idx.len()).rev() {
                    idx[d] += 1;
                    if idx[d] <= dims[d].1 {
                        break;
                    }
                    idx[d] = dims[d].0;
                }
            }
            cur.align_word();
        }
    }
    Ok(())
}

fn walk_fields(contract: &Contract, fields: &[Field], prefix: &str, cur: &mut Cursor, out: &mut Vec<Member>) -> Result<(), LayoutError> {
    cur.align_word();
    for f in fields {
        let path = if prefix.is_empty() { f.name.clone() } else { format!("{prefix}.{}", f.name) };
        walk(contract, &f.ty, &path, cur, out)?;
    }
    cur.align_word();
    Ok(())
}

/// Computes the layout of a whole DB (declaration order) or of a UDT (as if it were a DB).
pub fn layout_of_fields(contract: &Contract, name: &str, fields: &[Field]) -> Result<Layout, LayoutError> {
    let mut cur = Cursor::default();
    let mut members = Vec::new();
    walk_fields(contract, fields, "", &mut cur, &mut members)?;
    Ok(Layout { name: name.to_string(), size: cur.byte, members })
}

/// Size (bytes) and alignment (1 or 2) of a type.
pub fn size_of(contract: &Contract, ty: &TypeRef) -> Result<u32, LayoutError> {
    let mut cur = Cursor::default();
    let mut out = Vec::new();
    walk(contract, ty, "", &mut cur, &mut out)?;
    cur.align_word();
    Ok(cur.byte)
}
