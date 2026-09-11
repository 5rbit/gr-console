//! PLC user constants from SimaticML tag-table XML (`tags/Const/*.xml`).

use crate::LayoutError;

/// Returns (name, value, data type) for every `SW.Tags.PlcUserConstant` in the document.
pub fn parse_const_xml(xml: &str) -> Result<Vec<(String, i64, String)>, LayoutError> {
    let doc = roxmltree::Document::parse(xml.trim_start_matches('\u{feff}')).map_err(|e| LayoutError::Parse { line: None, msg: format!("xml: {e}") })?;
    let mut out = Vec::new();
    for node in doc.descendants().filter(|n| n.has_tag_name("SW.Tags.PlcUserConstant")) {
        let Some(attrs) = node.children().find(|c| c.has_tag_name("AttributeList")) else { continue };
        let get = |tag: &str| attrs.children().find(|c| c.has_tag_name(tag)).and_then(|c| c.text()).map(|s| s.trim().to_string());
        let (Some(name), Some(value)) = (get("Name"), get("Value")) else { continue };
        let ty = get("DataTypeName").unwrap_or_default();
        if let Some(v) = parse_literal(&value) {
            out.push((name, v, ty));
        }
    }
    Ok(out)
}

/// Parses SCL numeric literals: `123`, `16#43`, `2#1010`, `8#17`, `TRUE`/`FALSE`, with `_` separators.
pub fn parse_literal(v: &str) -> Option<i64> {
    let v = v.trim().replace('_', "");
    let lower = v.to_ascii_lowercase();
    if lower == "true" {
        return Some(1);
    }
    if lower == "false" {
        return Some(0);
    }
    if let Some((radix, digits)) = v.split_once('#') {
        let r = match radix {
            "16" => 16,
            "2" => 2,
            "8" => 8,
            "10" => 10,
            _ => return None,
        };
        return i64::from_str_radix(digits, r).ok();
    }
    v.parse::<i64>().ok().or_else(|| v.parse::<f64>().ok().map(|f| f as i64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_constants() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?><Document><SW.Tags.PlcTagTable ID="0"><ObjectList>
<SW.Tags.PlcUserConstant ID="1"><AttributeList><DataTypeName>Byte</DataTypeName><Name>CMD_TASK_MOVE</Name><Value>16#43</Value></AttributeList></SW.Tags.PlcUserConstant>
<SW.Tags.PlcUserConstant ID="2"><AttributeList><DataTypeName>Int</DataTypeName><Name>MEAS_LOG_LAST</Name><Value>199</Value></AttributeList></SW.Tags.PlcUserConstant>
</ObjectList></SW.Tags.PlcTagTable></Document>"#;
        let c = parse_const_xml(xml).unwrap();
        assert_eq!(c[0], ("CMD_TASK_MOVE".into(), 0x43, "Byte".into()));
        assert_eq!(c[1].1, 199);
    }
}
