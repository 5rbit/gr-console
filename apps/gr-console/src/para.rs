//! `GET /api/para?robot=<id>` — the robot's `PARA` DB decoded for reading, grouped by its top-level structs,
//! with the TIA declaration comments (`[p22] Z_BeltThickness`) and type names from the contract.
//!
//! Read-only on purpose: PARA is the machine's own parameter set (TIA / HMI owns writes). Values come from the
//! slow-tier snapshot (`at` says how old); arrays stay one row with a JSON array value.

use axum::Router;
use axum::extract::{Query, State};
use axum::routing::get;
use plc_layout::docs::MemberDoc;
use serde::{Deserialize, Serialize};
use serde_json::{Value as Json, json};

use crate::error::{ApiError, ApiResult};
use crate::plc::ensure_db;
use crate::state::AppState;

const DB: &str = "PARA";

#[derive(Deserialize)]
struct ParaQuery {
    robot: Option<u8>,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ParaRow {
    /// Full path, e.g. `Sensor.GIDL_ZOffset`.
    pub path: String,
    /// Last segment.
    pub name: String,
    pub ty: String,
    pub comment: String,
    /// Parameter number parsed from a `[pNNN]` comment prefix (first number of a range).
    pub param: Option<u32>,
    /// Depth inside the group (0 = direct member of the top-level struct).
    pub depth: usize,
    /// `false` = sub-struct header row (value is `null`).
    pub leaf: bool,
    pub value: Json,
}

#[derive(Debug, PartialEq, Serialize)]
pub struct ParaGroup {
    pub name: String,
    pub ty: String,
    pub comment: String,
    pub rows: Vec<ParaRow>,
}

/// `[p22] Z_BeltThickness` → 22, `[p50-p53]` → 50, `[p001 - p099] 장비` → 1.
pub fn param_no(comment: &str) -> Option<u32> {
    let i = comment.find("[p")?;
    let digits: String = comment[i + 2..].chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn pointer(path: &str) -> String {
    path.split('.').map(|s| format!("/{}", s.replace('~', "~0").replace('/', "~1"))).collect()
}

/// Groups the declaration docs by top-level member and attaches the decoded values.
pub fn build(docs: &[MemberDoc], data: &Json) -> Vec<ParaGroup> {
    let mut groups: Vec<ParaGroup> = Vec::new();
    for d in docs {
        let value = if d.leaf { data.pointer(&pointer(&d.path)).cloned().unwrap_or(Json::Null) } else { Json::Null };
        let name = d.path.rsplit('.').next().unwrap_or(&d.path).to_string();
        if d.depth == 0 {
            if d.leaf {
                // a top-level scalar: its own one-row group
                let row = ParaRow { path: d.path.clone(), name: name.clone(), ty: d.ty.clone(), comment: d.comment.clone(), param: param_no(&d.comment), depth: 0, leaf: true, value };
                groups.push(ParaGroup { name, ty: d.ty.clone(), comment: d.comment.clone(), rows: vec![row] });
            } else {
                groups.push(ParaGroup { name, ty: d.ty.clone(), comment: d.comment.clone(), rows: Vec::new() });
            }
            continue;
        }
        if let Some(g) = groups.last_mut() {
            g.rows.push(ParaRow { path: d.path.clone(), name, ty: d.ty.clone(), comment: d.comment.clone(), param: param_no(&d.comment), depth: d.depth - 1, leaf: d.leaf, value });
        }
    }
    groups
}

async fn para(State(st): State<AppState>, Query(q): Query<ParaQuery>) -> ApiResult<Json> {
    let (r, h) = st.robot_and_plc(q.robot)?;
    ensure_db(h, DB)?;
    let snap = h.snap();
    let d = snap.db(DB).ok_or_else(|| ApiError::PlcUnavailable(format!("{}.{DB} not read yet", h.name())))?;
    let docs = h.contract.member_docs(DB)?;
    let groups = build(&docs, &d.json);
    Ok(axum::Json(json!({ "robot": r.id, "robot_name": r.name, "plc": h.name(), "contract": h.cfg.contract, "at": d.at, "seq": d.seq, "groups": groups })))
}

pub fn router() -> Router<AppState> {
    Router::new().route("/api/para", get(para))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(path: &str, ty: &str, comment: &str, depth: usize, leaf: bool) -> MemberDoc {
        MemberDoc { path: path.into(), ty: ty.into(), comment: comment.into(), depth, leaf }
    }

    #[test]
    fn param_numbers() {
        assert_eq!(param_no("[p22] Z_BeltThickness"), Some(22));
        assert_eq!(param_no("      [p50-p53]"), Some(50));
        assert_eq!(param_no("[p001 - p099] 장비 파라미터"), Some(1));
        assert_eq!(param_no("no number"), None);
        assert_eq!(param_no("[px]"), None);
    }

    #[test]
    fn groups_rows_and_values() {
        let docs = vec![
            doc("Machine", "Struct", "[p001 - p099] 장비", 0, false),
            doc("Machine.ID", "USInt", "[p1] Machine ID", 1, true),
            doc("Machine.T", "Array[1..2] of Real", "[p50-p53]", 1, true),
            doc("Machine.Sub", "LGR_X", "", 1, false),
            doc("Machine.Sub.A", "Real", "[p9] a", 2, true),
            doc("Ver", "UInt", "", 0, true),
        ];
        let data = json!({ "Machine": { "ID": 2, "T": [1.5, 2.5], "Sub": { "A": 0.25 } }, "Ver": 7 });
        let g = build(&docs, &data);
        assert_eq!(g.len(), 2);
        assert_eq!((g[0].name.as_str(), g[0].rows.len()), ("Machine", 4));
        assert_eq!(g[0].rows[0].value, json!(2));
        assert_eq!(g[0].rows[0].param, Some(1));
        assert_eq!(g[0].rows[1].value, json!([1.5, 2.5]));
        assert!(!g[0].rows[2].leaf && g[0].rows[2].value.is_null());
        assert_eq!((g[0].rows[3].depth, g[0].rows[3].value.clone()), (1, json!(0.25)));
        assert_eq!((g[1].name.as_str(), g[1].rows[0].value.clone()), ("Ver", json!(7)));
    }
}
