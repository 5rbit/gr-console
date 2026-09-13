//! `JsonW_<Udt>` / `JsonR_<Udt>` SCL functions on top of the Json_W* / Json_R* runtime
//! (interfaces: siemens `docs/link/plc-blocks.md` sections 2-4).
//!
//! Writer: key literals with the separating comma and opening brackets merged into one `Json_WRaw` call (at most
//! 64 characters), one value call per elementary member, `FOR` loops over arrays with the declared bounds (constant
//! names kept), nested UDTs through their own `JsonW_` function, anonymous structs inline.
//! Reader: `Json_RObjBegin` + `WHILE Json_RNextKey` with an `IF`/`ELSIF` chain in declaration order (unknown keys
//! skipped), arrays with an index guard (extra elements skipped), String members through a `String` temp.

use plc_layout::Contract;
use plc_layout::ast::{Bound, Field, Prim, TypeRef};

use crate::error::{ErrCode, LinkError};
use crate::wire_json::push_name;

use super::names::{MAX_LIT_LEN, Names, member, scl_str};
use super::text::GENERATED_MARKER;

const COMMA: &str = "\"Json_WComma\"(Buf := #Buf, Pos := #Pos);";
const SKIP: &str = "\"Json_RSkip\"(Buf := #Buf, Pos := #Pos);";

/// Statement lines of a block body: a tab plus two spaces per nesting level (style of the runtime sources).
pub(crate) struct Code {
    out: String,
    level: usize,
}

impl Code {
    pub(crate) fn new() -> Self {
        Code { out: String::new(), level: 0 }
    }

    pub(crate) fn line(&mut self, s: &str) {
        self.out.push('\t');
        for _ in 0..self.level {
            self.out.push_str("  ");
        }
        self.out.push_str(s);
        self.out.push('\n');
    }

    /// Line opening a nested block (`IF … THEN`, `WHILE … DO`).
    fn open(&mut self, s: &str) {
        self.line(s);
        self.level += 1;
    }

    /// Line at the level of the enclosing statement (`ELSIF`, `ELSE`).
    fn mid(&mut self, s: &str) {
        self.level -= 1;
        self.line(s);
        self.level += 1;
    }

    fn close(&mut self, s: &str) {
        self.level -= 1;
        self.line(s);
    }

    pub(crate) fn finish(self) -> String {
        self.out
    }
}

/// One declared variable of a block interface.
pub(crate) struct Var {
    name: String,
    ty: String,
    comment: String,
}

impl Var {
    pub(crate) fn new(name: impl Into<String>, ty: impl Into<String>, comment: impl Into<String>) -> Self {
        Var { name: name.into(), ty: ty.into(), comment: comment.into() }
    }
}

pub(crate) fn buf_var(write: bool) -> Var {
    Var::new("Buf", "Array[*] of Byte", if write { "출력 버퍼" } else { "입력 버퍼" })
}

pub(crate) fn pos_var(write: bool) -> Var {
    Var::new("Pos", "DInt", if write { "다음 쓸 절대 인덱스 (< 0 = 오류 유지)" } else { "다음 읽을 절대 인덱스 (< 0 = 오류 유지)" })
}

fn section(kw: &str, vars: &[Var]) -> String {
    let mut s = format!("   {kw}\n");
    for v in vars {
        s.push_str(&format!("      {} : {};", v.name, v.ty));
        if !v.comment.is_empty() {
            s.push_str("   // ");
            s.push_str(&v.comment);
        }
        s.push('\n');
    }
    s.push_str("   END_VAR\n");
    s
}

/// `FUNCTION "<name>" : Void` source in TIA export layout (optimized access, no String lengths in parameters).
pub(crate) fn fc_source(name: &str, inputs: &[Var], in_outs: &[Var], temps: &[Var], body: &str) -> String {
    let mut sections = Vec::new();
    if !inputs.is_empty() {
        sections.push(section("VAR_INPUT", inputs));
    }
    sections.push(section("VAR_IN_OUT", in_outs));
    if !temps.is_empty() {
        sections.push(section("VAR_TEMP", temps));
    }
    format!("FUNCTION \"{name}\" : Void\n{{ S7_Optimized_Access := 'TRUE' }}\nVERSION : 0.1\n{}\n\nBEGIN\n{body}END_FUNCTION\n", sections.join("\n"))
}

