//! Layout signature: CRC32 over the canonical (resolved, comment/attribute/init-free) declaration.
//!
//! Two declarations that produce the same memory layout and member names produce the same signature;
//! renaming, reordering, resizing or retyping any member changes it. The `LayoutSig` member itself is
//! part of the declaration (name + type), but its start value is not.

use crate::LayoutError;
use crate::ast::{Bound, Field, TypeRef};
use crate::contract::Contract;

pub const SIG_MEMBER: &str = "LayoutSig";

pub fn canonical_fields(contract: &Contract, fields: &[Field], out: &mut String) -> Result<(), LayoutError> {
    out.push('{');
    for f in fields {
        out.push_str(&f.name);
        out.push(':');
        canonical_type(contract, &f.ty, out)?;
        out.push(';');
    }
    out.push('}');
    Ok(())
}

pub fn canonical_type(contract: &Contract, ty: &TypeRef, out: &mut String) -> Result<(), LayoutError> {
    match ty {
        TypeRef::Prim(p) => out.push_str(&p.name()),
        TypeRef::Udt(name) => {
            let udt = contract.udt(name)?;
            canonical_fields(contract, &udt.fields, out)?;
        }
        TypeRef::Struct(fields) => canonical_fields(contract, fields, out)?,
        TypeRef::Array { dims, elem } => {
            out.push('[');
            for (i, (lo, hi)) in dims.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&format!("{}..{}", contract.bound(lo)?, contract.bound(hi)?));
            }
            out.push(']');
            canonical_type(contract, elem, out)?;
        }
    }
    Ok(())
}

/// Canonical text of a DB declaration.
pub fn canonical_db(contract: &Contract, db: &str) -> Result<String, LayoutError> {
    let decl = contract.db(db)?;
    let mut s = String::new();
    canonical_fields(contract, &decl.fields, &mut s)?;
    Ok(s)
}

pub fn layout_sig(contract: &Contract, db: &str) -> Result<u32, LayoutError> {
    Ok(crc32fast::hash(canonical_db(contract, db)?.as_bytes()))
}

#[allow(dead_code)]
fn _bound_unused(_: &Bound) {}
