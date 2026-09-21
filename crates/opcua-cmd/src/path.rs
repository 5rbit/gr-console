//! Member-path parsing and normalization.
//!
//! Canonical form: segments joined by `.`, each `Name` optionally followed by one or more
//! `[idx]` parts, no quotes, no whitespace: `TaskData.Position[3]`, `GR[2].CMD`.

use crate::OpcError;

/// One path segment: a name with zero or more array indices.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Segment {
    pub name: String,
    pub indices: Vec<u32>,
}

impl Segment {
    /// `Name[i][j]`
    pub fn render(&self) -> String {
        let mut s = self.name.clone();
        for i in &self.indices {
            s.push('[');
            s.push_str(&i.to_string());
            s.push(']');
        }
        s
    }
}

/// Strip S7-style quotes and surrounding whitespace from a browse name or path piece.
pub fn strip_quotes(s: &str) -> String {
    s.chars().filter(|c| *c != '"').collect::<String>().trim().to_string()
}

/// Parse a single segment `Name[1][2]` (quotes/whitespace tolerated).
pub fn parse_segment(raw: &str) -> Result<Segment, OpcError> {
    let raw = strip_quotes(raw);
    let bad = |why: &str| OpcError::Config(format!("bad path segment {raw:?}: {why}"));
    if raw.is_empty() {
        return Err(bad("empty"));
    }
    let (name, rest) = match raw.find('[') {
        Some(pos) => (raw[..pos].trim().to_string(), &raw[pos..]),
        None => (raw.trim().to_string(), ""),
    };
    if name.is_empty() {
        return Err(bad("missing name before '['"));
    }
    if name.contains(']') {
        return Err(bad("unexpected ']'"));
    }
    let mut indices = Vec::new();
    let mut rest = rest;
    while !rest.is_empty() {
        let Some(stripped) = rest.strip_prefix('[') else {
            return Err(bad("trailing characters after index"));
        };
        let Some(close) = stripped.find(']') else {
            return Err(bad("missing ']'"));
        };
        let idx = stripped[..close].trim();
        let idx: u32 = idx.parse().map_err(|_| bad("index is not a non-negative integer"))?;
        indices.push(idx);
        rest = stripped[close + 1..].trim_start();
    }
    Ok(Segment { name, indices })
}

/// Parse a dotted path into segments. Empty path → empty list.
pub fn parse_path(path: &str) -> Result<Vec<Segment>, OpcError> {
    let path = path.trim();
    if path.is_empty() {
        return Ok(Vec::new());
    }
    path.split('.').map(parse_segment).collect()
}

/// Render segments in canonical form.
pub fn render(segments: &[Segment]) -> String {
    segments.iter().map(Segment::render).collect::<Vec<_>>().join(".")
}

/// Canonicalize a member path: `"TaskData" . "Position" [ 3 ]` → `TaskData.Position[3]`.
pub fn normalize_path(path: &str) -> Result<String, OpcError> {
    Ok(render(&parse_path(path)?))
}

/// Attach a child browse name to a parent path, folding S7 array-element children
/// (`[2]`, `2`, or `GR[2]` under `GR`) into `Parent[2]` instead of a new segment.
pub fn join_child(parent: &str, child_browse_name: &str) -> String {
    let child = strip_quotes(child_browse_name);
    let trimmed = child.trim();

    // `[2]` or `2`
    let bare = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')).map(str::trim).unwrap_or(trimmed);
    if !parent.is_empty()
        && let Ok(idx) = bare.parse::<u32>()
        && bare.chars().all(|c| c.is_ascii_digit())
    {
        return format!("{parent}[{idx}]");
    }

    // `GR[2]` under `GR`, `Position[1]` under `Position`, `GR[2][3]` under `GR[2]`
    if let (Ok(child_seg), Ok(parent_segs)) = (parse_segment(trimmed), parse_path(parent))
        && let Some(last) = parent_segs.last()
        && child_seg.name == last.name
        && child_seg.indices.len() > last.indices.len()
        && child_seg.indices[..last.indices.len()] == last.indices[..]
    {
        let mut segs = parent_segs;
        segs.pop();
        segs.push(child_seg);
        return render(&segs);
    }

    let child_norm = normalize_path(trimmed).unwrap_or_else(|_| trimmed.to_string());
    if parent.is_empty() { child_norm } else { format!("{parent}.{child_norm}") }
}

/// Candidate browse names for the array element `idx` of a node named `name`
/// (with any already-applied indices rendered into `name`).
pub fn element_candidates(name_with_indices: &str, idx: u32) -> [String; 3] {
    [format!("{name_with_indices}[{idx}]"), format!("[{idx}]"), idx.to_string()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_render() {
        assert_eq!(normalize_path("TaskData.Position[3]").unwrap(), "TaskData.Position[3]");
        assert_eq!(normalize_path(" \"TaskData\" . \"Position\" [ 3 ] ").unwrap(), "TaskData.Position[3]");
        assert_eq!(normalize_path("GR[2].CMD").unwrap(), "GR[2].CMD");
        assert_eq!(normalize_path("A[1][2].B").unwrap(), "A[1][2].B");
        assert_eq!(normalize_path("").unwrap(), "");
        assert!(normalize_path("A[").is_err());
        assert!(normalize_path("A[x]").is_err());
        assert!(normalize_path("[1]").is_err());
        assert!(normalize_path("A..B").is_err());
    }

    #[test]
    fn segments() {
        let s = parse_segment("Position[1]").unwrap();
        assert_eq!(s, Segment { name: "Position".into(), indices: vec![1] });
        assert_eq!(s.render(), "Position[1]");
    }

    #[test]
    fn join_children() {
        assert_eq!(join_child("", "Header"), "Header");
        assert_eq!(join_child("Header", "CMD_ID"), "Header.CMD_ID");
        assert_eq!(join_child("TaskData.Position", "[3]"), "TaskData.Position[3]");
        assert_eq!(join_child("TaskData.Position", "3"), "TaskData.Position[3]");
        assert_eq!(join_child("TaskData.Position", "Position[3]"), "TaskData.Position[3]");
        assert_eq!(join_child("TaskData.Position", "\"Position\"[3]"), "TaskData.Position[3]");
        assert_eq!(join_child("Data", "[0]"), "Data[0]");
        assert_eq!(join_child("Data", "0"), "Data[0]");
        assert_eq!(join_child("GR", "GR[2]"), "GR[2]");
        assert_eq!(join_child("GR[2]", "GR[2][1]"), "GR[2][1]");
        // Same-named non-array child is a real child, not an element.
        assert_eq!(join_child("X", "X"), "X.X");
        assert_eq!(join_child("OPCUA", "\"GR\""), "OPCUA.GR");
    }

    #[test]
    fn candidates() {
        assert_eq!(element_candidates("GR", 2), ["GR[2]".to_string(), "[2]".to_string(), "2".to_string()]);
    }
}
