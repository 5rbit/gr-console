//! 구조체 멤버 통째 쓰기 — S7-1500 OPC UA 서버는 UDT 멤버(`CMD.TaskData`)를 ExtensionObject 하나로 읽고 쓴다
//! (인코딩 ID `TE_"<UDT>"`). 리프 51 개를 한 Write 에 담는 것보다 노드 1 개가 서버 쪽에서 훨씬 싸다
//! (2026-09-21 실기 GRM 읽기: 리프 등록 ~77 ms, 구조체 ~11–16 ms).
//!
//! 본문은 OPC UA 바이너리 그대로: 선언 순서, 리틀 엔디언, Bool 1 바이트(비트 묶음 아님), 배열 = Int32 길이 + 원소,
//! 중첩 UDT 는 그 자리에 펼침. 이 모듈은 타입 정보를 모른다 — 호출자가 계약(UDT 선언)에서 [`StructSpec`] 을 만든다.
//!
//! 안전장치: 세션마다 **읽기로** 검증한다 — 구조체와 그 리프들을 한 Read 로 읽어, 리프 값을 이 인코더로 만든 바이트가
//! 서버가 준 본문과 한 바이트도 다르지 않을 때만 그 세션에서 구조체 쓰기를 켠다. 다르면 리프 쓰기 그대로.

use std::collections::HashMap;

use opcua::types::type_loader::ByteStringBody;
use opcua::types::{ByteString, ExtensionObject, NodeId, Variant};

use crate::value::PlcKind;
use crate::{OpcError, PlcValue};

/// 구조체 본문의 한 자리 — 선언 순서대로 늘어놓는다.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum StructSlot {
    /// 스칼라 리프. `path` 는 명령 루트 기준 정규 경로(PLC 첨자): `TaskData.Position[1]`.
    Leaf { path: String, kind: PlcKind },
    /// 배열 앞의 Int32 원소 수.
    ArrayLen(i32),
}

/// 루트 아래 구조체 멤버 하나(`TaskData`)의 바이너리 배치.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct StructSpec {
    pub member: String,
    pub slots: Vec<StructSlot>,
}

impl StructSpec {
    pub fn leaves(&self) -> impl Iterator<Item = (&str, PlcKind)> {
        self.slots.iter().filter_map(|s| match s {
            StructSlot::Leaf { path, kind } => Some((path.as_str(), *kind)),
            StructSlot::ArrayLen(_) => None,
        })
    }

    /// 인덱스 없는 직속 리프 하나(`TaskData.WorkId` → `WorkId`) — 구조체 노드 ID 를 그 리프 ID 에서 얻는다.
    fn direct_leaf(&self) -> Option<(&str, &str)> {
        let prefix = format!("{}.", self.member);
        self.leaves().find_map(|(p, _)| {
            let name = p.strip_prefix(&prefix)?;
            (!name.contains(['.', '['])).then_some((p, name))
        })
    }
}

fn num_f64(v: &PlcValue) -> f64 {
    match *v {
        PlcValue::Bool(b) => b as u8 as f64,
        PlcValue::U8(x) => x as f64,
        PlcValue::I8(x) => x as f64,
        PlcValue::U16(x) => x as f64,
        PlcValue::I16(x) => x as f64,
        PlcValue::U32(x) => x as f64,
        PlcValue::I32(x) => x as f64,
        PlcValue::F32(x) => x as f64,
        PlcValue::F64(x) => x,
        PlcValue::Str(_) => f64::NAN,
    }
}

