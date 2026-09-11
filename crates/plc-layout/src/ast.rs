use serde::Serialize;

/// Elementary data types (size in bytes; Bool = 0 → bit).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub enum Prim {
    Bool,
    Byte,
    Char,
    USInt,
    SInt,
    Word,
    Int,
    UInt,
    Date,
    DWord,
    DInt,
    UDInt,
    Real,
    Time,
    Tod,
    LReal,
    LInt,
    ULInt,
    LWord,
    LTime,
    DateAndTime,
    Dtl,
    /// STRING[n]: 2 + n bytes.
    String(u16),
}

impl Prim {
    pub fn from_name(name: &str) -> Option<Prim> {
        let n = name.trim();
        let lower = n.to_ascii_lowercase();
        if let Some(rest) = lower.strip_prefix("string") {
            let rest = rest.trim();
            if rest.is_empty() {
                return Some(Prim::String(254));
            }
            let inner = rest.trim_start_matches('[').trim_end_matches(']').trim();
            return inner.parse::<u16>().ok().map(Prim::String);
        }
        Some(match lower.as_str() {
            "bool" => Prim::Bool,
            "byte" => Prim::Byte,
            "char" => Prim::Char,
            "usint" => Prim::USInt,
            "sint" => Prim::SInt,
            "word" => Prim::Word,
            "int" => Prim::Int,
            "uint" => Prim::UInt,
            "date" => Prim::Date,
            "dword" => Prim::DWord,
            "dint" => Prim::DInt,
            "udint" => Prim::UDInt,
            "real" => Prim::Real,
            "time" => Prim::Time,
            "time_of_day" | "tod" => Prim::Tod,
            "lreal" => Prim::LReal,
            "lint" => Prim::LInt,
            "ulint" => Prim::ULInt,
            "lword" => Prim::LWord,
            "ltime" => Prim::LTime,
            "date_and_time" | "dt" => Prim::DateAndTime,
            "dtl" => Prim::Dtl,
            // hardware / system data types (2-byte identifiers) and misc.
            "hw_any" | "hw_io" | "hw_iosystem" | "hw_device" | "hw_submodule" | "hw_interface" | "hw_module" | "hw_dpslave" | "hw_hsc" | "hw_pwm" | "hw_pto" | "hw_ieport" | "conn_any" | "conn_ouc" | "conn_prg" | "conn_r_id" | "port" | "rtm" | "event_any" | "event_att" | "event_hwint" | "ob_any" | "ob_delay" | "ob_tod" | "ob_cyclic" | "ob_att" | "ob_pcycle" | "ob_hwint" | "ob_diag" | "ob_timeerror" | "ob_startup" | "db_any" | "db_www" | "db_dyn" | "pip" | "s5time" | "wchar" | "remote" | "aom_ident" | "conn_prog" => Prim::Word,
            "ldt" => Prim::LTime,
            _ => return None,
        })
    }

    /// Size in bytes (0 for Bool).
    pub fn size(&self) -> u32 {
        match self {
            Prim::Bool => 0,
            Prim::Byte | Prim::Char | Prim::USInt | Prim::SInt => 1,
            Prim::Word | Prim::Int | Prim::UInt | Prim::Date => 2,
            Prim::DWord | Prim::DInt | Prim::UDInt | Prim::Real | Prim::Time | Prim::Tod => 4,
            Prim::LReal | Prim::LInt | Prim::ULInt | Prim::LWord | Prim::LTime | Prim::DateAndTime => 8,
            Prim::Dtl => 12,
            Prim::String(n) => 2 + *n as u32,
        }
    }

    /// Alignment requirement: 0 bit, 1 byte, 2 word.
    pub fn align(&self) -> u32 {
        match self {
            Prim::Bool => 0,
            Prim::Byte | Prim::Char | Prim::USInt | Prim::SInt => 1,
            _ => 2,
        }
    }

    pub fn name(&self) -> String {
        match self {
            Prim::String(n) => format!("String[{n}]"),
            other => format!("{other:?}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum Bound {
    Int(i64),
    /// Quoted constant name, e.g. `"X"` or `"MEAS_LOG_LAST"`.
    Const(String),
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub enum TypeRef {
    Prim(Prim),
    Udt(String),
    Struct(Vec<Field>),
    /// One or more dimensions (multi-dim arrays are laid out row-major like one flat array).
    Array { dims: Vec<(Bound, Bound)>, elem: Box<TypeRef> },
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Field {
    pub name: String,
    pub ty: TypeRef,
    pub attrs: Vec<(String, String)>,
    pub init: Option<String>,
    pub comment: String,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct DbDecl {
    pub name: String,
    pub optimized: bool,
    pub attrs: Vec<(String, String)>,
    pub fields: Vec<Field>,
    /// `BEGIN` section assignments (`path := value`).
    pub begin: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct UdtDecl {
    pub name: String,
    pub fields: Vec<Field>,
}
