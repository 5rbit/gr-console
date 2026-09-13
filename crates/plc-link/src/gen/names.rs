//! Generated identifiers: block names (registry prefixes, TIA length limit, `[alias]`), constant names, SCL member
//! access and SCL string literals.

use crate::error::LinkError;
use crate::registry::RegistryDoc;

/// TIA block name limit (characters).
pub const MAX_NAME_LEN: usize = 120;
/// Longest `Json_WRaw` literal and `Json_RNextKey` key (runtime `String[64]` work areas).
pub const MAX_LIT_LEN: usize = 64;

/// Name factory bound to a registry document.
pub struct Names<'a> {
    doc: &'a RegistryDoc,
}

impl<'a> Names<'a> {
    pub fn new(doc: &'a RegistryDoc) -> Self {
        Names { doc }
    }

    fn block(&self, generated: String, from: &str) -> Result<String, LinkError> {
        if let Some(short) = self.doc.alias.get(&generated) {
            if short.chars().count() > MAX_NAME_LEN {
                return Err(LinkError::parse(format!("[alias] \"{generated}\" = \"{short}\": the short name is longer than {MAX_NAME_LEN} characters")));
            }
            return Ok(short.clone());
        }
        let n = generated.chars().count();
        if n > MAX_NAME_LEN {
            return Err(LinkError::parse(format!(
                "generated block name \"{generated}\" (from {from}) has {n} characters, TIA allows {MAX_NAME_LEN}; add a short name to [alias] in messages.toml: \"{generated}\" = \"<short name>\""
            )));
        }
        Ok(generated)
    }

    /// `JsonW_<Udt>` (or its alias).
    pub fn writer(&self, udt: &str) -> Result<String, LinkError> {
        self.block(format!("{}{udt}", self.doc.writer_prefix), &format!("UDT \"{udt}\""))
    }

    /// `JsonR_<Udt>` (or its alias).
    pub fn reader(&self, udt: &str) -> Result<String, LinkError> {
        self.block(format!("{}{udt}", self.doc.reader_prefix), &format!("UDT \"{udt}\""))
    }

    /// `LnkJ_<Msg>` (or its alias).
    pub fn envelope(&self, msg: &str) -> Result<String, LinkError> {
        self.block(format!("{}{msg}", self.doc.envelope_prefix), &format!("message {msg}"))
    }

    /// `LNK_<KIND>_<MSG>` (e.g. `LNK_SIG_MEASLOG`).
    pub fn constant(&self, kind: &str, msg: &str) -> String {
        format!("{}{kind}_{}", self.doc.const_prefix, msg.to_ascii_uppercase())
    }

    /// Constant table `LNK_Const_Gen`.
    pub fn table(&self) -> String {
        format!("{}Const_Gen", self.doc.const_prefix)
    }

    /// Test vector DB `LNK_TestVectors`.
    pub fn test_db(&self) -> String {
        format!("{}TestVectors", self.doc.const_prefix)
    }
}

/// SCL keywords and elementary type names that cannot follow `.` unquoted.
const RESERVED: &[&str] = &[
    "AND",
    "ANY",
    "ARRAY",
    "AT",
    "BEGIN",
    "BOOL",
    "BY",
    "BYTE",
    "CASE",
    "CHAR",
    "CONST",
    "CONTINUE",
    "DATE",
    "DATE_AND_TIME",
    "DINT",
    "DO",
    "DT",
    "DTL",
    "DWORD",
    "ELSE",
    "ELSIF",
    "END_CASE",
    "END_FOR",
    "END_FUNCTION",
    "END_IF",
    "END_REGION",
    "END_REPEAT",
    "END_STRUCT",
    "END_TYPE",
    "END_VAR",
    "END_WHILE",
    "EXIT",
    "FALSE",
    "FOR",
    "FUNCTION",
    "FUNCTION_BLOCK",
    "GOTO",
    "IF",
    "INT",
    "LDT",
    "LINT",
    "LREAL",
    "LTIME",
    "LTOD",
    "LWORD",
    "MOD",
    "NOT",
    "OF",
    "OR",
    "REAL",
    "REGION",
    "REPEAT",
    "RETURN",
    "S5TIME",
    "SINT",
    "STRING",
    "STRUCT",
    "THEN",
    "TIME",
    "TIME_OF_DAY",
    "TO",
    "TOD",
    "TRUE",
    "TYPE",
    "UDINT",
    "UINT",
    "ULINT",
    "UNTIL",
    "USINT",
    "VAR",
    "VAR_INPUT",
    "VAR_IN_OUT",
    "VAR_OUTPUT",
    "VAR_TEMP",
    "VOID",
    "WCHAR",
    "WHILE",
    "WORD",
    "WSTRING",
    "XOR",
];

/// SCL member access segment: plain identifiers as written, anything else (or a reserved word) quoted.
pub fn member(name: &str) -> String {
    let mut ch = name.chars();
    let plain = ch.next().is_some_and(|c| c.is_ascii_alphabetic() || c == '_') && ch.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if plain && !RESERVED.contains(&name.to_ascii_uppercase().as_str()) { name.to_string() } else { format!("\"{name}\"") }
}

/// Content of an SCL character string literal: `$` → `$$`, `'` → `$'`.
pub fn scl_str(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for ch in s.chars() {
        match ch {
            '$' => out.push_str("$$"),
            '\'' => out.push_str("$'"),
            c => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn member_and_literal_escaping() {
        assert_eq!(member("Position"), "Position");
        assert_eq!(member("Time"), "\"Time\"");
        assert_eq!(member("Quoted Name"), "\"Quoted Name\"");
        assert_eq!(scl_str("it's $5"), "it$'s $$5");
    }
}