pub(crate) fn call_val(fc: &str, val: &str) -> String {
    format!("\"{fc}\"(Buf := #Buf, Pos := #Pos, Val := {val});")
}

pub(crate) fn call_raw(lit: &str) -> String {
    format!("\"Json_WRaw\"(Buf := #Buf, Pos := #Pos, Lit := '{}');", scl_str(lit))
}

/// A `Json_WRaw` literal must fit the runtime work area.
pub(crate) fn check_lit(lit: &str, path: &str) -> Result<(), LinkError> {
    if lit.len() > MAX_LIT_LEN {
        return Err(LinkError::at(ErrCode::Parse, path, format!("JSON literal {lit:?} has {} characters, Json_WRaw accepts at most {MAX_LIT_LEN}", lit.len())));
    }
    Ok(())
}

/// Runtime function for an elementary type: `Json_W<T>` (`dir` = 'W') or `Json_R<T>` (`dir` = 'R').
fn prim_fc(dir: char, p: Prim) -> Option<String> {
    let t = match p {
        Prim::Bool => "Bool",
        Prim::Byte => "Byte",
        Prim::Word => "Word",
        Prim::DWord => "DWord",
        Prim::LWord => "LWord",
        Prim::SInt => "SInt",
        Prim::Int => "Int",
        Prim::DInt => "DInt",
        Prim::LInt => "LInt",
        Prim::USInt => "USInt",
        Prim::UInt => "UInt",
        Prim::UDInt => "UDInt",
        Prim::ULInt => "ULInt",
        Prim::Real => "Real",
        Prim::LReal => "LReal",
        Prim::Char => "Char",
        Prim::String(_) => "Str",
        Prim::Time => "Time",
        Prim::Dtl => "Dtl",
        Prim::Date | Prim::Tod | Prim::DateAndTime | Prim::LTime => return None,
    };
    Some(format!("Json_{dir}{t}"))
}

fn bound_text(b: &Bound) -> String {
    match b {
        Bound::Int(i) => i.to_string(),
        Bound::Const(n) => format!("\"{n}\""),
    }
}

/// Resolved bounds of one array dimension (TIA rejects a DInt FOR variable with constant bounds of another integer
/// type) and a trailing comment naming the declared constant bounds, if any.
fn dim_bounds(c: &Contract, d: &(Bound, Bound), path: &str) -> Result<(i64, i64, String), LinkError> {
    let resolve = |b: &Bound| c.bound(b).map_err(|e| LinkError::at(ErrCode::Parse, path, e.to_string()));
    let (lo, hi) = (resolve(&d.0)?, resolve(&d.1)?);
    let named = matches!(d.0, Bound::Const(_)) || matches!(d.1, Bound::Const(_));
    let comment = if named { format!("   // {}..{}", bound_text(&d.0), bound_text(&d.1)) } else { String::new() };
    Ok((lo, hi, comment))
}

fn join(path: &str, name: &str) -> String {
    if path.is_empty() { name.to_string() } else { format!("{path}.{name}") }
}

/// `"Name":`, with a leading comma for every member but the first.
fn json_key(name: &str, comma: bool) -> String {
    let mut s = String::with_capacity(name.len() + 4);
    if comma {
        s.push(',');
    }
    s.push('"');
    push_name(&mut s, name);
    s.push_str("\":");
    s
}

/// Writer statements before literal rendering.
enum WOp {
    Raw(String),
    Stmt(String),
    For { var: String, lo: i64, hi: i64, comment: String, body: Vec<WOp> },
}

/// Collects writer statements, merging consecutive punctuation / key text into one literal.
#[derive(Default)]
struct WEmit {
    ops: Vec<WOp>,
    pending: String,
}

