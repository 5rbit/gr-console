//! Node-map resolution: cache verification, address-space browsing, string-node-id fallback.

use std::collections::BTreeMap;
use std::str::FromStr;

use opcua::client::Session;
use opcua::types::{AttributeId, BrowseDescription, BrowseDirection, BrowseResultMaskFlags, NodeClass, NodeClassMask, NodeId, ObjectId, ReadValueId, ReferenceTypeId, TimestampsToReturn, Variant};

use crate::connect::{map_err, with_timeout};
use crate::nodemap::NodeMap;
use crate::path::{Segment, element_candidates, join_child, parse_path, strip_quotes};
use crate::value::PlcKind;
use crate::{OpcError, OpcUaConfig};

/// Member used to verify a cached / synthesized map.
pub const PROBE_MEMBER: &str = "Header.Protocol";

const DB_SEARCH_DEPTH: usize = 4;
const SUBTREE_MAX_DEPTH: usize = 16;
const SUBTREE_MAX_NODES: usize = 50_000;
const BROWSE_CHUNK: usize = 50;
const READ_CHUNK: usize = 200;

/// Known leaf members of `LGR_Interface_GR_Command` (GR2 version: the Command bytes are Bool bit structs),
/// used only when browsing fails and node ids have to be synthesized. Kinds are hints; the `DataType` attribute wins.
pub const KNOWN_MEMBERS: &[(&str, PlcKind)] = &[
    ("NTP.YEAR", PlcKind::U16),
    ("NTP.MONTH", PlcKind::U8),
    ("NTP.DAY", PlcKind::U8),
    ("NTP.WEEKDAY", PlcKind::U8),
    ("NTP.HOUR", PlcKind::U8),
    ("NTP.MINUTE", PlcKind::U8),
    ("NTP.SECOND", PlcKind::U8),
    ("NTP.NANOSECOND", PlcKind::U32),
    ("Header.Protocol", PlcKind::U8),
    ("Header.CMD_ID", PlcKind::U8),
    ("Header.CMD", PlcKind::U8),
    ("Header.SRC", PlcKind::U16),
    ("Header.DST", PlcKind::U16),
    ("Header.SEQ", PlcKind::U16),
    ("Command.Stop.Normal", PlcKind::Bool),
    ("Command.Stop.Fast", PlcKind::Bool),
    ("Command.Stop.Cycle", PlcKind::Bool),
    ("Command.Stop.Spare_X3", PlcKind::Bool),
    ("Command.Stop.Spare_X4", PlcKind::Bool),
    ("Command.Stop.Spare_X5", PlcKind::Bool),
    ("Command.Stop.Spare_X6", PlcKind::Bool),
    ("Command.Stop.Spare_X7", PlcKind::Bool),
    ("Command.Common.Reset", PlcKind::Bool),
    ("Command.Common.Initializing", PlcKind::Bool),
    ("Command.Common.Start", PlcKind::Bool),
    ("Command.Common.Spare_X3", PlcKind::Bool),
    ("Command.Common.Spare_X4", PlcKind::Bool),
    ("Command.Common.Spare_X5", PlcKind::Bool),
    ("Command.Common.Spare_X6", PlcKind::Bool),
    ("Command.Common.BuzzerStop", PlcKind::Bool),
    ("Command.Task.Complete.WorkId", PlcKind::U32),
    ("Command.Task.Complete.TaskId", PlcKind::U32),
    ("Command.Task.Delete.WorkId", PlcKind::U32),
    ("Command.Task.Delete.TaskId", PlcKind::U32),
    ("Command.Jog.xPositive", PlcKind::Bool),
    ("Command.Jog.xNegative", PlcKind::Bool),
    ("Command.Jog.yPositive", PlcKind::Bool),
    ("Command.Jog.yNegative", PlcKind::Bool),
    ("Command.Jog.zPositive", PlcKind::Bool),
    ("Command.Jog.zNegative", PlcKind::Bool),
    ("Command.Jog.gPositive", PlcKind::Bool),
    ("Command.Jog.gNegative", PlcKind::Bool),
    ("Command.B3_Spare.Spare_X0", PlcKind::Bool),
    ("Command.B3_Spare.Spare_X1", PlcKind::Bool),
    ("Command.B3_Spare.Spare_X2", PlcKind::Bool),
    ("Command.B3_Spare.Spare_X3", PlcKind::Bool),
    ("Command.B3_Spare.Spare_X4", PlcKind::Bool),
    ("Command.B3_Spare.Spare_X5", PlcKind::Bool),
    ("Command.B3_Spare.Spare_X6", PlcKind::Bool),
    ("Command.B3_Spare.Spare_X7", PlcKind::Bool),
    ("Command.Home.X", PlcKind::Bool),
    ("Command.Home.Y", PlcKind::Bool),
    ("Command.Home.Z", PlcKind::Bool),
    ("Command.Home.G", PlcKind::Bool),
    ("Command.Home.Spare_X4", PlcKind::Bool),
    ("Command.Home.Spare_X5", PlcKind::Bool),
    ("Command.Home.Spare_X6", PlcKind::Bool),
    ("Command.Home.Spare_X7", PlcKind::Bool),
    ("Command.MaintPos.X", PlcKind::Bool),
    ("Command.MaintPos.Y", PlcKind::Bool),
    ("Command.MaintPos.Z", PlcKind::Bool),
    ("Command.MaintPos.G", PlcKind::Bool),
    ("Command.MaintPos.Spare_X4", PlcKind::Bool),
    ("Command.MaintPos.Spare_X5", PlcKind::Bool),
    ("Command.MaintPos.Spare_X6", PlcKind::Bool),
    ("Command.MaintPos.Spare_X7", PlcKind::Bool),
    ("Command.CellOrigin.X", PlcKind::Bool),
    ("Command.CellOrigin.Y", PlcKind::Bool),
    ("Command.CellOrigin.Spare_X2", PlcKind::Bool),
    ("Command.CellOrigin.Spare_X3", PlcKind::Bool),
    ("Command.CellOrigin.Spare_X4", PlcKind::Bool),
    ("Command.CellOrigin.Spare_X5", PlcKind::Bool),
    ("Command.CellOrigin.Spare_X6", PlcKind::Bool),
    ("Command.CellOrigin.Spare_X7", PlcKind::Bool),
    ("Command.ChangeMode", PlcKind::U8),
    ("Data[0]", PlcKind::U8),
    ("Data[1]", PlcKind::U8),
    ("Data[2]", PlcKind::U8),
    ("Data[3]", PlcKind::U8),
    ("Data[4]", PlcKind::U8),
    ("Data[5]", PlcKind::U8),
    ("Data[6]", PlcKind::U8),
    ("Data[7]", PlcKind::U8),
    ("Data[8]", PlcKind::U8),
    ("Data[9]", PlcKind::U8),
    ("Data[10]", PlcKind::U8),
    ("Data[11]", PlcKind::U8),
    ("Data[12]", PlcKind::U8),
    ("Data[13]", PlcKind::U8),
    ("Data[14]", PlcKind::U8),
    ("Data[15]", PlcKind::U8),
    ("TaskData.WorkId", PlcKind::U32),
    ("TaskData.TaskId", PlcKind::U32),
    ("TaskData.TaskType", PlcKind::U8),
    // Elementary-type arrays are exposed 0-based by the S7-1500 server (`Array[1..4] of Real` →
    // `Position[0]..[3]`); `OpcUaConfig::array_bases` rebases them to PLC indices after resolution.
    ("TaskData.Position[0]", PlcKind::F32),
    ("TaskData.Position[1]", PlcKind::F32),
    ("TaskData.Position[2]", PlcKind::F32),
    ("TaskData.Position[3]", PlcKind::F32),
    ("TaskData.Item.Code", PlcKind::U32),
    ("TaskData.Item.Count", PlcKind::U8),
    ("TaskData.Item.InnerDiameter", PlcKind::F32),
    ("TaskData.Item.OuterDiameter", PlcKind::F32),
    ("TaskData.Item.LowerBidHeight", PlcKind::F32),
    ("TaskData.Item.UpperBidHeight", PlcKind::F32),
    ("TaskData.Item.Height", PlcKind::F32),
    ("TaskData.Item.DeflectionFactor", PlcKind::F32),
    ("TaskData.Cell.Use", PlcKind::Bool),
    ("TaskData.Cell.BlendUse", PlcKind::Bool),
    ("TaskData.Cell.Id", PlcKind::U16),
    ("TaskData.Cell.Section", PlcKind::U16),
    ("TaskData.Cell.Row", PlcKind::U16),
    ("TaskData.Cell.Col", PlcKind::U16),
    ("TaskData.Cell.Lenth", PlcKind::F32),
    ("TaskData.Cell.Width", PlcKind::F32),
    ("TaskData.Cell.Position[0]", PlcKind::F32),
    ("TaskData.Cell.Position[1]", PlcKind::F32),
    ("TaskData.Cell.Position[2]", PlcKind::F32),
    ("TaskData.BlendUpDistance", PlcKind::Unknown),
    ("TaskData.GripHeight", PlcKind::Unknown),
    ("TaskData.UseDragOut", PlcKind::Bool),
    ("TaskData.Outbound", PlcKind::Bool),
];

