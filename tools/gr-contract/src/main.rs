//! gr-contract: PLC contract maintenance.
//!
//! * `sync`    copy the needed DB/UDT/constant sources from a TIA export into `plc/contract/<PLC>/`
//! * `dump`    print the standard-access member table of a DB (compare with the TIA offset column)
//! * `sizes`   print DB / UDT sizes
//! * `gen-sig` compute the LayoutSig of a DB and rewrite the `LayoutSig` start value in a .db source
//! * `gen-link` generate the PLC link SCL / constant table / test vector DB and the PC artifacts

mod gen_link;

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
        /// Link message registry (plc/link/messages.toml): every message `udt` is added to the extra UDTs
        #[arg(long)]
        messages: Option<PathBuf>,
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
    /// Generate JsonW_/JsonR_/LnkJ_ SCL, LNK_Const_Gen.xml, LNK_TestVectors.db and plc/generated/link/<PLC>.
    GenLink {
        #[arg(long)]
        plc: String,
        #[arg(long, default_value = "plc/contract")]
        contract: PathBuf,
        #[arg(long, default_value = "plc/link/messages.toml")]
        messages: PathBuf,
        /// TIA export root (contains <PLC>/{blocks,types,tags})
        #[arg(long, default_value = "../siemens/export")]
        export: PathBuf,
        /// Default <export>/<PLC>/blocks/700. Communication/Socket/Gen
        #[arg(long)]
        out_scl: Option<PathBuf>,
        /// Default <export>/<PLC>/tags/Const
        #[arg(long)]
        out_tags: Option<PathBuf>,
        /// Default plc/generated/link/<PLC>
        #[arg(long)]
        out_pc: Option<PathBuf>,
        /// Write nothing; exit 1 when any file differs (whitespace-normalized compare)
        #[arg(long)]
        check: bool,
        /// Remove generated .scl (marker comment) and PC schema/vector files that were not produced
        #[arg(long)]
        prune: bool,
        /// Every registry message must be available in the contract
        #[arg(long)]
        strict: bool,
        /// Fail when contract .udt files differ from the export's
        #[arg(long)]
        verify_sync: bool,
    },
}

