//! Line-oriented parser for TIA SCL declarations (`.udt` and `.db` external sources).

use crate::LayoutError;
use crate::ast::{Bound, DbDecl, Field, Prim, TypeRef, UdtDecl};

fn perr(line: usize, msg: impl Into<String>) -> LayoutError {
    LayoutError::Parse { line: Some(line), msg: msg.into() }
}

/// Strips `//` comments (outside quotes) and trims. Returns (code, comment).
fn split_comment(line: &str) -> (String, String) {
    let mut in_q = false;
    let bytes = line.as_bytes();
    let mut i = 0;
    while i + 1 < bytes.len() {
        match bytes[i] {
            b'"' => in_q = !in_q,
            b'\'' if !in_q => {
                // skip single-quoted attribute values
                if let Some(end) = line[i + 1..].find('\'') {
                    i += end + 1;
                }
            }
            b'/' if !in_q && bytes[i + 1] == b'/' => {
                return (line[..i].trim().to_string(), line[i + 2..].trim().to_string());
            }
            _ => {}
        }
        i += 1;
    }
    (line.trim().trim_start_matches('\u{feff}').to_string(), String::new())
}

struct Lines<'a> {
    lines: Vec<&'a str>,
    pos: usize,
}

impl<'a> Lines<'a> {
    fn new(src: &'a str) -> Self {
        Self { lines: src.lines().collect(), pos: 0 }
    }
    fn next(&mut self) -> Option<(usize, &'a str)> {
        let l = self.lines.get(self.pos).copied()?;
        self.pos += 1;
        Some((self.pos, l))
    }
}

/// Parses a `TYPE "Name" … END_TYPE` source.
pub fn parse_udt(src: &str) -> Result<UdtDecl, LayoutError> {
    let mut it = Lines::new(src);
    let mut name = None;
    while let Some((ln, raw)) = it.next() {
        let (code, _) = split_comment(raw);
        if let Some(rest) = code.strip_prefix("TYPE ") {
            name = Some(unquote(rest.trim()).to_string());
            continue;
        }
        if name.is_some() && code.eq_ignore_ascii_case("STRUCT") {
            let fields = parse_fields(&mut it, "END_STRUCT")?;
            return Ok(UdtDecl { name: name.unwrap(), fields });
        }
        if name.is_some() && code.eq_ignore_ascii_case("END_TYPE") {
            return Err(perr(ln, "TYPE without STRUCT"));
        }
    }
    Err(LayoutError::Parse { line: None, msg: "no TYPE declaration found".into() })
}

/// Parses a `DATA_BLOCK "Name" … END_DATA_BLOCK` source (global DB).
pub fn parse_db(src: &str) -> Result<DbDecl, LayoutError> {
    let mut it = Lines::new(src);
    let mut name: Option<String> = None;
    let mut attrs: Vec<(String, String)> = Vec::new();
    let mut fields = Vec::new();
    let mut begin = Vec::new();
    while let Some((ln, raw)) = it.next() {
        let (code, _) = split_comment(raw);
        if code.is_empty() {
            continue;
        }
        if name.is_none() {
            if let Some(rest) = code.strip_prefix("DATA_BLOCK ") {
                name = Some(unquote(rest.trim()).to_string());
            }
            continue;
        }
        if code.starts_with('{') {
            // attribute block, possibly spanning lines
            let mut text = code.clone();
            while !text.contains('}') {
                let Some((_, more)) = it.next() else { break };
                text.push(' ');
                text.push_str(&split_comment(more).0);
            }
            attrs.extend(parse_attrs(&text));
            continue;
        }
        let upper = code.to_ascii_uppercase();
        if upper.starts_with("VAR") && !upper.starts_with("VAR_") {
            // VAR, VAR RETAIN, VAR DB_SPECIFIC ...
            fields.extend(parse_fields(&mut it, "END_VAR")?);
        } else if upper == "STRUCT" {
            fields.extend(parse_fields(&mut it, "END_STRUCT")?);
        } else if upper == "BEGIN" {
            while let Some((_, raw)) = it.next() {
                let (c, _) = split_comment(raw);
                if c.to_ascii_uppercase().starts_with("END_DATA_BLOCK") {
                    break;
                }
                if let Some((l, r)) = c.split_once(":=") {
                    begin.push((l.trim().to_string(), r.trim().trim_end_matches(';').trim().to_string()));
                }
            }
        } else if upper.starts_with("END_DATA_BLOCK") {
            break;
        } else if upper.starts_with("VERSION")
            || upper.starts_with("TITLE")
            || upper.starts_with("AUTHOR")
            || upper.starts_with("FAMILY")
            || upper.starts_with("NAME")
            || upper == "NON_RETAIN"
            || upper == "RETAIN"
        {
            continue;
        } else {
            return Err(perr(ln, format!("unexpected: {code}")));
        }
    }
    let name = name.ok_or(LayoutError::Parse { line: None, msg: "no DATA_BLOCK declaration".into() })?;
    let optimized = attrs
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("S7_Optimized_Access"))
        .map(|(_, v)| v.eq_ignore_ascii_case("TRUE"))
        .unwrap_or(true);
    Ok(DbDecl { name, optimized, attrs, fields, begin })
}