#[derive(Clone, Debug)]
struct Child {
    node_id: NodeId,
    browse_name: String,
    class: NodeClass,
}

/// Browse the forward hierarchical Object/Variable children of each parent (batched,
/// continuation points followed). Result index i corresponds to `parents[i]`.
async fn browse_children(session: &Session, cfg: &OpcUaConfig, parents: &[NodeId]) -> Result<Vec<Vec<Child>>, OpcError> {
    let timeout = cfg.connect_timeout();
    let mut out: Vec<Vec<Child>> = Vec::with_capacity(parents.len());
    for chunk in parents.chunks(BROWSE_CHUNK) {
        let descs: Vec<BrowseDescription> = chunk
            .iter()
            .map(|id| BrowseDescription {
                node_id: id.clone(),
                browse_direction: BrowseDirection::Forward,
                reference_type_id: ReferenceTypeId::HierarchicalReferences.into(),
                include_subtypes: true,
                node_class_mask: (NodeClassMask::OBJECT | NodeClassMask::VARIABLE).bits(),
                result_mask: (BrowseResultMaskFlags::BrowseName | BrowseResultMaskFlags::NodeClass | BrowseResultMaskFlags::DisplayName).bits(),
            })
            .collect();
        let results = with_timeout(timeout, session.browse(&descs, 0, None)).await?.map_err(map_err)?;
        for (i, parent) in chunk.iter().enumerate() {
            let mut children = Vec::new();
            let Some(result) = results.get(i) else {
                out.push(children);
                continue;
            };
            if result.status_code.is_bad() {
                tracing::debug!(node = %parent, status = %result.status_code, "browse failed");
                out.push(children);
                continue;
            }
            let mut refs = result.references.clone().unwrap_or_default();
            let mut cp = result.continuation_point.clone();
            while !cp.is_null() {
                let more = with_timeout(timeout, session.browse_next(false, &[cp.clone()])).await?.map_err(map_err)?;
                let Some(next) = more.into_iter().next() else {
                    break;
                };
                refs.extend(next.references.unwrap_or_default());
                cp = next.continuation_point;
            }
            for r in refs {
                if r.node_id.server_index != 0 {
                    continue;
                }
                children.push(Child { node_id: r.node_id.node_id.clone(), browse_name: r.browse_name.name.as_ref().to_string(), class: r.node_class });
            }
            out.push(children);
        }
    }
    Ok(out)
}

