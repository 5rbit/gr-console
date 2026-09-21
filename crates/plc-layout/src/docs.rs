//! Declaration docs: member paths with their TIA comments and type names, for viewers (e.g. the PARA page).
//!
//! `Layout` flattens arrays element by element and drops comments (the signature must not depend on
//! them); a viewer wants the opposite — one row per declared field with its `// comment`, arrays kept as
//! one value. So this walks the declaration tree instead of the layout.

use serde::Serialize;

use crate::LayoutError;
use crate::ast::{Bound, Field, TypeRef};
use crate::contract::Contract;

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct MemberDoc {
    /// Dotted path without indices, e.g. `Machine.MotorRefTorque`.
    pub path: String,
    /// Display type, e.g. `Real`, `Array[1..4] of Real`, `LGR_PARA_SpeedSet`.
    pub ty: String,
    pub comment: String,
    /// Nesting depth (0 = top-level field of the DB).
    pub depth: usize,
    /// `false` for Struct / UDT containers — their members follow as deeper rows. Arrays are leaves.
    pub leaf: bool,
}

fn bound(b: &Bound) -> String {
    match b {
        Bound::Int(i) => i.to_string(),
        Bound::Const(c) => c.clone(),
    }
}

/// Display name of a type (arrays keep their declared bounds, constants by name).
pub fn type_name(ty: &TypeRef) -> String {
    match ty {
        TypeRef::Prim(p) => p.name(),
        TypeRef::Udt(n) => n.clone(),
        TypeRef::Struct(_) => "Struct".into(),
        TypeRef::Array { dims, elem } => {
            let d: Vec<String> = dims.iter().map(|(lo, hi)| format!("{}..{}", bound(lo), bound(hi))).collect();
            format!("Array[{}] of {}", d.join(", "), type_name(elem))
        }
    }
}

fn walk(contract: &Contract, fields: &[Field], prefix: &str, depth: usize, out: &mut Vec<MemberDoc>) -> Result<(), LayoutError> {
    for f in fields {
        let path = if prefix.is_empty() { f.name.clone() } else { format!("{prefix}.{}", f.name) };
        let children = match &f.ty {
            TypeRef::Struct(inner) => Some(inner.clone()),
            TypeRef::Udt(name) => Some(contract.udt(name)?.fields.clone()),
            _ => None,
        };
        out.push(MemberDoc { path: path.clone(), ty: type_name(&f.ty), comment: f.comment.trim().to_string(), depth, leaf: children.is_none() });
        if let Some(inner) = children {
            walk(contract, &inner, &path, depth + 1, out)?;
        }
    }
    Ok(())
}

impl Contract {
    /// Every declared member of a DB in declaration order (containers before their members).
    pub fn member_docs(&self, db: &str) -> Result<Vec<MemberDoc>, LayoutError> {
        let d = self.db(db)?;
        let mut out = Vec::new();
        walk(self, &d.fields, "", 0, &mut out)?;
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn docs_keep_comments_and_arrays() {
        let mut c = Contract::new();
        c.add_udt_source("TYPE \"U\"\nVERSION : 0.1\n   STRUCT\n      A : Real;   // [p9] a value\n   END_STRUCT;\n\nEND_TYPE\n").unwrap();
        c.add_db_source(
            "DATA_BLOCK \"P\"\n{ S7_Optimized_Access := 'FALSE' }\nVERSION : 0.1\nNON_RETAIN\n   STRUCT \n      Machine : Struct   // group\n         ID : USInt;   // [p1] Machine ID\n         T : Array[1..4] of Real;   // [p50-p53]\n      END_STRUCT;\n      S : \"U\";\n   END_STRUCT;\n\n\nBEGIN\n\nEND_DATA_BLOCK\n",
        )
        .unwrap();
        let d = c.member_docs("P").unwrap();
        let rows: Vec<(&str, &str, &str, usize, bool)> = d.iter().map(|m| (m.path.as_str(), m.ty.as_str(), m.comment.as_str(), m.depth, m.leaf)).collect();
        assert_eq!(
            rows,
            vec![
                ("Machine", "Struct", "group", 0, false),
                ("Machine.ID", "USInt", "[p1] Machine ID", 1, true),
                ("Machine.T", "Array[1..4] of Real", "[p50-p53]", 1, true),
                ("S", "U", "", 0, false),
                ("S.A", "Real", "[p9] a value", 1, true),
            ]
        );
    }
}
