//! Resolved node map (path → node id / kind) and its JSON cache.

use std::collections::BTreeMap;
use std::path::Path;
use std::str::FromStr;

use opcua::types::NodeId;

use crate::OpcError;
use crate::path::normalize_path;
use crate::value::PlcKind;

/// Summary of the node map for diagnostics.
#[derive(Clone, Debug, serde::Serialize)]
pub struct NodeMapInfo {
    pub ns: u16,
    pub count: usize,
    /// First 20 `(path, nodeid)` pairs.
    pub sample: Vec<(String, String)>,
}

/// Cache file format: `{ns, endpoint, root_nodeid, members: {path: nodeid}}` plus `kinds`.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct NodeMap {
    pub ns: u16,
    pub endpoint: String,
    pub root_nodeid: String,
    /// Leaf member path (canonical, relative to root) → node id string.
    pub members: BTreeMap<String, String>,
    /// Leaf member path → scalar kind (missing → `Unknown`).
    #[serde(default)]
    pub kinds: BTreeMap<String, PlcKind>,
    /// How this map was produced: `browse`, `cache`, `synth:quoted-index`,
    /// `synth:index-in-quotes`.
    #[serde(default)]
    pub source: String,
}

impl NodeMap {
    pub fn info(&self) -> NodeMapInfo {
        NodeMapInfo { ns: self.ns, count: self.members.len(), sample: self.members.iter().take(20).map(|(p, n)| (p.clone(), n.clone())).collect() }
    }

    /// Node id and kind for a member path (path is normalized first).
    pub fn lookup(&self, path: &str) -> Result<(NodeId, PlcKind), OpcError> {
        let key = normalize_path(path)?;
        let id = self.members.get(&key).ok_or_else(|| OpcError::NodeMissing(key.clone()))?;
        let node = NodeId::from_str(id).map_err(|_| OpcError::Config(format!("{key}: invalid cached node id {id:?}")))?;
        let kind = self.kinds.get(&key).copied().unwrap_or(PlcKind::Unknown);
        Ok((node, kind))
    }

    /// All member paths with the given dotted prefix (e.g. `Command.`).
    pub fn paths_with_prefix(&self, prefix: &str) -> Vec<String> {
        self.members.keys().filter(|k| k.starts_with(prefix)).cloned().collect()
    }

    /// All direct array elements `Name[i]` of `name`, in index order.
    pub fn array_elements(&self, name: &str) -> Vec<String> {
        let mut v: Vec<(u32, String)> = self
            .members
            .keys()
            .filter_map(|k| {
                let rest = k.strip_prefix(name)?.strip_prefix('[')?;
                let idx: u32 = rest.strip_suffix(']')?.parse().ok()?;
                Some((idx, k.clone()))
            })
            .collect();
        v.sort();
        v.into_iter().map(|(_, k)| k).collect()
    }

    /// Rewrite 0-based array element keys to PLC indices (see [`rebase_array_keys`]).
    /// Returns the number of keys rewritten.
    pub fn rebase_arrays(&mut self, bases: &BTreeMap<String, i64>) -> usize {
        let (members, n) = rebase_array_keys(&self.members, bases);
        if n > 0 {
            let (kinds, _) = rebase_array_keys(&self.kinds, bases);
            self.members = members;
            self.kinds = kinds;
        }
        n
    }

    pub fn load(path: &Path) -> Option<NodeMap> {
        let text = std::fs::read_to_string(path).ok()?;
        match serde_json::from_str::<NodeMap>(&text) {
            Ok(mut m) => {
                m.source = "cache".to_string();
                Some(m)
            }
            Err(e) => {
                tracing::warn!(?path, error = %e, "node cache unreadable, ignoring");
                None
            }
        }
    }

    pub fn save(&self, path: &Path) {
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            let _ = std::fs::create_dir_all(parent);
        }
        match serde_json::to_string_pretty(self) {
            Ok(text) => {
                if let Err(e) = std::fs::write(path, text) {
                    tracing::warn!(?path, error = %e, "failed to write node cache");
                }
            }
            Err(e) => tracing::warn!(error = %e, "failed to serialize node cache"),
        }
    }
}