fn main() -> anyhow::Result<()> {
    match Cli::parse().cmd {
        Cmd::Sync { from, out, plc, dbs, mut udts, messages } => {
            if let Some(m) = messages {
                udts.extend(message_udts(&m)?);
            }
            sync(&from, &out, &plc, &dbs, &udts)
        }
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
                match c.size_of_udt(&u).and_then(|s| Ok((s, c.udt_sig(&u)?))) {
                    Ok((s, sig)) => println!("UDT {:<24} {:>7} B  sig 16#{sig:08X}", u, s),
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
        Cmd::GenLink { plc, contract, messages, export, out_scl, out_tags, out_pc, check, prune, strict, verify_sync } => {
            let args = gen_link::Args { plc, contract, messages, export, out_scl, out_tags, out_pc, check, prune, strict, verify_sync };
            if !gen_link::run(&args)? {
                std::process::exit(1);
            }
            Ok(())
        }
    }
}

/// Rewrites the `LayoutSig : DWord [:= ...];` declaration line to carry the start value, and a BEGIN-section
/// `LayoutSig := ...;` assignment (TIA export format) to the same value — the BEGIN value wins on import, so a stale
/// one there would put the old signature into the PLC.
fn patch_sig(text: &str, sig: u32) -> anyhow::Result<String> {
    let mut out = String::with_capacity(text.len() + 32);
    let mut found = false;
    for line in text.split_inclusive('\n') {
        let trimmed = line.trim_start();
        let rest = trimmed.strip_prefix("LayoutSig").map(str::trim_start);
        if let Some(rest) = rest.filter(|r| r.starts_with(":=")) {
            // BEGIN section start value: keep the assignment form (a declaration there is a syntax error in TIA)
            let indent = &line[..line.len() - trimmed.len()];
            let mut l = format!("{indent}LayoutSig := 16#{:04X}_{:04X};", sig >> 16, sig & 0xFFFF);
            if let Some(i) = rest.find("//") {
                l.push_str("   ");
                l.push_str(rest[i..].trim_end());
            }
            let eol = if line.ends_with("\r\n") { "\r\n" } else { "\n" };
            out.push_str(&l);
            out.push_str(eol);
            found = true;
        } else if rest.is_some_and(|r| r.starts_with(':') || r.starts_with('{')) {
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

    let needed = udt_closure(&full, dbs, extra_udts)?;
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

/// Every UDT referenced (transitively) by the DBs and by the extra UDTs, plus the extras themselves.
/// UDTs missing from the export stay in the set (the caller reports them).
fn udt_closure(full: &Contract, dbs: &[String], extra_udts: &[String]) -> anyhow::Result<BTreeSet<String>> {
    let mut needed: BTreeSet<String> = BTreeSet::new();
    // seed with the extras so their children are expanded too
    let mut stack: Vec<TypeRef> = extra_udts.iter().map(|s| s.trim()).filter(|s| !s.is_empty()).map(|s| TypeRef::Udt(s.to_string())).collect();
    for db in dbs {
        let d = full.db(db).with_context(|| format!("DB {db} not in export"))?;
        push_fields(&d.fields, &mut stack);
    }
    let mut seen: HashSet<String> = HashSet::new();
    while let Some(t) = stack.pop() {
        match t {
            TypeRef::Udt(n) => {
                if seen.insert(n.clone()) {
                    needed.insert(n.clone());
                    if let Ok(u) = full.udt(&n) {
                        push_fields(&u.fields, &mut stack);
                    }
                }
            }
            TypeRef::Struct(fields) => push_fields(&fields, &mut stack),
            TypeRef::Array { elem, .. } => stack.push(*elem),
            TypeRef::Prim(_) => {}
        }
    }
    Ok(needed)
}

/// Non-empty `udt` values of every `[[message]]` in a link registry TOML.
fn message_udts(path: &Path) -> anyhow::Result<Vec<String>> {
    let text = std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let doc: toml::Value = toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
    let mut out = Vec::new();
    if let Some(msgs) = doc.get("message").and_then(|m| m.as_array()) {
        for m in msgs {
            if let Some(u) = m.get("udt").and_then(|u| u.as_str()).map(str::trim).filter(|u| !u.is_empty()) {
                out.push(u.to_string());
            }
        }
    }
    Ok(out)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn patch_sig_rewrites_declaration_and_begin_value() {
        let src = "DATA_BLOCK \"X\"\n// 레이아웃은 LayoutSig 로 검증\n   STRUCT\n      LayoutSig : DWord;   //  서명\n   END_STRUCT;\n\nBEGIN\n   LayoutSig := 16#4853_A87B;\n\nEND_DATA_BLOCK\n";
        let out = patch_sig(src, 0x6306_CEEE).unwrap();
        assert!(out.contains("      LayoutSig : DWord := 16#6306CEEE;   //  서명\n"), "{out}");
        assert!(out.contains("\n   LayoutSig := 16#6306_CEEE;\n"), "{out}");
        assert!(out.contains("// 레이아웃은 LayoutSig 로 검증\n"));
        assert!(!out.contains("4853"));
    }

    fn temp_root(tag: &str) -> PathBuf {
        let p = std::env::temp_dir().join(format!("gr-contract-test-{tag}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        p
    }

    fn write(p: &Path, text: &str) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }

    #[test]
    fn sync_expands_children_of_extra_udts() {
        let root = temp_root("sync");
        let src = root.join("export/P");
        write(&src.join("types/A.udt"), "TYPE \"A\"\n   STRUCT\n      B1 : \"B\";\n      Arr : Array[0..1] of \"C\";\n   END_STRUCT;\nEND_TYPE\n");
        write(&src.join("types/B.udt"), "TYPE \"B\"\n   STRUCT\n      X : Int;\n   END_STRUCT;\nEND_TYPE\n");
        write(&src.join("types/C.udt"), "TYPE \"C\"\n   STRUCT\n      S : Struct\n         D1 : \"D\";\n      END_STRUCT;\n   END_STRUCT;\nEND_TYPE\n");
        write(&src.join("types/D.udt"), "TYPE \"D\"\n   STRUCT\n      Y : Bool;\n   END_STRUCT;\nEND_TYPE\n");
        write(&src.join("types/Unused.udt"), "TYPE \"Unused\"\n   STRUCT\n      Z : Bool;\n   END_STRUCT;\nEND_TYPE\n");
        write(&src.join("blocks/DB1.db"), "DATA_BLOCK \"DB1\"\n{ S7_Optimized_Access := 'FALSE' }\n   VAR\n      V : Int;\n   END_VAR\nBEGIN\nEND_DATA_BLOCK\n");
        write(&root.join("messages.toml"), "[[message]]\nid = 1\nname = \"M\"\nudt = \"A\"\n\n[[message]]\nid = 2\nname = \"Heartbeat\"\nudt = \"\"\n");

        let full = Contract::load_dir(&src).unwrap();
        let extras = message_udts(&root.join("messages.toml")).unwrap();
        assert_eq!(extras, vec!["A".to_string()]);
        let closure = udt_closure(&full, &["DB1".to_string()], &extras).unwrap();
        assert_eq!(closure.into_iter().collect::<Vec<_>>(), vec!["A", "B", "C", "D"]);

        let out = root.join("contract");
        sync(&root.join("export"), &out, "P", &["DB1".to_string()], &extras).unwrap();
        for n in ["A", "B", "C", "D"] {
            assert!(out.join(format!("P/types/{n}.udt")).is_file(), "{n} copied");
        }
        assert!(!out.join("P/types/Unused.udt").exists());
        assert!(out.join("P/blocks/DB1.db").is_file());
        let _ = std::fs::remove_dir_all(&root);
    }
}