fn unquote(s: &str) -> &str {
    s.trim().trim_matches('"')
}

/// Parses `{ A := 'x' ; B := 'y' }` → pairs.
pub fn parse_attrs(text: &str) -> Vec<(String, String)> {
    let inner = text.trim().trim_start_matches('{').trim_end_matches('}');
    inner
        .split(';')
        .filter_map(|p| {
            let (k, v) = p.split_once(":=")?;
            Some((k.trim().to_string(), v.trim().trim_matches('\'').to_string()))
        })
        .collect()
}

/// Parses field lines until `terminator` (consumed).
fn parse_fields(it: &mut Lines, terminator: &str) -> Result<Vec<Field>, LayoutError> {
    let mut out = Vec::new();
    while let Some((ln, raw)) = it.next() {
        let (code, comment) = split_comment(raw);
        if code.is_empty() {
            continue;
        }
        let upper = code.to_ascii_uppercase();
        if upper.starts_with(terminator) {
            return Ok(out);
        }
        // name {attrs} : type [:= init] ;
        let (name, rest) = split_name(&code).ok_or_else(|| perr(ln, format!("cannot parse field: {code}")))?;
        let mut rest = rest.trim();
        let mut attrs = Vec::new();
        if rest.starts_with('{') {
            let end = rest.find('}').ok_or_else(|| perr(ln, "unterminated attribute block"))?;
            attrs = parse_attrs(&rest[..=end]);
            rest = rest[end + 1..].trim();
        }
        let rest = rest.strip_prefix(':').ok_or_else(|| perr(ln, format!("expected ':' in: {code}")))?.trim();
        let (type_text, init) = match rest.split_once(":=") {
            Some((t, i)) => (t.trim(), Some(i.trim().trim_end_matches(';').trim().to_string())),
            None => (rest.trim_end_matches(';').trim(), None),
        };
        let ty = parse_type(type_text, it, ln)?;
        out.push(Field { name, ty, attrs, init, comment });
    }
    Err(LayoutError::Parse { line: None, msg: format!("missing {terminator}") })
}

fn split_name(code: &str) -> Option<(String, &str)> {
    let code = code.trim();
    if let Some(rest) = code.strip_prefix('"') {
        let end = rest.find('"')?;
        return Some((rest[..end].to_string(), &rest[end + 1..]));
    }
    let end = code.find(|c: char| c == ' ' || c == '{' || c == ':').unwrap_or(code.len());
    Some((code[..end].to_string(), &code[end..]))
}

