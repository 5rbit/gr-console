//! Scenario JSON / CSV import-export and validation.
//!
//! JSON export: `{ "schema": 1, "id", "name", "description", "repeat", "steps": [...] }` — import
//! accepts that document or a plain `Scenario` (the id is dropped; a new one is assigned on save).
//! CSV columns: `label,type,target_kind,target_id,item_code,count,wait_for,wait_after_ms,on_failure,
//! note,params` with `params` encoded as `k=v;k=v`.

use gr_proto::{TaskParams, TaskType};
use serde::Serialize;
use serde_json::{Map, Value as Json, json};

use super::{OnFailure, Scenario, Step, WaitFor};
use crate::error::ApiError;
use crate::ledger::Target;
use crate::state::AppState;

pub const SCHEMA: u32 = 1;
pub const CSV_COLUMNS: [&str; 11] = ["label", "type", "target_kind", "target_id", "item_code", "count", "wait_for", "wait_after_ms", "on_failure", "note", "params"];

// ---------------------------------------------------------------------------------------------
// JSON
// ---------------------------------------------------------------------------------------------

pub fn to_json(s: &Scenario) -> Json {
    json!({ "schema": SCHEMA, "id": s.id, "name": s.name, "description": s.description, "repeat": s.repeat, "steps": s.steps })
}

/// Parses an exported document or a raw `Scenario`. The result has no id / timestamps.
pub fn from_json(bytes: &[u8]) -> Result<Scenario, ApiError> {
    let text = std::str::from_utf8(bytes).map_err(|e| ApiError::BadRequest(format!("json: not utf-8 ({e})")))?;
    let text = text.trim_start_matches('\u{feff}');
    let v: Json = serde_json::from_str(text)?;
    if !v.is_object() {
        return Err(ApiError::BadRequest("json: expected an object".into()));
    }
    if let Some(schema) = v.get("schema").and_then(Json::as_u64)
        && schema > SCHEMA as u64
    {
        return Err(ApiError::BadRequest(format!("json: unsupported schema {schema} (max {SCHEMA})")));
    }
    let mut s: Scenario = serde_json::from_value(v)?;
    s.id = String::new();
    s.normalize();
    Ok(s)
}

// ---------------------------------------------------------------------------------------------
// CSV
// ---------------------------------------------------------------------------------------------

/// `{"a":1,"b":true}` → `a=1;b=true` (keys sorted so the output is stable).
pub fn params_to_kv(p: &Json) -> String {
    let Some(obj) = p.as_object() else { return String::new() };
    let mut keys: Vec<&String> = obj.keys().collect();
    keys.sort();
    keys.into_iter()
        .filter_map(|k| {
            let v = &obj[k];
            let s = match v {
                Json::Null => return None,
                Json::String(s) => s.clone(),
                other => other.to_string(),
            };
            Some(format!("{k}={s}"))
        })
        .collect::<Vec<_>>()
        .join(";")
}

/// `a=1;b=true;c=x` → `{"a":1,"b":true,"c":"x"}`.
pub fn params_from_kv(s: &str) -> Result<Json, String> {
    let mut m = Map::new();
    for part in s.split(';').map(str::trim).filter(|p| !p.is_empty()) {
        let (k, v) = part.split_once('=').ok_or_else(|| format!("params: '{part}' is not k=v"))?;
        let k = k.trim();
        let v = v.trim();
        if k.is_empty() {
            return Err(format!("params: empty key in '{part}'"));
        }
        let val = match v {
            "true" | "TRUE" | "True" => Json::Bool(true),
            "false" | "FALSE" | "False" => Json::Bool(false),
            _ => match v.parse::<i64>() {
                Ok(n) => json!(n),
                Err(_) => match v.parse::<f64>() {
                    Ok(f) => json!(f),
                    Err(_) => Json::String(v.to_string()),
                },
            },
        };
        m.insert(k.to_string(), val);
    }
    Ok(Json::Object(m))
}

