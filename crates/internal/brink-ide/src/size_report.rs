//! The Size view's byte accounting (#3339 phase 4).
//!
//! REAL on-disk bytes from the file's own offset table, never estimates: the
//! sections plus the header sum to the file, and `shipping` is an exact
//! re-serialization without the `DebugInfo` section — what a release export
//! actually produces.
//!
//! Here rather than in the wasm wrapper for the same reason as
//! [`crate::program_model`]: measuring a compiled artifact is not a wasm
//! concern, and the native Program Explorer needs the same numbers.

/// Byte accounting for a compiled `.inkb`, as the Size view's JSON.
///
/// # Errors
/// When `story_bytes` is not a decodable `.inkb`.
pub fn size_report_of(story_bytes: &[u8]) -> Result<serde_json::Value, String> {
    let index = brink_format::read_inkb_index(story_bytes).map_err(|e| e.to_string())?;
    let data = brink_format::read_inkb(story_bytes).map_err(|e| e.to_string())?;

    let sections: Vec<serde_json::Value> = index
        .sections
        .iter()
        .filter_map(|entry| {
            let range = index.section_range(entry.kind)?;
            Some(serde_json::json!({
                "kind": format!("{:?}", entry.kind),
                "bytes": range.len(),
            }))
        })
        .collect();

    let debug = index
        .section_range(brink_format::SectionKind::DebugInfo)
        .map_or(0, |r| r.len());

    // Exact shipping size: re-serialize without debug info.
    let shipping = if data.debug_info.is_some() {
        let mut stripped = data.clone();
        stripped.debug_info = None;
        let mut buf = Vec::new();
        brink_format::write_inkb(&stripped, &mut buf);
        buf.len()
    } else {
        story_bytes.len()
    };

    // Per-scope line-table bytes, each measured alone (minus the u32 count
    // prefix `write_section_line_tables` adds).
    let resolver = crate::program_model::Resolver::new(&data);
    let line_scopes: Vec<serde_json::Value> = data
        .line_tables
        .iter()
        .map(|lt| {
            let mut buf = Vec::new();
            brink_format::write_section_line_tables(std::slice::from_ref(lt), &mut buf);
            let path = resolver.path_or_empty(lt.scope_id);
            serde_json::json!({
                "name": if path.is_empty() { serde_json::Value::Null } else { path.into() },
                "bytes": buf.len().saturating_sub(4),
            })
        })
        .collect();

    let report = serde_json::json!({
        "total": story_bytes.len(),
        "shipping": shipping,
        "debug": debug,
        "header": index.header_size(),
        "sections": sections,
        "line_scopes": line_scopes,
    });
    Ok(report)
}