fn parse_type(text: &str, it: &mut Lines, ln: usize) -> Result<TypeRef, LayoutError> {
    let t = text.trim();
    let upper = t.to_ascii_uppercase();
    if upper == "STRUCT" {
        let fields = parse_fields(it, "END_STRUCT")?;
        return Ok(TypeRef::Struct(fields));
    }
    if upper.starts_with("ARRAY") {
        let open = t.find('[').ok_or_else(|| perr(ln, "array without ["))?;
        let close = t.find(']').ok_or_else(|| perr(ln, "array without ]"))?;
        let dims_text = &t[open + 1..close];
        let mut dims = Vec::new();
        for d in dims_text.split(',') {
            let (lo, hi) = d.split_once("..").ok_or_else(|| perr(ln, format!("bad array dims: {d}")))?;
            dims.push((parse_bound(lo)?, parse_bound(hi)?));
        }
        let rest = t[close + 1..].trim();
        let elem_text = rest
            .strip_prefix("of ")
            .or_else(|| rest.strip_prefix("OF "))
            .or_else(|| rest.strip_prefix("Of "))
            .ok_or_else(|| perr(ln, "array without 'of'"))?;
        let elem = parse_type(elem_text, it, ln)?;
        return Ok(TypeRef::Array { dims, elem: Box::new(elem) });
    }
    if let Some(inner) = t.strip_prefix('"') {
        let name = inner.trim_end_matches('"');
        return Ok(TypeRef::Udt(name.to_string()));
    }
    Prim::from_name(t).map(TypeRef::Prim).ok_or_else(|| perr(ln, format!("unsupported type: {t}")))
}

fn parse_bound(s: &str) -> Result<Bound, LayoutError> {
    let s = s.trim();
    if let Some(inner) = s.strip_prefix('"') {
        return Ok(Bound::Const(inner.trim_end_matches('"').to_string()));
    }
    s.parse::<i64>().map(Bound::Int).map_err(|_| LayoutError::Parse { line: None, msg: format!("bad bound {s}") })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_udt_with_nested_struct_and_arrays() {
        let src = r#"TYPE "T"
VERSION : 0.1
   STRUCT
      A { S7_SetPoint := 'False'} : UInt;   // comment
      B : Struct   // nested
         X : Bool;
         Y : Array["X".."G"] of Real;
      END_STRUCT;
      C : Array[0..19] of Real := 0.0;
      "Quoted Name" : "Other_Udt";
      S : String[10];
      D {InstructionName := 'DTL'; LibVersion := '1.0'} : DTL;
   END_STRUCT;

END_TYPE
"#;
        let u = parse_udt(src).unwrap();
        assert_eq!(u.name, "T");
        assert_eq!(u.fields.len(), 6);
        assert_eq!(u.fields[0].comment, "comment");
        assert!(matches!(u.fields[1].ty, TypeRef::Struct(ref f) if f.len() == 2));
        assert!(matches!(u.fields[3].ty, TypeRef::Udt(ref n) if n == "Other_Udt"));
        assert_eq!(u.fields[3].name, "Quoted Name");
        assert!(matches!(u.fields[4].ty, TypeRef::Prim(Prim::String(10))));
        assert!(matches!(u.fields[5].ty, TypeRef::Prim(Prim::Dtl)));
    }

    #[test]
    fn parses_db_sections_and_attrs() {
        let src = "DATA_BLOCK \"D\"\nTITLE = X\n{ DB_Accessible_From_Webserver := 'FALSE' ;\n S7_Optimized_Access := 'FALSE' }\nVERSION : 0.1\nNON_RETAIN\n   VAR RETAIN\n      Head : Int;\n   END_VAR\n   VAR \n      Reset : Bool;\n   END_VAR\n\nBEGIN\n   Head := 3;\nEND_DATA_BLOCK\n";
        let d = parse_db(src).unwrap();
        assert_eq!(d.name, "D");
        assert!(!d.optimized);
        assert_eq!(d.fields.len(), 2);
        assert_eq!(d.begin, vec![("Head".to_string(), "3".to_string())]);
    }
}