pub fn to_csv(s: &Scenario) -> Result<String, ApiError> {
    let mut w = csv::Writer::from_writer(Vec::new());
    w.write_record(CSV_COLUMNS).map_err(|e| ApiError::Internal(format!("csv: {e}")))?;
    for st in &s.steps {
        let rec = [
            st.label.clone(),
            st.task_type.name().to_string(),
            st.target.as_ref().map(|t| t.kind.clone()).unwrap_or_default(),
            st.target.as_ref().map(|t| t.id.to_string()).unwrap_or_default(),
            st.item_code.map(|c| c.to_string()).unwrap_or_default(),
            st.count.to_string(),
            st.wait_for.as_str().to_string(),
            st.wait_after_ms.to_string(),
            st.on_failure.as_str().to_string(),
            st.note.clone(),
            params_to_kv(&st.params),
        ];
        w.write_record(rec).map_err(|e| ApiError::Internal(format!("csv: {e}")))?;
    }
    let bytes = w.into_inner().map_err(|e| ApiError::Internal(format!("csv: {e}")))?;
    String::from_utf8(bytes).map_err(|e| ApiError::Internal(format!("csv: {e}")))
}

fn parse_task_type(s: &str) -> Option<TaskType> {
    Some(match s.trim().to_ascii_uppercase().as_str() {
        "UP" => TaskType::Up,
        "PICK" => TaskType::Pick,
        "DROP" => TaskType::Drop,
        "MOVE" => TaskType::Move,
        "MEASURE" => TaskType::Measure,
        _ => return None,
    })
}

/// Parses CSV text (header row required, column order free, BOM tolerated).
pub fn from_csv(text: &str, name: &str) -> Result<Scenario, ApiError> {
    let text = text.trim_start_matches('\u{feff}');
    let mut rdr = csv::ReaderBuilder::new().flexible(true).trim(csv::Trim::All).from_reader(text.as_bytes());
    let headers: Vec<String> = rdr.headers().map_err(|e| ApiError::BadRequest(format!("csv: {e}")))?.iter().map(|h| h.trim().to_ascii_lowercase()).collect();
    let col = |name: &str| headers.iter().position(|h| h == name);
    let (Some(c_type),) = (col("type"),) else {
        return Err(ApiError::BadRequest("csv: missing 'type' column".into()));
    };
    let cols: Vec<Option<usize>> = CSV_COLUMNS.iter().map(|c| col(c)).collect();
    let get = |rec: &csv::StringRecord, i: usize| -> String { cols[i].and_then(|c| rec.get(c)).unwrap_or("").trim().to_string() };
    let mut steps = Vec::new();
    for (n, rec) in rdr.records().enumerate() {
        let rec = rec.map_err(|e| ApiError::BadRequest(format!("csv row {}: {e}", n + 2)))?;
        let row = n + 2;
        let bad = |field: &str, v: &str| ApiError::BadRequest(format!("csv row {row}: invalid {field} '{v}'"));
        let type_s = rec.get(c_type).unwrap_or("").trim();
        if rec.iter().all(|f| f.trim().is_empty()) {
            continue; // blank line
        }
        let task_type = parse_task_type(type_s).ok_or_else(|| bad("type", type_s))?;
        let kind = get(&rec, 2).to_ascii_lowercase();
        let id_s = get(&rec, 3);
        let target = if kind.is_empty() && id_s.is_empty() {
            None
        } else {
            if kind != "cell" && kind != "station" {
                return Err(bad("target_kind", &kind));
            }
            let id: u16 = id_s.parse().map_err(|_| bad("target_id", &id_s))?;
            Some(Target { kind, id })
        };
        let item_s = get(&rec, 4);
        let item_code = if item_s.is_empty() { None } else { Some(item_s.parse::<u32>().map_err(|_| bad("item_code", &item_s))?) };
        let count_s = get(&rec, 5);
        let count = if count_s.is_empty() { 1 } else { count_s.parse::<u32>().map_err(|_| bad("count", &count_s))? };
        let wf = get(&rec, 6);
        let wait_for = WaitFor::parse(&wf).ok_or_else(|| bad("wait_for", &wf))?;
        let wa = get(&rec, 7);
        let wait_after_ms = if wa.is_empty() { 0 } else { wa.parse::<u64>().map_err(|_| bad("wait_after_ms", &wa))? };
        let of = get(&rec, 8);
        let on_failure = OnFailure::parse(&of).ok_or_else(|| bad("on_failure", &of))?;
        let params = params_from_kv(&get(&rec, 10)).map_err(|e| ApiError::BadRequest(format!("csv row {row}: {e}")))?;
        let mut step = Step { id: String::new(), label: get(&rec, 0), task_type, target, item_code, count, params, wait_for, wait_after_ms, on_failure, note: get(&rec, 9), robot: None };
        step.normalize();
        steps.push(step);
    }
    Ok(Scenario { name: name.to_string(), steps, ..Default::default() })
}

