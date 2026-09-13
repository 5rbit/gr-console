//! `gr-contract gen-link`: writes the PLC link sources and PC artifacts produced by `plc_link::codegen`.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use plc_layout::Contract;
use plc_link::RegistryDoc;
use plc_link::codegen::{GenOpts, Target, generate, has_generated_marker, normalize_for_compare};

pub struct Args {
    pub plc: String,
    pub contract: PathBuf,
    pub messages: PathBuf,
    pub export: PathBuf,
    pub out_scl: Option<PathBuf>,
    pub out_tags: Option<PathBuf>,
    pub out_pc: Option<PathBuf>,
    pub check: bool,
    pub prune: bool,
    pub strict: bool,
    pub verify_sync: bool,
}

/// Runs the command. `Ok(false)` = `--check` found differences or `--verify-sync` found stale contract UDTs.
pub fn run(a: &Args) -> anyhow::Result<bool> {
    let contract_root = a.contract.join(&a.plc);
    let c = Contract::load_dir(&contract_root).with_context(|| format!("loading {}", contract_root.display()))?;
    let doc = RegistryDoc::load(&a.messages)?;
    let reg = if a.strict { doc.resolve_strict(&c)? } else { doc.resolve(&c)? };

    let mut ok = true;
    if a.verify_sync {
        let stale = stale_udts(&contract_root.join("types"), &a.export.join(&a.plc).join("types"))?;
        if stale.is_empty() {
            println!("verify-sync: contract UDTs match {}", a.export.join(&a.plc).display());
        } else {
            for s in &stale {
                eprintln!("stale contract: {s}");
            }
            eprintln!("run `gr-contract sync --from {} --plc {} --dbs ... --messages {}` first", a.export.display(), a.plc, a.messages.display());
            if !a.check {
                bail!("{} contract UDT(s) differ from the export", stale.len());
            }
            ok = false;
        }
    }

    let out = generate(&c, &reg, &GenOpts { plc: a.plc.clone(), strict: a.strict })?;
    for w in &out.warnings {
        eprintln!("warning: {w}");
    }

    let scl_dir = a.out_scl.clone().unwrap_or_else(|| a.export.join(&a.plc).join("blocks").join("700. Communication").join("Socket").join("Gen"));
    let tags_dir = a.out_tags.clone().unwrap_or_else(|| a.export.join(&a.plc).join("tags").join("Const"));
    let pc_dir = a.out_pc.clone().unwrap_or_else(|| Path::new("plc/generated/link").join(&a.plc));
    let dir_of = |t: Target| match t {
        Target::Scl => &scl_dir,
        Target::Tags => &tags_dir,
        Target::Pc => &pc_dir,
    };

    let mut produced = BTreeSet::new();
    let mut pending: Vec<(PathBuf, &[u8], &str)> = Vec::new();
    let mut same = 0usize;
    for f in &out.files {
        let dest = f.path.split('/').fold(dir_of(f.target).clone(), |p, seg| p.join(seg));
        produced.insert(dest.clone());
        match std::fs::read(&dest) {
            Ok(old) if normalize_for_compare(&old) == normalize_for_compare(&f.bytes) => same += 1,
            Ok(old) => {
                if f.target == Target::Tags {
                    warn_removed_constants(&dest, &old, &out.constants.iter().map(|k| (k.name.clone(), k.data_type.to_string())).collect());
                }
                pending.push((dest, &f.bytes, "changed"));
            }
            Err(_) => pending.push((dest, &f.bytes, "new")),
        }
    }
    let stale = if a.prune { stale_generated(&scl_dir, &pc_dir, &produced)? } else { Vec::new() };

    if a.check {
        for (p, _, why) in &pending {
            println!("{why}: {}", p.display());
        }
        for p in &stale {
            println!("stale: {}", p.display());
        }
        let clean = pending.is_empty() && stale.is_empty();
        println!("gen-link --check: {same} up to date, {} to write, {} to remove -> {}", pending.len(), stale.len(), if clean { "OK" } else { "OUT OF DATE" });
        return Ok(ok && clean);
    }

    for (p, bytes, why) in &pending {
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).with_context(|| format!("creating {}", parent.display()))?;
        }
        std::fs::write(p, bytes).with_context(|| format!("writing {}", p.display()))?;
        println!("{why}: {}", p.display());
    }
    for p in &stale {
        std::fs::remove_file(p).with_context(|| format!("removing {}", p.display()))?;
        println!("removed: {}", p.display());
    }
    println!("{}: registry hash 16#{:08X}, {} blocks, {same} files unchanged, {} written, {} removed", a.plc, reg.hash(), out.blocks.len(), pending.len(), stale.len());
    for (i, b) in out.blocks.iter().enumerate() {
        println!("  {:>3}  {:<11}  {}", i + 1, b.kind.name(), b.name);
    }
    Ok(ok)
}