impl WEmit {
    fn lit(&mut self, tok: &str, path: &str) -> Result<(), LinkError> {
        check_lit(tok, path)?;
        if self.pending.len() + tok.len() > MAX_LIT_LEN {
            self.flush();
        }
        self.pending.push_str(tok);
        Ok(())
    }

    fn flush(&mut self) {
        if !self.pending.is_empty() {
            self.ops.push(WOp::Raw(std::mem::take(&mut self.pending)));
        }
    }

    fn stmt(&mut self, s: String) {
        self.flush();
        self.ops.push(WOp::Stmt(s));
    }

    fn finish(mut self) -> Vec<WOp> {
        self.flush();
        self.ops
    }
}

/// Array elements are separated by a comma: merged into the element's first literal when it fits, otherwise
/// `Json_WComma`.
fn render_writer(code: &mut Code, ops: &[WOp]) {
    for op in ops {
        match op {
            WOp::Raw(t) => code.line(&call_raw(t)),
            WOp::Stmt(s) => code.line(s),
            WOp::For { var, lo, hi, comment, body } => {
                code.open(&format!("FOR {var} := {lo} TO {hi} DO{comment}"));
                code.open(&format!("IF {var} > {lo} THEN"));
                let rest = match body.first() {
                    Some(WOp::Raw(first)) if first.len() < MAX_LIT_LEN => {
                        code.line(&call_raw(&format!(",{first}")));
                        code.mid("ELSE");
                        code.line(&call_raw(first));
                        &body[1..]
                    }
                    _ => {
                        code.line(COMMA);
                        &body[..]
                    }
                };
                code.close("END_IF;");
                render_writer(code, rest);
                code.close("END_FOR;");
            }
        }
    }
}

struct Gen<'a> {
    c: &'a Contract,
    names: &'a Names<'a>,
    udt: &'a str,
    /// Loop index temps `i0..` in use.
    loops: usize,
    /// Key temps `k0..` in use (reader).
    keys: usize,
    /// String temp `s` in use (reader).
    string_tmp: bool,
}

impl<'a> Gen<'a> {
    fn new(c: &'a Contract, names: &'a Names<'a>, udt: &'a str) -> Self {
        Gen { c, names, udt, loops: 0, keys: 0, string_tmp: false }
    }

    fn at(&self, path: &str) -> String {
        if path.is_empty() { self.udt.to_string() } else { format!("{}.{path}", self.udt) }
    }

    fn unsupported(&self, path: &str, p: Prim) -> LinkError {
        LinkError::at(ErrCode::Parse, self.at(path), format!("type {} is not supported on the link", p.name()))
    }

    fn check_name(&self, name: &str, path: &str) -> Result<(), LinkError> {
        if !name.is_ascii() || name.chars().any(|c| c.is_ascii_control()) {
            return Err(LinkError::at(ErrCode::Parse, self.at(path), format!("member name {name:?} cannot be a link JSON key (printable ASCII only)")));
        }
        if name.len() > MAX_LIT_LEN {
            return Err(LinkError::at(ErrCode::Parse, self.at(path), format!("member name {name:?} is longer than the {MAX_LIT_LEN}-character JSON key limit")));
        }
        Ok(())
    }

    // ---- writer

    fn w_fields(&mut self, e: &mut WEmit, fields: &[Field], access: &str, path: &str, depth: usize) -> Result<(), LinkError> {
        e.lit("{", &self.at(path))?;
        for (i, f) in fields.iter().enumerate() {
            let p = join(path, &f.name);
            self.check_name(&f.name, &p)?;
            e.lit(&json_key(&f.name, i > 0), &self.at(&p))?;
            self.w_ty(e, &f.ty, &format!("{access}.{}", member(&f.name)), &p, depth)?;
        }
        e.lit("}", &self.at(path))
    }

    fn w_ty(&mut self, e: &mut WEmit, ty: &TypeRef, access: &str, path: &str, depth: usize) -> Result<(), LinkError> {
        match ty {
            TypeRef::Prim(p) => {
                let fc = prim_fc('W', *p).ok_or_else(|| self.unsupported(path, *p))?;
                e.stmt(call_val(&fc, access));
            }
            TypeRef::Udt(n) => e.stmt(call_val(&self.names.writer(n)?, access)),
            TypeRef::Struct(fields) => self.w_fields(e, fields, access, path, depth)?,
            TypeRef::Array { dims, elem } => self.w_array(e, dims, elem, access, &mut Vec::new(), path, depth)?,
        }
        Ok(())
    }