// ---------------------------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Issue {
    /// `None` = scenario-level.
    pub step_index: Option<u32>,
    pub field: String,
    pub message: String,
}

fn issue(step: usize, field: &str, message: impl Into<String>) -> Issue {
    Issue { step_index: Some(step as u32), field: field.into(), message: message.into() }
}

/// Registry-free checks (shape, required fields, params keys).
pub fn validate_shape(s: &Scenario) -> Vec<Issue> {
    let mut out = Vec::new();
    if s.name.trim().is_empty() {
        out.push(Issue { step_index: None, field: "name".into(), message: "이름이 비어 있음".into() });
    }
    if s.steps.is_empty() {
        out.push(Issue { step_index: None, field: "steps".into(), message: "스텝이 없음".into() });
    }
    let base = TaskParams::default();
    for (i, st) in s.steps.iter().enumerate() {
        let tt = st.task_type;
        if st.count == 0 {
            out.push(issue(i, "count", "수량은 1 이상"));
        }
        if st.count > 255 {
            out.push(issue(i, "count", "수량은 255 이하"));
        }
        if matches!(tt, TaskType::Pick | TaskType::Drop | TaskType::Measure) && st.item_code.is_none() {
            out.push(issue(i, "item_code", format!("{}에는 품목이 필요", tt.name())));
        }
        match &st.target {
            None => {
                if matches!(tt, TaskType::Pick | TaskType::Drop | TaskType::Move) {
                    out.push(issue(i, "target", format!("{}에는 대상이 필요", tt.name())));
                }
            }
            Some(t) => {
                if t.kind != "cell" && t.kind != "station" {
                    out.push(issue(i, "target", format!("알 수 없는 대상 종류 '{}'", t.kind)));
                }
                if t.id == 0 {
                    out.push(issue(i, "target", "대상 id가 0"));
                }
            }
        }
        if !st.params.is_object() {
            out.push(issue(i, "params", "params는 객체여야 함"));
        } else if let Err(e) = base.overlay(&st.params) {
            out.push(issue(i, "params", format!("params 오류: {e}")));
        }
    }
    out
}