/// BFS from ObjectsFolder (depth ≤ 4) for a node whose BrowseName equals `db_name`.
async fn find_db(session: &Session, cfg: &OpcUaConfig) -> Result<Option<NodeId>, OpcError> {
    let wanted = strip_quotes(&cfg.db_name);
    let mut frontier: Vec<NodeId> = vec![ObjectId::ObjectsFolder.into()];
    for depth in 0..DB_SEARCH_DEPTH {
        if frontier.is_empty() {
            break;
        }
        let levels = browse_children(session, cfg, &frontier).await?;
        let mut next = Vec::new();
        for children in levels {
            for c in children {
                if strip_quotes(&c.browse_name) == wanted {
                    tracing::info!(node = %c.node_id, depth, "found data block");
                    return Ok(Some(c.node_id));
                }
                // Skip the standard namespace (Server object etc.).
                if c.node_id.namespace != 0 {
                    next.push(c.node_id);
                }
            }
        }
        frontier = next;
    }
    Ok(None)
}

fn child_named<'a>(children: &'a [Child], names: &[String]) -> Option<&'a Child> {
    children.iter().find(|c| names.iter().any(|n| strip_quotes(&c.browse_name) == *n))
}

/// Walk `root_path` segments from the DB node.
async fn walk_root(session: &Session, cfg: &OpcUaConfig, db: NodeId, segments: &[Segment]) -> Result<NodeId, OpcError> {
    let mut node = db;
    for seg in segments {
        let children = browse_children(session, cfg, std::slice::from_ref(&node)).await?.pop().unwrap_or_default();
        let found = child_named(&children, std::slice::from_ref(&seg.name)).ok_or_else(|| {
            OpcError::Config(format!("root_path segment {:?} not found under {}; children: [{}]", seg.name, node, children.iter().map(|c| c.browse_name.clone()).collect::<Vec<_>>().join(", ")))
        })?;
        node = found.node_id.clone();
        let mut rendered = seg.name.clone();
        for idx in &seg.indices {
            let children = browse_children(session, cfg, std::slice::from_ref(&node)).await?.pop().unwrap_or_default();
            let candidates = element_candidates(&rendered, *idx);
            let found = child_named(&children, &candidates).ok_or_else(|| {
                OpcError::Config(format!(
                    "array element [{idx}] of {rendered:?} not found under {node} (tried {}); \
                     is \"Export array members\" enabled on the PLC? children: [{}]",
                    candidates.join("/"),
                    children.iter().map(|c| c.browse_name.clone()).collect::<Vec<_>>().join(", ")
                ))
            })?;
            node = found.node_id.clone();
            rendered = format!("{rendered}[{idx}]");
        }
    }
    Ok(node)
}