/// 리프 값(없으면 0) → 본문 바이트. 문자열·알 수 없는 종류는 지원하지 않는다(→ 리프 쓰기).
pub fn encode(spec: &StructSpec, values: &HashMap<String, PlcValue>) -> Result<Vec<u8>, OpcError> {
    let mut out = Vec::with_capacity(spec.slots.len() * 3);
    for slot in &spec.slots {
        let (path, kind) = match slot {
            StructSlot::ArrayLen(n) => {
                out.extend_from_slice(&n.to_le_bytes());
                continue;
            }
            StructSlot::Leaf { path, kind } => (path, *kind),
        };
        let v = values.get(path);
        let n = v.map(num_f64).unwrap_or(0.0);
        if n.is_nan() && !matches!(kind, PlcKind::F32 | PlcKind::F64) {
            return Err(OpcError::Config(format!("{path}: value not numeric for struct write")));
        }
        // 정수 종류는 같은 폭의 값만 받는다(범위 밖이면 거절 — 리프 쓰기의 coerce 와 같은 뜻).
        let int = |lo: f64, hi: f64| -> Result<f64, OpcError> { if n.fract() != 0.0 || n < lo || n > hi { Err(OpcError::Config(format!("{path}: {n} out of range for {kind:?}"))) } else { Ok(n) } };
        match kind {
            PlcKind::Bool => out.push(u8::from(n != 0.0)),
            PlcKind::U8 => out.push(int(0.0, u8::MAX as f64)? as u8),
            PlcKind::I8 => out.extend_from_slice(&(int(i8::MIN as f64, i8::MAX as f64)? as i8).to_le_bytes()),
            PlcKind::U16 => out.extend_from_slice(&(int(0.0, u16::MAX as f64)? as u16).to_le_bytes()),
            PlcKind::I16 => out.extend_from_slice(&(int(i16::MIN as f64, i16::MAX as f64)? as i16).to_le_bytes()),
            PlcKind::U32 => out.extend_from_slice(&(int(0.0, u32::MAX as f64)? as u32).to_le_bytes()),
            PlcKind::I32 => out.extend_from_slice(&(int(i32::MIN as f64, i32::MAX as f64)? as i32).to_le_bytes()),
            // PlcValue 에 64 비트 정수가 없다 — 32 비트 범위만.
            PlcKind::U64 => out.extend_from_slice(&(int(0.0, u32::MAX as f64)? as u64).to_le_bytes()),
            PlcKind::I64 => out.extend_from_slice(&(int(i32::MIN as f64, i32::MAX as f64)? as i64).to_le_bytes()),
            // Real 은 F32 원래 값을 그대로(f64 왕복은 같은 값이지만 NaN 비트 모양까지 지킨다).
            PlcKind::F32 => out.extend_from_slice(
                &match v {
                    Some(PlcValue::F32(x)) => *x,
                    _ => n as f32,
                }
                .to_le_bytes(),
            ),
            PlcKind::F64 => out.extend_from_slice(&n.to_le_bytes()),
            PlcKind::Str | PlcKind::Unknown => return Err(OpcError::Config(format!("{path}: {kind:?} not supported in struct write"))),
        }
    }
    Ok(out)
}

/// 구조체 노드 ID — 직속 리프의 문자열 ID 에서 `."<리프>"` 를 떼어 낸다(S7-1500: `ns=3;s="OPCUA"."GR"[1]."CMD"."TaskData"."WorkId"`).
pub fn struct_node(spec: &StructSpec, leaf_id: impl Fn(&str) -> Option<NodeId>) -> Option<NodeId> {
    let (path, name) = spec.direct_leaf()?;
    let id = leaf_id(path)?;
    let opcua::types::Identifier::String(s) = &id.identifier else { return None };
    let s: &str = s.as_ref();
    let base = s.strip_suffix(&format!(".\"{name}\""))?;
    Some(NodeId::new(id.namespace, base.to_string()))
}

/// 읽은 구조체 값 → (인코딩 ID, 본문). 서버가 알 수 없는 타입으로 준 ExtensionObject 만(FallbackTypeLoader).
pub fn raw_body(v: &Variant) -> Option<(NodeId, Vec<u8>)> {
    let Variant::ExtensionObject(eo) = v else { return None };
    let b = eo.inner_as::<ByteStringBody>()?;
    Some((b.encoding_id().clone(), b.raw_body().value.clone().unwrap_or_default()))
}

/// 본문 → 쓸 값.
pub fn to_variant(encoding_id: &NodeId, body: Vec<u8>) -> Variant {
    Variant::ExtensionObject(ExtensionObject::new(ByteStringBody::new(ByteString::from(body), encoding_id.clone())))
}