/// Warns about constants of the existing table that the generated table no longer has (or declares with another type).
fn warn_removed_constants(path: &Path, old: &[u8], new: &BTreeMap<String, String>) {
    let text = String::from_utf8_lossy(old);
    let Ok(list) = plc_layout::consts::parse_const_xml(&text) else { return };
    for (name, _, ty) in list {
        match new.get(&name) {
            None => eprintln!("warning: {} loses constant {name} ({ty})", path.display()),
            Some(t) if !t.eq_ignore_ascii_case(&ty) => eprintln!("warning: {}: constant {name} changes type {ty} -> {t}", path.display()),
            Some(_) => {}
        }
    }
}

/// Generated files that were not produced this run: marker-carrying `.scl` in the SCL directory, schema / vector
/// files in the PC directory.
fn stale_generated(scl_dir: &Path, pc_dir: &Path, produced: &BTreeSet<PathBuf>) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for p in files_with_ext(scl_dir, "scl")? {
        if produced.contains(&p) {
            continue;
        }
        let text = std::fs::read(&p).with_context(|| format!("reading {}", p.display()))?;
        if has_generated_marker(&String::from_utf8_lossy(&text)) {
            out.push(p);
        }
    }
    for sub in ["schema", "vectors"] {
        for p in files_with_ext(&pc_dir.join(sub), "json")? {
            if !produced.contains(&p) {
                out.push(p);
            }
        }
    }
    out.sort();
    Ok(out)
}

fn files_with_ext(dir: &Path, ext: &str) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    if !dir.is_dir() {
        return Ok(out);
    }
    for e in std::fs::read_dir(dir).with_context(|| format!("reading {}", dir.display()))?.flatten() {
        let p = e.path();
        if p.is_file() && p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case(ext)) {
            out.push(p);
        }
    }
    out.sort();
    Ok(out)
}

/// Contract UDT sources that differ (normalized) from the export's, or are missing there.
fn stale_udts(contract_types: &Path, export_types: &Path) -> anyhow::Result<Vec<String>> {
    if !export_types.is_dir() {
        bail!("--verify-sync: {} is not a directory", export_types.display());
    }
    let mut export: BTreeMap<String, PathBuf> = BTreeMap::new();
    let mut stack = vec![export_types.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("udt"))
                && let Some(name) = p.file_name().and_then(|n| n.to_str())
            {
                export.insert(name.to_string(), p);
            }
        }
    }
    let mut stale = Vec::new();
    for p in files_with_ext(contract_types, "udt")? {
        let name = p.file_name().and_then(|n| n.to_str()).unwrap_or_default().to_string();
        match export.get(&name) {
            Some(e) => {
                if normalize_for_compare(&std::fs::read(&p)?) != normalize_for_compare(&std::fs::read(e)?) {
                    stale.push(format!("{name} differs from {}", e.display()));
                }
            }
            None => stale.push(format!("{name} is not in {}", export_types.display())),
        }
    }
    Ok(stale)
}

#[cfg(test)]
mod tests {
    use super::*;
    use plc_link::codegen::GENERATED_MARKER;

    #[test]
    fn marker_is_what_prune_looks_for() {
        assert!(has_generated_marker(&format!("\t{GENERATED_MARKER} from \"X\" sig 16#00000000 - do not edit")));
        assert!(!has_generated_marker("// hand written"));
    }
}
