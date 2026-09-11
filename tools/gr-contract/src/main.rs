//! gr-contract: PLC contract maintenance.
//!
//! * `sync`    copy the needed DB/UDT/constant sources from a TIA export into `plc/contract/<PLC>/`
//! * `dump`    print the standard-access member table of a DB (compare with the TIA offset column)
//! * `sizes`   print DB / UDT sizes
//! * `gen-sig` compute the LayoutSig of a DB and rewrite the `LayoutSig` start value in a .db source

use std::collections::{BTreeSet, HashSet};
use std::path::{Path, PathBuf};

use anyhow::{Context, bail};
use clap::{Parser, Subcommand};
use plc_layout::ast::{Field, TypeRef};
use plc_layout::{Contract, LayoutError};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Copy DBs (+ UDT closure + constant tables) from a TIA export dir into the contract dir.
    Sync {
        /// TIA export root (contains <PLC>/{blocks,types,tags})
        #[arg(long)]
        from: PathBuf,
        /// Contract root (default plc/contract)
        #[arg(long, default_value = "plc/contract")]
        out: PathBuf,
        #[arg(long)]
        plc: String,
        /// Comma-separated DB names
        #[arg(long, value_delimiter = ',')]
        dbs: Vec<String>,
        /// Extra UDTs to include even if not referenced
        #[arg(long, value_delimiter = ',', default_value = "")]
        udts: Vec<String>,
    },
    /// Print the member table of a DB.
    Dump {
        #[arg(long, default_value = "plc/contract")]
        contract: PathBuf,
        #[arg(long)]
        plc: String,
        #[arg(long)]
        db: String,
        /// Only members whose path starts with this prefix
        #[arg(long)]
        prefix: Option<String>,
        #[arg(long)]
        json: bool,
    },
    /// Print sizes of all DBs and UDTs in a contract.
    Sizes {
        #[arg(long, default_value = "plc/contract")]
        contract: PathBuf,
        #[arg(long)]
        plc: String,
    },
    /// Compute the layout signature of a DB and (optionally) rewrite `LayoutSig := 16#....` in a .db source.
    GenSig {
        #[arg(long, default_value = "plc/contract")]
        contract: PathBuf,
        #[arg(long)]
        plc: String,
        #[arg(long)]
        db: String,
        /// .db source file to patch (the `LayoutSig` member start value)
        #[arg(long)]
        patch: Option<PathBuf>,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Sync { from, out, plc, dbs, udts } => sync(&from, &out, &plc, &dbs, &udts),
        Cmd::Dump { contract, plc, db, prefix, json } => {
            let c = Contract::load_dir(&contract.join(&plc))?;
            let l = c.layout_db(&db)?;
            if json {
                println!("{}", serde_json::to_string_pretty(&l)?);
                return Ok(());
            }
            println!("DB \"{db}\" size {} bytes, number {:?}, sig 16#{:08X}", l.size, c.db_number(&db), c.layout_sig(&db)?);
            println!("{:>8}  {:<3}  {:<12} path", "offset", "bit", "type");
            for m in l.members.iter().filter(|m| prefix.as_ref().is_none_or(|p| m.path.starts_with(p))) {
                let bit = m.bit.map(|b| b.to_string()).unwrap_or_default();
                println!("{:>8}  {:<3}  {:<12} {}", m.offset, bit, m.prim.name(), m.path);
            }
            Ok(())
        }
        Cmd::Sizes { contract, plc } => {
            let c = Contract::load_dir(&contract.join(&plc))?;
            let mut dbs: Vec<_> = c.dbs.keys().cloned().collect();
            dbs.sort();
            for d in dbs {
                match c.layout_db(&d) {
                    Ok(l) => println!("DB  {:<24} {:>7} B  number {:?}  sig 16#{:08X}", d, l.size, c.db_number(&d), c.layout_sig(&d)?),
                    Err(e) => println!("DB  {:<24} ERROR {e}", d),
                }
            }
            let mut udts: Vec<_> = c.udts.keys().cloned().collect();
            udts.sort();
            for u in udts {
                match c.size_of_udt(&u) {
                    Ok(s) => println!("UDT {:<24} {:>7} B", u, s),
                    Err(e) => println!("UDT {:<24} ERROR {e}", u),
                }
            }
            Ok(())
        }
        Cmd::GenSig { contract, plc, db, patch } => {
            let c = Contract::load_dir(&contract.join(&plc))?;
            let sig = c.layout_sig(&db)?;
            println!("{db}: LayoutSig = 16#{sig:08X} ({sig})");
            if let Some(p) = patch {
                let text = std::fs::read_to_string(&p)?;
                let patched = patch_sig(&text, sig)?;
                std::fs::write(&p, patched)?;
                println!("patched {}", p.display());
            }
            Ok(())
        }
    }
}