/// Shape checks + registry lookups (unknown cell / station / item).
pub fn validate(st: &AppState, s: &Scenario) -> Result<Vec<Issue>, ApiError> {
    let mut out = validate_shape(s);
    for (i, step) in s.steps.iter().enumerate() {
        if let Some(t) = &step.target {
            let known = match t.kind.as_str() {
                "cell" => st.registry.cell(t.id)?.is_some(),
                "station" => st.registry.station(t.id)?.is_some(),
                _ => true, // already reported by the shape check
            };
            if !known {
                out.push(issue(i, "target", format!("{} {} 이(가) 레지스트리에 없음", if t.kind == "cell" { "셀" } else { "스테이션" }, t.id)));
            }
        }
        if let Some(code) = step.item_code
            && st.registry.item(code)?.is_none()
        {
            out.push(issue(i, "item_code", format!("품목 {code} 이(가) 레지스트리에 없음")));
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> Scenario {
        let mut s = Scenario {
            name: "삼단 테스트".into(),
            description: "PICK/DROP/MEASURE".into(),
            repeat: 2,
            steps: vec![
                Step {
                    label: "pick".into(),
                    task_type: TaskType::Pick,
                    target: Some(Target { kind: "cell".into(), id: 101 }),
                    item_code: Some(1001),
                    count: 2,
                    params: json!({ "grip_height": 55, "avoid": true }),
                    wait_for: WaitFor::Completed,
                    wait_after_ms: 500,
                    on_failure: OnFailure::Retry,
                    note: "a, \"quoted\" note".into(),
                    ..Default::default()
                },
                Step {
                    label: "drop".into(),
                    task_type: TaskType::Drop,
                    target: Some(Target { kind: "station".into(), id: 2101 }),
                    item_code: Some(1001),
                    on_failure: OnFailure::Skip,
                    ..Default::default()
                },
                Step {
                    label: "measure".into(),
                    task_type: TaskType::Measure,
                    target: Some(Target { kind: "cell".into(), id: 103 }),
                    item_code: Some(1002),
                    params: json!({ "measure_item": true }),
                    wait_for: WaitFor::Accepted,
                    ..Default::default()
                },
                Step { label: "up".into(), task_type: TaskType::Up, ..Default::default() },
            ],
            ..Default::default()
        };
        s.normalize();
        s
    }

    fn strip(mut s: Scenario) -> Scenario {
        s.id.clear();
        s.created_at.clear();
        s.updated_at.clear();
        for st in &mut s.steps {
            st.id.clear();
        }
        s
    }

    #[test]
    fn json_round_trip() {
        let s = sample();
        let doc = to_json(&s);
        assert_eq!(doc["schema"], json!(SCHEMA));
        let back = from_json(doc.to_string().as_bytes()).unwrap();
        assert!(back.id.is_empty());
        assert_eq!(strip(back.clone()).steps, strip(s.clone()).steps);
        assert_eq!(back.name, s.name);
        assert_eq!(back.repeat, 2);
        // step ids are kept when present
        assert_eq!(back.steps[0].id, s.steps[0].id);
    }

    #[test]
    fn json_tolerates_unknown_and_missing_fields() {
        let raw = br#"{"schema":1,"name":"x","extra":42,"steps":[{"type":"PICK","target":{"kind":"cell","id":5},"item_code":7,"future_field":true}]}"#;
        let s = from_json(raw).unwrap();
        assert_eq!(s.steps.len(), 1);
        assert_eq!(s.steps[0].count, 1);
        assert_eq!(s.steps[0].wait_for, WaitFor::Completed);
        assert!(!s.steps[0].id.is_empty());
        assert!(from_json(br#"{"schema":99,"name":"x"}"#).is_err());
        assert!(from_json(b"[1,2]").is_err());
    }

    #[test]
    fn csv_round_trip() {
        let s = sample();
        let text = to_csv(&s).unwrap();
        let first = text.lines().next().unwrap();
        assert_eq!(first, CSV_COLUMNS.join(","));
        assert_eq!(text.lines().count(), 5);
        let back = from_csv(&text, &s.name).unwrap();
        assert_eq!(strip(back).steps, strip(s).steps);
    }

    #[test]
    fn csv_params_kv() {
        assert_eq!(params_to_kv(&json!({ "b": true, "a": 1, "z": null, "s": "x" })), "a=1;b=true;s=x");
        assert_eq!(params_from_kv("a=1; b=true ;s=x;f=1.5").unwrap(), json!({ "a": 1, "b": true, "s": "x", "f": 1.5 }));
        assert_eq!(params_from_kv("").unwrap(), json!({}));
        assert!(params_from_kv("novalue").is_err());
    }

    #[test]
    fn csv_column_order_free_and_errors() {
        let text = "\u{feff}type,target_id,target_kind,item_code\nPICK,12,cell,1001\n\n";
        let s = from_csv(text, "n").unwrap();
        assert_eq!(s.steps.len(), 1);
        let t = s.steps[0].target.as_ref().unwrap();
        assert_eq!((t.kind.as_str(), t.id), ("cell", 12));
        assert_eq!(s.steps[0].item_code, Some(1001));
        assert!(from_csv("label,type\nx,FLY\n", "n").is_err());
        assert!(from_csv("type,target_kind,target_id\nPICK,cell,abc\n", "n").is_err());
        assert!(from_csv("label\nx\n", "n").is_err());
    }

    #[test]
    fn validation_shape() {
        let mut s = sample();
        assert!(validate_shape(&s).is_empty());
        s.name.clear();
        s.steps[0].count = 0;
        s.steps[0].item_code = None;
        s.steps[1].target = None;
        s.steps[2].params = json!({ "grip_height": "tall" });
        s.steps[3].target = Some(Target { kind: "moon".into(), id: 0 });
        let v = validate_shape(&s);
        let fields: Vec<(Option<u32>, &str)> = v.iter().map(|i| (i.step_index, i.field.as_str())).collect();
        assert!(fields.contains(&(None, "name")));
        assert!(fields.contains(&(Some(0), "count")));
        assert!(fields.contains(&(Some(0), "item_code")));
        assert!(fields.contains(&(Some(1), "target")));
        assert!(fields.contains(&(Some(2), "params")));
        assert_eq!(fields.iter().filter(|f| **f == (Some(3), "target")).count(), 2);
    }
}