/// Read the `DataType` attribute of each node → kind (Unknown on any failure).
async fn read_kinds(session: &Session, cfg: &OpcUaConfig, nodes: &[(String, NodeId)]) -> BTreeMap<String, PlcKind> {
    let mut kinds = BTreeMap::new();
    for chunk in nodes.chunks(READ_CHUNK) {
        let ids: Vec<ReadValueId> = chunk.iter().map(|(_, id)| ReadValueId::new(id.clone(), AttributeId::DataType)).collect();
        let res = with_timeout(cfg.connect_timeout(), session.read(&ids, TimestampsToReturn::Neither, 0.0)).await;
        let values = match res {
            Ok(Ok(v)) => v,
            Ok(Err(e)) => {
                tracing::warn!(error = %e, "DataType read failed; kinds unknown");
                continue;
            }
            Err(_) => {
                tracing::warn!("DataType read timed out; kinds unknown");
                continue;
            }
        };
        for ((path, _), dv) in chunk.iter().zip(values) {
            let kind = match dv.value {
                Some(Variant::NodeId(id)) => PlcKind::from_data_type(&id),
                _ => PlcKind::Unknown,
            };
            kinds.insert(path.clone(), kind);
        }
    }
    kinds
}

/// Browse the whole subtree under `root`, returning every leaf Variable with its path.
async fn collect_leaves(session: &Session, cfg: &OpcUaConfig, root: &NodeId) -> Result<Vec<(String, NodeId)>, OpcError> {
    let mut leaves = Vec::new();
    let mut frontier: Vec<(String, NodeId)> = vec![(String::new(), root.clone())];
    let mut seen = 0usize;
    for _depth in 0..SUBTREE_MAX_DEPTH {
        if frontier.is_empty() {
            break;
        }
        let ids: Vec<NodeId> = frontier.iter().map(|(_, id)| id.clone()).collect();
        let levels = browse_children(session, cfg, &ids).await?;
        let mut next = Vec::new();
        for ((path, _), children) in frontier.iter().zip(levels) {
            for c in children {
                seen += 1;
                if seen > SUBTREE_MAX_NODES {
                    return Err(OpcError::Config(format!("subtree under {root} exceeds {SUBTREE_MAX_NODES} nodes; refine root_path")));
                }
                let child_path = join_child(path, &c.browse_name);
                next.push((child_path, c.node_id, c.class));
            }
        }
        // A node is a leaf if browsing it yields no children. Browse the whole level once.
        let next_ids: Vec<NodeId> = next.iter().map(|(_, id, _)| id.clone()).collect();
        let grand = browse_children(session, cfg, &next_ids).await?;
        frontier.clear();
        for ((path, id, class), children) in next.into_iter().zip(grand) {
            if children.is_empty() {
                if class == NodeClass::Variable {
                    leaves.push((path, id));
                }
            } else {
                frontier.push((path, id));
            }
        }
        // `frontier` children are browsed again next iteration; that is one redundant
        // browse per level, accepted for simplicity.
    }
    Ok(leaves)
}