/// Rewrites the `LayoutSig : DWord [:= ...];` declaration line to carry the start value.
fn patch_sig(text: &str, sig: u32) -> anyhow::Result<String> {
    let mut out = String::with_capacity(text.len() + 32);
    let mut found = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        if trimmed.starts_with("LayoutSig") && trimmed.contains(':') {
            let indent = &line[..line.len() - trimmed.len()];
            let (code, comment) = match trimmed.find("//") {
                Some(i) => (&trimmed[..i], Some(trimmed[i..].trim_end())),
                None => (trimmed.trim_end(), None),
            };
            let attrs = code.find('{').and_then(|s| code.find('}').map(|e| &code[s..=e])).unwrap_or("");
            let attrs_sp = if attrs.is_empty() { "" } else { " " };
            let mut l = format!("{indent}LayoutSig {attrs}{attrs_sp}: DWord := 16#{sig:08X};");
            if let Some(cm) = comment {
                l.push_str("   ");
                l.push_str(cm);
            } else {
                l.push_str("   // 레이아웃 서명 (gr-contract gen-sig 가 생성, 수동 수정 금지)");
            }
            let eol = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
            out.push_str(&l);
            out.push_str(eol);
            found = true;
        } else {
            out.push_str(line);
        }
    }
    if !found {
        bail!("no `LayoutSig` member found in source");
    }
    Ok(out)
}

fn sync(from: &Path, out: &Path, plc: &str, dbs: &[String], extra_udts: &[String]) -> anyhow::Result<()> {
    let src_root = from.join(plc);
    let full = Contract::load_dir(&src_root).with_context(|| format!("loading {}", src_root.display()))?;
    let dst = out.join(plc);
    std::fs::create_dir_all(dst.join("blocks"))?;
    std::fs::create_dir_all(dst.join("types"))?;
    std::fs::create_dir_all(dst.join("tags/Const"))?;

    // UDT closure
    let mut needed: BTreeSet<String> = extra_udts.iter().filter(|s| !s.is_empty()).cloned().collect();
    let mut stack: Vec<TypeRef> = Vec::new();
    for db in dbs {
        let d = full.db(db).with_context(|| format!("DB {db} not in export"))?;
        for f in &d.fields {
            stack.push(f.ty.clone());
        }
    }
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(t) = stack.pop() {
        match t {
            TypeRef::Udt(n) => {
                if seen.insert(n.clone()) {
                    needed.insert(n.clone());
                    let u = full.udt(&n)?;
                    for f in &u.fields {
                        stack.push(f.ty.clone());
                    }
                }
            }
            TypeRef::Struct(fields) => push_fields(&fields, &mut stack),
            TypeRef::Array { elem, .. } => stack.push(*elem),
            TypeRef::Prim(_) => {}
        }
    }
    for n in needed.iter().filter(|n| !seen.contains(*n)) {
        let u = full.udt(n)?;
        for f in &u.fields {
            stack.push(f.ty.clone());
        }
    }
    // copy files
    let copied_udts = copy_matching(&src_root.join("types"), &dst.join("types"), "udt", |stem| needed.contains(stem))?;
    let copied_dbs = copy_matching(&src_root.join("blocks"), &dst.join("blocks"), "db", |stem| dbs.iter().any(|d| d == stem))?;
    let copied_xml = copy_matching(&src_root.join("blocks"), &dst.join("blocks"), "xml", |stem| dbs.iter().any(|d| d == stem))?;
    let copied_consts = copy_matching(&src_root.join("tags"), &dst.join("tags/Const"), "xml", |_| true)?;
    println!("{plc}: {copied_dbs} DBs, {copied_xml} DB xml, {copied_udts} UDTs (of {} needed), {copied_consts} constant tables -> {}", needed.len(), dst.display());
    let missing: Vec<_> = needed.iter().filter(|n| !full.udts.contains_key(*n)).collect();
    if !missing.is_empty() {
        bail!("UDTs referenced but not found in export: {missing:?}");
    }
    // verify the copied contract resolves
    let c = Contract::load_dir(&dst)?;
    for db in dbs {
        let l = c.layout_db(db).map_err(|e: LayoutError| anyhow::anyhow!("{db}: {e}"))?;
        println!("  {db}: {} bytes, DB{:?}, sig 16#{:08X}", l.size, c.db_number(db), c.layout_sig(db)?);
    }
    Ok(())
}

fn push_fields(fields: &[Field], stack: &mut Vec<TypeRef>) {
    for f in fields {
        stack.push(f.ty.clone());
    }
}

fn copy_matching(src: &Path, dst: &Path, ext: &str, keep: impl Fn(&str) -> bool) -> anyhow::Result<usize> {
    let mut n = 0;
    if !src.is_dir() {
        return Ok(0);
    }
    let mut stack = vec![src.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d)?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let Some(x) = p.extension().and_then(|s| s.to_str()) else { continue };
            if !x.eq_ignore_ascii_case(ext) {
                continue;
            }
            let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
            if !keep(stem) {
                continue;
            }
            if ext == "xml" && dst.ends_with("Const") {
                let text = std::fs::read_to_string(&p)?;
                if !text.contains("PlcUserConstant") {
                    continue;
                }
            }
            std::fs::copy(&p, dst.join(p.file_name().unwrap()))?;
            n += 1;
        }
    }
    Ok(n)
}