    /// One dimension per `FOR` (row-major, first dimension outermost); `idx` = loop variables of the outer dimensions.
    #[allow(clippy::too_many_arguments)]
    fn w_array(&mut self, e: &mut WEmit, dims: &[(Bound, Bound)], elem: &TypeRef, base: &str, idx: &mut Vec<String>, path: &str, depth: usize) -> Result<(), LinkError> {
        e.lit("[", &self.at(path))?;
        e.flush();
        let (lo, hi, comment) = dim_bounds(self.c, &dims[0], &self.at(path))?;
        let var = format!("#i{depth}");
        self.loops = self.loops.max(depth + 1);
        idx.push(var.clone());
        let mut body = WEmit::default();
        let elem_path = format!("{path}[]");
        if dims.len() > 1 {
            self.w_array(&mut body, &dims[1..], elem, base, idx, &elem_path, depth + 1)?;
        } else {
            let access = format!("{base}[{}]", idx.join(", "));
            self.w_ty(&mut body, elem, &access, &elem_path, depth + 1)?;
        }
        idx.pop();
        e.ops.push(WOp::For { var, lo, hi, comment, body: body.finish() });
        e.lit("]", &self.at(path))
    }

    // ---- reader

    fn r_fields(&mut self, code: &mut Code, fields: &[Field], access: &str, path: &str, kd: usize, ld: usize) -> Result<(), LinkError> {
        let k = format!("#k{kd}");
        self.keys = self.keys.max(kd + 1);
        code.open("IF \"Json_RObjBegin\"(Buf := #Buf, Pos := #Pos) THEN");
        code.open(&format!("WHILE \"Json_RNextKey\"(Buf := #Buf, Pos := #Pos, Key => {k}) DO"));
        for (i, f) in fields.iter().enumerate() {
            let p = join(path, &f.name);
            self.check_name(&f.name, &p)?;
            let cond = format!("{} {k} = '{}' THEN", if i == 0 { "IF" } else { "ELSIF" }, scl_str(&f.name));
            if i == 0 {
                code.open(&cond);
            } else {
                code.mid(&cond);
            }
            self.r_ty(code, &f.ty, &format!("{access}.{}", member(&f.name)), &p, kd, ld)?;
        }
        if fields.is_empty() {
            code.line(SKIP);
        } else {
            code.mid("ELSE");
            code.line(&format!("{SKIP}   // 모르는 키"));
            code.close("END_IF;");
        }
        code.close("END_WHILE;");
        code.close("END_IF;");
        Ok(())
    }