async fn browse_map(session: &Session, cfg: &OpcUaConfig, endpoint_url: &str) -> Result<NodeMap, OpcError> {
    let segments = parse_path(&cfg.root_path)?;
    let db = find_db(session, cfg).await?.ok_or_else(|| OpcError::Config(format!("data block {:?} not found within {DB_SEARCH_DEPTH} levels of Objects", cfg.db_name)))?;
    let root = walk_root(session, cfg, db, &segments).await?;
    let leaves = collect_leaves(session, cfg, &root).await?;
    if leaves.is_empty() {
        return Err(OpcError::Config(format!("no variable leaves found under {root}")));
    }
    let kinds = read_kinds(session, cfg, &leaves).await;
    let members = leaves.into_iter().map(|(p, id)| (p, id.to_string())).collect();
    Ok(NodeMap { ns: root.namespace, endpoint: endpoint_url.to_string(), root_nodeid: root.to_string(), members, kinds, source: "browse".to_string() })
}

/// S7 string id for the root: `"OPCUA"."GR"[2]."CMD"` or `"OPCUA"."GR[2]"."CMD"`.
pub fn synth_root(db: &str, segments: &[Segment], index_in_quotes: bool) -> String {
    let mut s = format!("\"{}\"", strip_quotes(db));
    for seg in segments {
        s.push_str(&synth_segment(seg, index_in_quotes));
    }
    s
}

fn synth_segment(seg: &Segment, index_in_quotes: bool) -> String {
    let idx: String = seg.indices.iter().map(|i| format!("[{i}]")).collect();
    if index_in_quotes { format!(".\"{}{idx}\"", seg.name) } else { format!(".\"{}\"{idx}", seg.name) }
}

/// `root` + member path in the same S7 string form.
pub fn synth_member(root: &str, member: &str, index_in_quotes: bool) -> Result<String, OpcError> {
    let mut s = root.to_string();
    for seg in parse_path(member)? {
        s.push_str(&synth_segment(&seg, index_in_quotes));
    }
    Ok(s)
}

