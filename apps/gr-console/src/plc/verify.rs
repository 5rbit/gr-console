//! Connect-time layout verification: DB size probe, LayoutSig, semantic checks.

use std::collections::HashMap;

use plc_layout::decode::decode_member;
use plc_layout::signature::SIG_MEMBER;
use plc_layout::{Contract, Layout};
use s7::{ProbeResult, S7Client};
use serde::Serialize;

use crate::config::PlcCfg;

#[derive(Clone, Debug, Serialize, PartialEq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckResult {
    Ok,
    Missing,
    SizeMismatch { expected: u32, actual: Option<u32> },
    SigMismatch { expected: u32, actual: u32 },
    Semantic { path: String, expected: i64, actual: String },
    Error { message: String },
}

impl CheckResult {
    pub fn text(&self) -> String {
        match self {
            CheckResult::Ok => "ok".into(),
            CheckResult::Missing => "DB does not exist".into(),
            CheckResult::SizeMismatch { expected, actual } => match actual {
                Some(a) => format!("size {a} B, expected {expected} B"),
                None => format!("size differs from expected {expected} B"),
            },
            CheckResult::SigMismatch { expected, actual } => format!("LayoutSig 16#{actual:08X}, expected 16#{expected:08X}"),
            CheckResult::Semantic { path, expected, actual } => format!("{path} = {actual}, expected {expected}"),
            CheckResult::Error { message } => message.clone(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
pub struct DbCheck {
    pub db: String,
    pub number: u16,
    pub expected_size: u32,
    pub expected_sig: Option<u32>,
    pub result: CheckResult,
}

pub async fn verify(client: &mut S7Client, cfg: &PlcCfg, contract: &Contract, layouts: &HashMap<String, Layout>) -> Vec<DbCheck> {
    let mut out = Vec::new();
    for db in cfg.all_dbs() {
        let (Some(number), Some(layout)) = (contract.db_number(&db), layouts.get(&db)) else {
            out.push(DbCheck { db: db.clone(), number: 0, expected_size: 0, expected_sig: None, result: CheckResult::Error { message: "not in contract".into() } });
            continue;
        };
        let expected_sig = layout.find(SIG_MEMBER).map(|_| contract.layout_sig(&db).unwrap_or(0));
        let mut check = DbCheck { db: db.clone(), number, expected_size: layout.size, expected_sig, result: CheckResult::Ok };
        match client.probe_db_size(number, layout.size).await {
            Ok(ProbeResult::Exact) => {}
            Ok(ProbeResult::Missing) => check.result = CheckResult::Missing,
            Ok(ProbeResult::Larger { actual }) => check.result = CheckResult::SizeMismatch { expected: layout.size, actual },
            Ok(ProbeResult::Smaller { actual }) => check.result = CheckResult::SizeMismatch { expected: layout.size, actual: Some(actual) },
            Err(e) => check.result = CheckResult::Error { message: e.to_string() },
        }
        if check.result == CheckResult::Ok
            && let (Some(m), Some(expected)) = (layout.find(SIG_MEMBER), expected_sig)
        {
            match client.read_db(number, m.offset, 4).await {
                Ok(b) => {
                    let actual = u32::from_be_bytes([b[0], b[1], b[2], b[3]]);
                    if actual != expected {
                        check.result = CheckResult::SigMismatch { expected, actual };
                    }
                }
                Err(e) => check.result = CheckResult::Error { message: e.to_string() },
            }
        }
        if check.result == CheckResult::Ok {
            for sc in cfg.checks.iter().filter(|c| c.db == db) {
                let Some(m) = layout.find(&sc.path) else {
                    check.result = CheckResult::Error { message: format!("check path {} not in layout", sc.path) };
                    break;
                };
                match client.read_db(number, m.offset, m.size.max(1) as usize).await {
                    Ok(b) => {
                        let mut local = m.clone();
                        local.offset = 0;
                        match decode_member(&b, &local) {
                            Ok(v) => {
                                let actual = v.as_f64().unwrap_or(f64::NAN);
                                if (actual - sc.equals as f64).abs() > 1e-6 {
                                    check.result = CheckResult::Semantic { path: sc.path.clone(), expected: sc.equals, actual: format!("{actual}") };
                                    break;
                                }
                            }
                            Err(e) => {
                                check.result = CheckResult::Error { message: e.to_string() };
                                break;
                            }
                        }
                    }
                    Err(e) => {
                        check.result = CheckResult::Error { message: e.to_string() };
                        break;
                    }
                }
            }
        }
        out.push(check);
    }
    out
}
