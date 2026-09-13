//! Contract + message registry loading. Link UDTs missing from the PLC contract (before `gr-contract sync`)
//! are filled in from embedded copies so the server and its tests work either way.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow};
use plc_layout::Contract;
use plc_link::{Codec, RegistryDoc};

/// Embedded link UDTs, added only when the contract lacks them.
pub const FALLBACK_UDTS: &[(&str, &str)] =
    &[("LNK_Hello", include_str!("../fallback/LNK_Hello.udt")), ("LNK_Ack", include_str!("../fallback/LNK_Ack.udt")), ("LNK_GR_Status", include_str!("../fallback/LNK_GR_Status.udt"))];

#[derive(Clone, Debug)]
pub struct Loaded {
    pub codec: Codec,
    /// Contract name (directory name, e.g. `GR2_PLC`).
    pub name: String,
    pub dir: PathBuf,
    pub messages: PathBuf,
    /// Embedded UDTs that were added because the contract lacks them.
    pub fallback_udts: Vec<String>,
}

/// Repository root: the current directory when it has `plc/link/messages.toml`, else the source tree.
pub fn repo_root() -> PathBuf {
    let cwd = std::env::current_dir().unwrap_or_default();
    if cwd.join("plc/link/messages.toml").is_file() {
        return cwd;
    }
    let src = PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../.."));
    if src.join("plc/link/messages.toml").is_file() { src } else { cwd }
}

/// `plc/contract/<name>` below the repository root, or `name` itself when it is an existing directory.
pub fn contract_dir(name_or_dir: &str) -> PathBuf {
    let p = PathBuf::from(name_or_dir);
    if p.is_dir() && p.join("types").is_dir() {
        return p;
    }
    repo_root().join("plc/contract").join(name_or_dir)
}

pub fn default_messages() -> PathBuf {
    repo_root().join("plc/link/messages.toml")
}

/// Loads `dir` (+ `extra` directories merged into the same contract), adds missing fallback link UDTs and
/// resolves the registry (unavailable messages allowed).
pub fn load(dir: &Path, extra: &[PathBuf], messages: &Path, fallback: bool) -> anyhow::Result<Loaded> {
    let mut c = Contract::load_dir(dir).map_err(|e| anyhow!("contract {}: {e}", dir.display()))?;
    if c.udts.is_empty() {
        return Err(anyhow!("contract {} has no UDTs (wrong directory?)", dir.display()));
    }
    // extra directories only fill gaps; the PLC contract wins
    for x in extra {
        let e = Contract::load_dir(x).map_err(|e| anyhow!("contract {}: {e}", x.display()))?;
        for (k, v) in e.udts {
            c.udts.entry(k).or_insert(v);
        }
        for (k, v) in e.consts {
            c.consts.entry(k).or_insert(v);
        }
        for (k, v) in e.dbs {
            c.dbs.entry(k).or_insert(v);
        }
    }
    let mut fallback_udts = Vec::new();
    if fallback {
        for (name, src) in FALLBACK_UDTS {
            if !c.udts.contains_key(*name) {
                c.add_udt_source(src).map_err(|e| anyhow!("fallback UDT {name}: {e}"))?;
                fallback_udts.push(name.to_string());
            }
        }
    }
    let doc = RegistryDoc::load(messages).map_err(|e| anyhow!("{}: {e}", messages.display()))?;
    let registry = doc.resolve(&c).map_err(|e| anyhow!("registry: {e}")).context("resolving plc/link/messages.toml against the contract")?;
    let name = dir.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(Loaded { codec: Codec::new(Arc::new(c), Arc::new(registry)), name, dir: dir.to_path_buf(), messages: messages.to_path_buf(), fallback_udts })
}

/// GR2_PLC from the repository with fallbacks (tests, self test).
pub fn load_default(name: &str) -> anyhow::Result<Loaded> {
    load(&contract_dir(name), &[], &default_messages(), true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gr2_contract_resolves_every_message() {
        let l = load_default("GR2_PLC").unwrap();
        let unavailable: Vec<_> = l.codec.registry().messages.iter().filter(|m| !m.available).map(|m| m.name.clone()).collect();
        assert!(unavailable.is_empty(), "{unavailable:?}");
        for (name, _) in FALLBACK_UDTS {
            assert!(l.codec.contract().udts.contains_key(*name));
        }
    }

    /// Once the LNK UDTs are synced, the PC hash equals the generator's `registry.json` (LNK_REGISTRY_HASH).
    #[test]
    fn gr2_hash_matches_generated_registry() {
        let l = load_default("GR2_PLC").unwrap();
        let path = repo_root().join("plc/generated/link/GR2_PLC/registry.json");
        let Ok(text) = std::fs::read_to_string(&path) else { return };
        if !l.fallback_udts.is_empty() {
            return;
        }
        let v: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(v["hash_text"].as_str(), Some(l.codec.registry().hash_text().as_str()));
        assert_eq!(v["registry_hash"].as_u64(), Some(l.codec.registry().hash() as u64));
    }
}