async fn read_value_ok(session: &Session, cfg: &OpcUaConfig, id: &NodeId) -> bool {
    let res = with_timeout(cfg.write_timeout(), session.read(&[ReadValueId::new_value(id.clone())], TimestampsToReturn::Neither, 0.0)).await;
    match res {
        Ok(Ok(v)) => v.first().is_some_and(|dv| dv.status.is_none_or(|s| s.is_good()) && dv.value.is_some()),
        _ => false,
    }
}

/// Verify a map by reading `Header.Protocol`.
pub async fn verify(session: &Session, cfg: &OpcUaConfig, map: &NodeMap) -> bool {
    match map.lookup(PROBE_MEMBER) {
        Ok((id, _)) => read_value_ok(session, cfg, &id).await,
        Err(_) => false,
    }
}

async fn synth_map(session: &Session, cfg: &OpcUaConfig, endpoint_url: &str) -> Result<NodeMap, OpcError> {
    let segments = parse_path(&cfg.root_path)?;
    let mut tried = Vec::new();
    for index_in_quotes in [false, true] {
        let root = synth_root(&cfg.db_name, &segments, index_in_quotes);
        let probe = synth_member(&root, PROBE_MEMBER, index_in_quotes)?;
        let probe_id = NodeId::new(cfg.ns_hint, probe.as_str());
        tried.push(probe_id.to_string());
        if !read_value_ok(session, cfg, &probe_id).await {
            continue;
        }
        tracing::info!(root = %root, ns = cfg.ns_hint, "browse failed; using synthesized string node ids");
        let candidates: Vec<(String, NodeId, PlcKind)> = KNOWN_MEMBERS
            .iter()
            .filter_map(|(p, k)| {
                let s = synth_member(&root, p, index_in_quotes).ok()?;
                Some((p.to_string(), NodeId::new(cfg.ns_hint, s.as_str()), *k))
            })
            .collect();
        // Keep only members the server actually has (Value readable).
        let mut members = BTreeMap::new();
        let mut hint_kinds = BTreeMap::new();
        for chunk in candidates.chunks(READ_CHUNK) {
            let ids: Vec<ReadValueId> = chunk.iter().map(|(_, id, _)| ReadValueId::new_value(id.clone())).collect();
            let values = with_timeout(cfg.connect_timeout(), session.read(&ids, TimestampsToReturn::Neither, 0.0)).await?.map_err(map_err)?;
            for ((path, id, kind), dv) in chunk.iter().zip(values) {
                if dv.status.is_none_or(|s| s.is_good()) && dv.value.is_some() {
                    members.insert(path.clone(), id.to_string());
                    hint_kinds.insert(path.clone(), *kind);
                }
            }
        }
        let nodes: Vec<(String, NodeId)> = members.iter().filter_map(|(p, s)| NodeId::from_str(s).ok().map(|id| (p.clone(), id))).collect();
        let mut kinds = read_kinds(session, cfg, &nodes).await;
        for (p, k) in hint_kinds {
            let e = kinds.entry(p).or_insert(PlcKind::Unknown);
            if *e == PlcKind::Unknown {
                *e = k;
            }
        }
        return Ok(NodeMap {
            ns: cfg.ns_hint,
            endpoint: endpoint_url.to_string(),
            root_nodeid: NodeId::new(cfg.ns_hint, root.as_str()).to_string(),
            members,
            kinds,
            source: if index_in_quotes { "synth:index-in-quotes".to_string() } else { "synth:quoted-index".to_string() },
        });
    }
    Err(OpcError::Config(format!("fallback probe failed for [{}]", tried.join(", "))))
}

/// A cached map was resolved for this `root_path`: its S7 string root id ends with the synthesized
/// root (either index style). Numeric / non-S7 ids cannot be compared and are accepted.
fn cache_root_matches(root_nodeid: &str, cfg: &OpcUaConfig) -> bool {
    let Some(sid) = root_nodeid.split_once(";s=").map(|(_, s)| s) else { return true };
    let Ok(segments) = parse_path(&cfg.root_path) else { return true };
    [false, true].into_iter().any(|q| sid == synth_root(&cfg.db_name, &segments, q))
}