    fn r_ty(&mut self, code: &mut Code, ty: &TypeRef, access: &str, path: &str, kd: usize, ld: usize) -> Result<(), LinkError> {
        match ty {
            TypeRef::Prim(Prim::String(n)) => {
                self.string_tmp = true;
                code.line(&format!("\"Json_RStr\"(Buf := #Buf, Pos := #Pos, MaxLen := {n}, Val := #s);"));
                code.open("IF #Pos >= 0 THEN");
                code.line(&format!("{access} := #s;"));
                code.close("END_IF;");
            }
            TypeRef::Prim(p) => {
                let fc = prim_fc('R', *p).ok_or_else(|| self.unsupported(path, *p))?;
                code.line(&call_val(&fc, access));
            }
            TypeRef::Udt(n) => code.line(&call_val(&self.names.reader(n)?, access)),
            TypeRef::Struct(fields) => self.r_fields(code, fields, access, path, kd + 1, ld)?,
            TypeRef::Array { dims, elem } => self.r_array(code, dims, elem, access, &mut Vec::new(), path, kd, ld)?,
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn r_array(&mut self, code: &mut Code, dims: &[(Bound, Bound)], elem: &TypeRef, base: &str, idx: &mut Vec<String>, path: &str, kd: usize, ld: usize) -> Result<(), LinkError> {
        let var = format!("#i{ld}");
        self.loops = self.loops.max(ld + 1);
        let (lo, hi, comment) = dim_bounds(self.c, &dims[0], &self.at(path))?;
        code.line(&format!("{var} := {lo};{comment}"));
        code.open("IF \"Json_RArrBegin\"(Buf := #Buf, Pos := #Pos) THEN");
        code.open("WHILE \"Json_RArrNext\"(Buf := #Buf, Pos := #Pos) DO");
        code.open(&format!("IF {var} <= {hi} THEN"));
        idx.push(var.clone());
        let elem_path = format!("{path}[]");
        if dims.len() > 1 {
            self.r_array(code, &dims[1..], elem, base, idx, &elem_path, kd, ld + 1)?;
        } else {
            let access = format!("{base}[{}]", idx.join(", "));
            self.r_ty(code, elem, &access, &elem_path, kd, ld + 1)?;
        }
        idx.pop();
        code.mid("ELSE");
        code.line(&format!("{SKIP}   // 초과 요소"));
        code.close("END_IF;");
        code.line(&format!("{var} := {var} + 1;"));
        code.close("END_WHILE;");
        code.line("\"Json_RArrEnd\"(Buf := #Buf, Pos := #Pos);");
        code.close("END_IF;");
        Ok(())
    }
}

/// `JsonW_<Udt>` source.
pub(crate) fn writer_fc(c: &Contract, names: &Names, udt: &str, name: &str) -> Result<String, LinkError> {
    let decl = c.udt(udt)?;
    let sig = c.udt_sig(udt)?;
    let mut g = Gen::new(c, names, udt);
    let mut e = WEmit::default();
    g.w_fields(&mut e, &decl.fields, "#Val", "", 0)?;
    let ops = e.finish();
    let mut code = Code::new();
    code.line(&format!("{GENERATED_MARKER} from \"{udt}\" sig 16#{sig:08X} - do not edit"));
    code.line("// wire-spec 2 : 선언 순서, 공백 없음. 버퍼가 모자라면 Pos := LNK_JERR_OVERFLOW (끈끈한 오류, 호출자가 끝에 한 번 확인)");
    render_writer(&mut code, &ops);
    let temps: Vec<Var> = (0..g.loops).map(|i| Var::new(format!("i{i}"), "DInt", "")).collect();
    let io = [buf_var(true), pos_var(true), Var::new("Val", format!("\"{udt}\""), "쓸 값")];
    Ok(fc_source(name, &[], &io, &temps, &code.finish()))
}

/// `JsonR_<Udt>` source.
pub(crate) fn reader_fc(c: &Contract, names: &Names, udt: &str, name: &str) -> Result<String, LinkError> {
    let decl = c.udt(udt)?;
    let sig = c.udt_sig(udt)?;
    let mut g = Gen::new(c, names, udt);
    let mut code = Code::new();
    code.line(&format!("{GENERATED_MARKER} from \"{udt}\" sig 16#{sig:08X} - do not edit"));
    code.line("// 키 순서 무관, 모르는 키 / 초과 배열 요소는 건너뜀, 없는 키는 그대로. 오류면 Pos := LNK_JERR_PARSE (끈끈한 오류, 호출자가 끝에 한 번 확인)");
    g.r_fields(&mut code, &decl.fields, "#Val", "", 0, 0)?;
    let mut temps: Vec<Var> = (0..g.loops).map(|i| Var::new(format!("i{i}"), "DInt", "")).collect();
    temps.extend((0..g.keys).map(|i| Var::new(format!("k{i}"), "String", "멤버 키 (Json_RNextKey)")));
    if g.string_tmp {
        temps.push(Var::new("s", "String", "문자열 멤버 임시 (Json_RStr)"));
    }
    let io = [buf_var(false), pos_var(false), Var::new("Val", format!("\"{udt}\""), "읽은 값")];
    Ok(fc_source(name, &[], &io, &temps, &code.finish()))
}
