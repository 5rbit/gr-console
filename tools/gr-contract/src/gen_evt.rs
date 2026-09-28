//! `gr-contract gen-evt`: the `EVT_Const_Gen` constant table from `plc/evtlog/catalog.toml`.

use std::path::PathBuf;

use anyhow::Context;
use evt_catalog::Catalog;
use plc_link::codegen::{ConstDef, normalize_for_compare, render_xml, tia_bytes};

pub const TABLE: &str = "EVT_Const_Gen";

pub struct Args {
    pub plc: String,
    pub catalog: PathBuf,
    pub export: PathBuf,
    pub out_tags: Option<PathBuf>,
    pub check: bool,
}

/// TIA bytes of the table for one PLC plus the constant list.
pub fn table(cat: &Catalog, plc: &str) -> anyhow::Result<(Vec<u8>, Vec<ConstDef>)> {
    let defs: Vec<ConstDef> = cat.constants(plc)?.into_iter().map(|k| ConstDef { name: k.name, data_type: k.data_type, value: k.value, comment_en: k.comment_en, comment_ko: k.comment_ko }).collect();
    Ok((tia_bytes(&render_xml(TABLE, &defs)), defs))
}

/// `Ok(false)` = `--check` found the file out of date.
pub fn run(a: &Args) -> anyhow::Result<bool> {
    let cat = Catalog::load(&a.catalog)?;
    let (bytes, defs) = table(&cat, &a.plc)?;
    let dir = a.out_tags.clone().unwrap_or_else(|| a.export.join(&a.plc).join("tags").join("Const"));
    let dest = dir.join(format!("{TABLE}.xml"));
    let same = std::fs::read(&dest).map(|old| normalize_for_compare(&old) == normalize_for_compare(&bytes)).unwrap_or(false);
    if a.check {
        println!("gen-evt --check {}: {} -> {}", a.plc, dest.display(), if same { "OK" } else { "OUT OF DATE" });
        return Ok(same);
    }
    if same {
        println!("unchanged: {}", dest.display());
    } else {
        std::fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
        std::fs::write(&dest, &bytes).with_context(|| format!("writing {}", dest.display()))?;
        println!("written: {}", dest.display());
    }
    println!("{}: {} constants (catalog v{})", a.plc, defs.len(), cat.version);
    for k in &defs {
        println!("  {:<48} {:<6} {}", k.name, k.data_type, k.value);
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_is_a_tia_tag_table_the_contract_can_read() {
        let cat = Catalog::load(&std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../plc/evtlog/catalog.toml")).unwrap();
        let (bytes, defs) = table(&cat, "GR2_PLC").unwrap();
        assert!(bytes.starts_with(&[0xEF, 0xBB, 0xBF]), "UTF-8 BOM like a TIA export");
        let text = String::from_utf8(bytes[3..].to_vec()).unwrap();
        assert!(text.contains("<Name>EVT_Const_Gen</Name>\r\n"));
        let parsed = plc_layout::consts::parse_const_xml(&text).unwrap();
        assert_eq!(parsed.len(), defs.len());
        assert!(parsed.iter().any(|(n, v, t)| n == "EVT_CATMASK_ALL" && *v == 0x000F_FFFE && t == "DWord"));
        assert!(parsed.iter().any(|(n, v, _)| n == "EVT_TRANS_RAISE" && *v == 1));
        assert!(parsed.iter().any(|(n, v, _)| n == "EVT_GRIP_REQ" && *v == 701));
    }
}