/// Rewrite 0-based elementary-array keys to PLC indices (`cfg.array_bases`).
fn rebase(map: &mut NodeMap, cfg: &OpcUaConfig) {
    let n = map.rebase_arrays(&cfg.array_bases);
    if n > 0 {
        tracing::info!(keys = n, source = %map.source, "0-based array elements rebased to PLC indices");
    }
}

/// Resolve the node map: cache (verified) → browse → synthesized string ids.
/// The result is written to `node_cache` when it did not come from the cache.
pub async fn resolve(session: &Session, cfg: &OpcUaConfig, endpoint_url: &str, use_cache: bool) -> Result<NodeMap, OpcError> {
    if use_cache
        && let Some(path) = &cfg.node_cache
        && let Some(mut map) = NodeMap::load(path)
    {
        rebase(&mut map, cfg);
        if !cache_root_matches(&map.root_nodeid, cfg) {
            tracing::warn!(?path, cached = %map.root_nodeid, root = %cfg.root_path, "node cache is for another root; re-browsing");
        } else if verify(session, cfg, &map).await {
            tracing::info!(?path, count = map.members.len(), "node cache verified");
            return Ok(map);
        }
        tracing::warn!(?path, "node cache failed verification; re-browsing");
    }

    let mut map = match browse_map(session, cfg, endpoint_url).await {
        Ok(map) => map,
        Err(browse_err) => {
            tracing::warn!(error = %browse_err, "browse failed; trying string node id fallback");
            match synth_map(session, cfg, endpoint_url).await {
                Ok(map) => map,
                Err(synth_err) => {
                    return Err(OpcError::Config(format!("node map resolution failed: browse: {browse_err}; fallback: {synth_err}")));
                }
            }
        }
    };
    rebase(&mut map, cfg);
    if !verify(session, cfg, &map).await {
        tracing::warn!("resolved map has no readable {PROBE_MEMBER}; continuing anyway");
    }
    if let Some(path) = &cfg.node_cache {
        map.save(path);
    }
    Ok(map)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_root_must_match_root_path() {
        let cfg = OpcUaConfig { root_path: "GR[1].CMD".into(), ..OpcUaConfig::default() };
        assert!(cache_root_matches("ns=3;s=\"OPCUA\".\"GR\"[1].\"CMD\"", &cfg));
        assert!(cache_root_matches("ns=3;s=\"OPCUA\".\"GR[1]\".\"CMD\"", &cfg));
        // the 2026-09-21 cache (PLC index used as server index) is refused
        assert!(!cache_root_matches("ns=3;s=\"OPCUA\".\"GR\"[2].\"CMD\"", &cfg));
        assert!(cache_root_matches("ns=3;i=1234", &cfg));
    }

    #[test]
    fn synth_forms() {
        let segs = parse_path("GR[2].CMD").unwrap();
        assert_eq!(synth_root("OPCUA", &segs, false), "\"OPCUA\".\"GR\"[2].\"CMD\"");
        assert_eq!(synth_root("OPCUA", &segs, true), "\"OPCUA\".\"GR[2]\".\"CMD\"");
        let root = synth_root("OPCUA", &segs, false);
        assert_eq!(synth_member(&root, "TaskData.Position[3]", false).unwrap(), "\"OPCUA\".\"GR\"[2].\"CMD\".\"TaskData\".\"Position\"[3]");
        assert_eq!(synth_member("\"OPCUA\"", "Header.CMD_ID", true).unwrap(), "\"OPCUA\".\"Header\".\"CMD_ID\"");
        let id = NodeId::new(3, synth_member(&root, "Header.CMD", false).unwrap().as_str());
        assert_eq!(id.to_string(), "ns=3;s=\"OPCUA\".\"GR\"[2].\"CMD\".\"Header\".\"CMD\"");
    }
}