/// Path with every array index removed: `TaskData.Cell.Position[2]` → `TaskData.Cell.Position`.
fn strip_indices(segs: &[crate::path::Segment]) -> String {
    segs.iter().map(|s| s.name.as_str()).collect::<Vec<_>>().join(".")
}

/// Declared lower bound of every array in a set of leaf paths that carry PLC indices
/// (e.g. contract layout members relative to the command root): the smallest index seen
/// per array, keyed by the array path without indices (`TaskData.Position` → 1).
/// Only the first dimension of each array segment is considered.
pub fn array_bases_from_paths<'a>(paths: impl IntoIterator<Item = &'a str>) -> BTreeMap<String, i64> {
    let mut out: BTreeMap<String, i64> = BTreeMap::new();
    for p in paths {
        let Ok(segs) = crate::path::parse_path(p) else { continue };
        for d in 0..segs.len() {
            if let Some(&i) = segs[d].indices.first() {
                let key = strip_indices(&segs[..=d]);
                let e = out.entry(key).or_insert(i64::from(i));
                *e = (*e).min(i64::from(i));
            }
        }
    }
    out
}

/// Root path with PLC indices (`GR[2].CMD`) → the S7-1500 OPC UA server's element names, which
/// are 0-based from the declared lower bound for arrays of structs as well: `GR : Array[1..3]`
/// exposes PLC `GR[2]` as `"GR"[1]` (live GRM 2026-09-21: `"GR"[2]` was PLC `GR[3]`).
/// `bases`: array path without indices, relative to the DB (`GR` → 1), from
/// [`array_bases_from_paths`] over the DB's contract paths. Arrays not in `bases` are kept.
pub fn server_root_path(root: &str, bases: &BTreeMap<String, i64>) -> String {
    let Ok(segs) = crate::path::parse_path(root) else { return root.to_string() };
    let mut out = Vec::with_capacity(segs.len());
    for d in 0..segs.len() {
        let seg = &segs[d];
        let mut s = seg.name.clone();
        let lb = bases.get(&strip_indices(&segs[..=d])).copied().unwrap_or(0);
        for (k, i) in seg.indices.iter().enumerate() {
            // only the first dimension carries the collected lower bound
            let v = if k == 0 { i64::from(*i) - lb } else { i64::from(*i) };
            s.push_str(&format!("[{}]", v.max(0)));
        }
        out.push(s);
    }
    out.join(".")
}