/// 세션 검증 결과 — 구조체 쓰기를 켤지. 불일치면 처음 다른 바이트 위치를 사유에 적는다.
pub fn verify(spec: &StructSpec, leaf_values: &HashMap<String, PlcValue>, raw: &[u8]) -> Result<(), String> {
    let enc = encode(spec, leaf_values).map_err(|e| e.to_string())?;
    if enc.len() != raw.len() {
        return Err(format!("{}: encoded {} B, server body {} B", spec.member, enc.len(), raw.len()));
    }
    match enc.iter().zip(raw).position(|(a, b)| a != b) {
        None => Ok(()),
        Some(i) => Err(format!("{}: byte {i} differs (encoded {:#04x}, server {:#04x})", spec.member, enc[i], raw[i])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `LGR_Task_Data` (2026-09-21 GRM 계약) — 선언 순서.
    pub(crate) fn task_spec() -> StructSpec {
        use PlcKind::*;
        let mut s = Vec::new();
        let leaf = |s: &mut Vec<StructSlot>, p: &str, k: PlcKind| s.push(StructSlot::Leaf { path: format!("TaskData.{p}"), kind: k });
        leaf(&mut s, "WorkId", U32);
        leaf(&mut s, "TaskId", U32);
        leaf(&mut s, "TaskType", U8);
        s.push(StructSlot::ArrayLen(4));
        for i in 1..=4 {
            leaf(&mut s, &format!("Position[{i}]"), F32);
        }
        leaf(&mut s, "Item.Code", U32);
        leaf(&mut s, "Item.Count", U8);
        for n in ["InnerDiameter", "OuterDiameter", "LowerBidHeight", "UpperBidHeight", "Height", "DeflectionFactor"] {
            leaf(&mut s, &format!("Item.{n}"), F32);
        }
        leaf(&mut s, "Cell.Use", Bool);
        leaf(&mut s, "Cell.BlendUse", Bool);
        for n in ["Id", "Section", "Row", "Col"] {
            leaf(&mut s, &format!("Cell.{n}"), U16);
        }
        leaf(&mut s, "Cell.Lenth", F32);
        leaf(&mut s, "Cell.Width", F32);
        s.push(StructSlot::ArrayLen(3));
        for i in 1..=3 {
            leaf(&mut s, &format!("Cell.Position[{i}]"), F32);
        }
        for (n, k) in [
            ("BlendUpDistance", U16),
            ("BlendDownDistance", U16),
            ("DragOutHeight", U16),
            ("DragOutDist", U16),
            ("DragOutDir", U8),
            ("DragInHeight", U16),
            ("DragInDist", U16),
            ("DragInDir", U8),
            ("LiftUpCreepDistance", U16),
            ("LiftDownCreepDistance", U16),
            ("LiftUpHeight", U16),
            ("PreGripDelta", U16),
            ("GripBackDelta", U16),
            ("GripHeight", U16),
        ] {
            leaf(&mut s, n, k);
        }
        for n in ["UseDragOut", "UseDragIn", "LiftUpAfterComplete", "LiftUpPartial", "MeasureFloor", "MeasureItem", "MeasureSku", "AdjustCenter", "FindStationItem", "Avoid", "Outbound"] {
            leaf(&mut s, n, Bool);
        }
        StructSpec { member: "TaskData".into(), slots: s }
    }

    /// 실기 GRM `"OPCUA"."GR"[1]."CMD"."TaskData"` 덤프(2026-09-21, 모든 리프 0) — 129 B, 0 아닌 바이트는 두 배열 길이뿐.
    #[test]
    fn zero_task_matches_device_dump() {
        let mut dump = vec![0u8; 129];
        dump[9] = 4;
        dump[76] = 3;
        let spec = task_spec();
        assert_eq!(spec.leaves().count(), 51);
        assert_eq!(encode(&spec, &HashMap::new()).unwrap(), dump);
        assert!(verify(&spec, &HashMap::new(), &dump).is_ok());
    }

    #[test]
    fn values_land_at_declared_offsets() {
        let spec = task_spec();
        let mut v = HashMap::new();
        v.insert("TaskData.WorkId".to_string(), PlcValue::U32(0x0102_0304));
        v.insert("TaskData.TaskType".to_string(), PlcValue::U8(5));
        v.insert("TaskData.Position[4]".to_string(), PlcValue::F32(372.0));
        v.insert("TaskData.Cell.BlendUse".to_string(), PlcValue::Bool(true));
        v.insert("TaskData.GripHeight".to_string(), PlcValue::U16(0xBEEF));
        v.insert("TaskData.Outbound".to_string(), PlcValue::Bool(true));
        let b = encode(&spec, &v).unwrap();
        assert_eq!(b.len(), 129);
        assert_eq!(&b[0..4], &[4, 3, 2, 1]);
        assert_eq!(b[8], 5);
        assert_eq!(&b[25..29], &372.0f32.to_le_bytes());
        assert_eq!(b[59], 1);
        assert_eq!(&b[116..118], &0xBEEFu16.to_le_bytes());
        assert_eq!(b[128], 1);
        let e = verify(&spec, &HashMap::new(), &b).unwrap_err();
        assert!(e.contains("byte 0 differs"), "{e}");
    }

    #[test]
    fn out_of_range_is_refused() {
        let spec = task_spec();
        let mut v = HashMap::new();
        v.insert("TaskData.TaskType".to_string(), PlcValue::U16(300));
        assert!(encode(&spec, &v).is_err());
    }

    #[test]
    fn struct_node_strips_leaf() {
        let spec = task_spec();
        let n = struct_node(&spec, |p| {
            assert_eq!(p, "TaskData.WorkId");
            Some(NodeId::new(3, "\"OPCUA\".\"GR\"[1].\"CMD\".\"TaskData\".\"WorkId\"".to_string()))
        })
        .unwrap();
        assert_eq!(n, NodeId::new(3, "\"OPCUA\".\"GR\"[1].\"CMD\".\"TaskData\"".to_string()));
    }

    #[test]
    fn body_round_trips_through_variant() {
        let enc = NodeId::new(3, "TE_\"LGR_Task_Data\"".to_string());
        let v = to_variant(&enc, vec![1, 2, 3]);
        assert_eq!(raw_body(&v), Some((enc, vec![1, 2, 3])));
    }
}
