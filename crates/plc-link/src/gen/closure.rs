//! UDT closure of the registry messages and callee-first ordering of the generated functions.

use std::collections::{BTreeMap, BTreeSet};

use plc_layout::Contract;
use plc_layout::ast::TypeRef;

use crate::error::{ErrCode, LinkError};

/// UDTs referenced by a UDT directly (through its own structs and arrays, not through other UDTs).
pub fn direct_udts(c: &Contract, udt: &str) -> Result<BTreeSet<String>, LinkError> {
    fn go(ty: &TypeRef, out: &mut BTreeSet<String>) {
        match ty {
            TypeRef::Udt(n) => {
                out.insert(n.clone());
            }
            TypeRef::Struct(fields) => fields.iter().for_each(|f| go(&f.ty, out)),
            TypeRef::Array { elem, .. } => go(elem, out),
            TypeRef::Prim(_) => {}
        }
    }
    let u = c.udt(udt).map_err(|_| LinkError::new(ErrCode::UnknownType, format!("unknown UDT \"{udt}\"")))?;
    let mut out = BTreeSet::new();
    for f in &u.fields {
        go(&f.ty, &mut out);
    }
    Ok(out)
}

/// The seed UDTs and every UDT they reference transitively.
pub fn udt_closure<'s>(c: &Contract, seeds: impl IntoIterator<Item = &'s str>) -> Result<BTreeSet<String>, LinkError> {
    let mut set = BTreeSet::new();
    let mut stack: Vec<String> = seeds.into_iter().map(str::to_string).collect();
    while let Some(u) = stack.pop() {
        if !set.contains(&u) {
            stack.extend(direct_udts(c, &u)?);
            set.insert(u);
        }
    }
    Ok(set)
}

/// The UDTs of `set` ordered so that each comes after every UDT of `set` it references (ties by name), i.e. the
/// order in which their generated functions can be imported and compiled.
pub fn callees_first(c: &Contract, set: &BTreeSet<String>) -> Result<Vec<String>, LinkError> {
    fn visit(c: &Contract, u: &str, set: &BTreeSet<String>, done: &mut BTreeMap<String, bool>, out: &mut Vec<String>) -> Result<(), LinkError> {
        match done.get(u) {
            Some(true) => return Ok(()),
            Some(false) => return Err(LinkError::parse(format!("UDT \"{u}\" references itself"))),
            None => {}
        }
        done.insert(u.to_string(), false);
        for d in direct_udts(c, u)? {
            if set.contains(&d) {
                visit(c, &d, set, done, out)?;
            }
        }
        done.insert(u.to_string(), true);
        out.push(u.to_string());
        Ok(())
    }
    let mut done = BTreeMap::new();
    let mut out = Vec::with_capacity(set.len());
    for u in set {
        visit(c, u, set, &mut done, &mut out)?;
    }
    Ok(out)
}