/// Index rebasing for servers that expose arrays 0-based (offset from the declared lower bound).
///
/// The S7-1500 OPC UA server exposes arrays 0-based: `Position : Array[1..4] of Real` browses as
/// `Position[0]..[3]`, and struct arrays too (see [`server_root_path`]; the root is translated
/// before browsing, so member keys under it never carry a struct index). The writer looks members
/// up by PLC index, so such keys are rewritten
/// `P[k]` → `P[k + lb]` using `bases` (array path without indices → declared lower bound).
///
/// Decided per array instance from the index set that is present: an array is rebased only
/// when `lb > 0` and index `0` is present (a PLC-indexed array with `lb > 0` never has
/// element 0). Struct arrays that already carry PLC indices, arrays with `lb == 0`, arrays
/// not in `bases`, and an already rebased map are left unchanged (idempotent).
/// Returns the rewritten map and the number of keys changed.
pub fn rebase_array_keys<V: Clone>(map: &BTreeMap<String, V>, bases: &BTreeMap<String, i64>) -> (BTreeMap<String, V>, usize) {
    use std::collections::BTreeSet;
    let bases: BTreeMap<String, i64> = bases.iter().filter(|(_, lb)| **lb > 0).filter_map(|(k, lb)| crate::path::parse_path(k).ok().map(|s| (strip_indices(&s), *lb))).collect();
    if bases.is_empty() {
        return (map.clone(), 0);
    }
    let mut entries: Vec<(Option<Vec<crate::path::Segment>>, String, V)> = map.iter().map(|(k, v)| (crate::path::parse_path(k).ok(), k.clone(), v.clone())).collect();
    let depth = entries.iter().filter_map(|(s, _, _)| s.as_ref().map(Vec::len)).max().unwrap_or(0);
    let mut changed = 0usize;
    for d in 0..depth {
        // Array instance (rendered prefix + name at depth d) → (lower bound, indices present).
        let mut groups: BTreeMap<String, (i64, BTreeSet<u32>)> = BTreeMap::new();
        let instance = |segs: &[crate::path::Segment]| -> Option<(String, i64, u32)> {
            let seg = segs.get(d)?;
            let idx = *seg.indices.first()?;
            let lb = *bases.get(&strip_indices(&segs[..=d]))?;
            let mut inst = crate::path::render(&segs[..d]);
            if !inst.is_empty() {
                inst.push('.');
            }
            inst.push_str(&seg.name);
            Some((inst, lb, idx))
        };
        for (segs, _, _) in &entries {
            if let Some((inst, lb, idx)) = segs.as_deref().and_then(instance) {
                groups.entry(inst).or_insert_with(|| (lb, BTreeSet::new())).1.insert(idx);
            }
        }
        groups.retain(|_, (_, set)| set.contains(&0));
        if groups.is_empty() {
            continue;
        }
        for (segs, key, _) in &mut entries {
            let Some(s) = segs.as_mut() else { continue };
            let Some((inst, lb, idx)) = instance(s) else { continue };
            if !groups.contains_key(&inst) {
                continue;
            }
            let Ok(new_idx) = u32::try_from(i64::from(idx) + lb) else { continue };
            s[d].indices[0] = new_idx;
            *key = crate::path::render(s);
            changed += 1;
        }
    }
    (entries.into_iter().map(|(_, k, v)| (k, v)).collect(), changed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(m: &BTreeMap<String, String>) -> Vec<&str> {
        m.keys().map(String::as_str).collect()
    }

    fn bases() -> BTreeMap<String, i64> {
        [("TaskData.Position", 1), ("TaskData.Cell.Position", 1), ("Data", 0), ("GR", 1)].iter().map(|(k, v)| (k.to_string(), *v)).collect()
    }

    #[test]
    fn server_root_is_zero_based_for_struct_arrays() {
        let b: BTreeMap<String, i64> = [("GR".to_string(), 1)].into_iter().collect();
        assert_eq!(server_root_path("GR[2].CMD", &b), "GR[1].CMD");
        assert_eq!(server_root_path("GR[1].CMD", &b), "GR[0].CMD");
        // array not in the contract → unchanged; plain path unchanged
        assert_eq!(server_root_path("X[3].CMD", &b), "X[3].CMD");
        assert_eq!(server_root_path("GRM.CMD", &b), "GRM.CMD");
        // bases built from DB-relative contract paths
        let from = array_bases_from_paths(["GR[1].CMD.Header.CMD", "GR[3].STAT.Mode"]);
        assert_eq!(server_root_path("GR[2].CMD", &from), "GR[1].CMD");
    }

    #[test]
    fn rebase_zero_based_elementary_arrays() {
        let mut m = BTreeMap::new();
        for k in [
            "TaskData.Position[0]",
            "TaskData.Position[1]",
            "TaskData.Position[2]",
            "TaskData.Position[3]",
            "TaskData.Cell.Position[0]",
            "TaskData.Cell.Position[1]",
            "TaskData.Cell.Position[2]",
            "Data[0]",
            "Data[15]",
            "GR[2].X",
            "GR[3].X",
            "Header.CMD",
        ] {
            m.insert(k.to_string(), format!("id:{k}"));
        }
        let (r, n) = rebase_array_keys(&m, &bases());
        assert_eq!(n, 7);
        assert_eq!(
            keys(&r),
            vec![
                "Data[0]",
                "Data[15]",
                "GR[2].X",
                "GR[3].X",
                "Header.CMD",
                "TaskData.Cell.Position[1]",
                "TaskData.Cell.Position[2]",
                "TaskData.Cell.Position[3]",
                "TaskData.Position[1]",
                "TaskData.Position[2]",
                "TaskData.Position[3]",
                "TaskData.Position[4]",
            ]
        );
        // Node ids stay with their element: PLC index 1 is the server's [0].
        assert_eq!(r["TaskData.Position[1]"], "id:TaskData.Position[0]");
        assert_eq!(r["TaskData.Position[4]"], "id:TaskData.Position[3]");
        assert_eq!(r["TaskData.Cell.Position[3]"], "id:TaskData.Cell.Position[2]");
        assert_eq!(r["Data[15]"], "id:Data[15]");
        assert_eq!(r["GR[2].X"], "id:GR[2].X");
        // Idempotent.
        let (again, n2) = rebase_array_keys(&r, &bases());
        assert_eq!(n2, 0);
        assert_eq!(again, r);
        // No bases → unchanged.
        let (same, n3) = rebase_array_keys(&m, &BTreeMap::new());
        assert_eq!((same, n3), (m, 0));
    }

    #[test]
    fn rebase_nested_under_zero_based_struct_array() {
        // A struct array that the server exposed 0-based would be rebased too, with inner arrays after it.
        let mut m = BTreeMap::new();
        for k in ["Q[0].P[0]", "Q[0].P[1]", "Q[1].P[0]", "Q[1].P[1]"] {
            m.insert(k.to_string(), k.to_string());
        }
        let b: BTreeMap<String, i64> = [("Q".to_string(), 1), ("Q.P".to_string(), 1)].into_iter().collect();
        let (r, _) = rebase_array_keys(&m, &b);
        assert_eq!(keys(&r), vec!["Q[1].P[1]", "Q[1].P[2]", "Q[2].P[1]", "Q[2].P[2]"]);
        assert_eq!(r["Q[2].P[2]"], "Q[1].P[1]");
    }

    #[test]
    fn bases_from_layout_paths() {
        let b = array_bases_from_paths([
            "TaskData.Position[1]",
            "TaskData.Position[4]",
            "TaskData.Cell.Position[3]",
            "TaskData.Cell.Position[1]",
            "Data[0]",
            "Data[15]",
            "Header.CMD",
            "Q[2].P[5]",
            "Q[3].P[6]",
        ]);
        let want: BTreeMap<String, i64> = [("Data", 0), ("Q", 2), ("Q.P", 5), ("TaskData.Cell.Position", 1), ("TaskData.Position", 1)].iter().map(|(k, v)| (k.to_string(), *v)).collect();
        assert_eq!(b, want);
    }

    fn map() -> NodeMap {
        let mut m = NodeMap::default();
        for (p, k) in [
            ("Header.Protocol", PlcKind::U8),
            ("Command.Stop", PlcKind::U8),
            ("Command.Task.Complete.WorkId", PlcKind::U32),
            ("Data[0]", PlcKind::U8),
            ("Data[10]", PlcKind::U8),
            ("Data[2]", PlcKind::U8),
            ("DataX[1]", PlcKind::U8),
        ] {
            m.members.insert(p.to_string(), format!("ns=3;s=\"OPCUA\".{p}"));
            m.kinds.insert(p.to_string(), k);
        }
        m
    }

    #[test]
    fn lookup_and_queries() {
        let m = map();
        let (id, kind) = m.lookup(" Header . Protocol ").unwrap();
        assert_eq!(kind, PlcKind::U8);
        assert_eq!(id.namespace, 3);
        assert!(matches!(m.lookup("Nope").unwrap_err(), OpcError::NodeMissing(p) if p == "Nope"));
        assert_eq!(m.paths_with_prefix("Command.").len(), 2);
        assert_eq!(m.array_elements("Data"), vec!["Data[0]", "Data[2]", "Data[10]"]);
        assert_eq!(m.info().count, 7);
    }

    #[test]
    fn cache_roundtrip() {
        let dir = std::env::temp_dir().join(format!("opcua-cmd-test-{}", std::process::id()));
        let file = dir.join("nodes.json");
        let m = map();
        m.save(&file);
        let loaded = NodeMap::load(&file).unwrap();
        assert_eq!(loaded.members, m.members);
        assert_eq!(loaded.kinds, m.kinds);
        assert_eq!(loaded.source, "cache");
        let _ = std::fs::remove_dir_all(dir);
    }
}
