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

#[cfg(test)]
mod tests {
    use super::*;

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
