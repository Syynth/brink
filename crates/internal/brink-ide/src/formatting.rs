/// Format only a specific knot or stitch region, leaving the rest unchanged.
///
/// Formats the whole document, then replaces only the lines corresponding to
/// the targeted region. Since formatting can change line lengths (shifting byte
/// offsets), we identify the region by line number in the original source.
pub fn format_region(source: &str, knot_name: &str, stitch_name: Option<&str>) -> String {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let knots: Vec<_> = tree.knots().collect();
    let Some((ki, knot)) = knots
        .iter()
        .enumerate()
        .find(|(_, k)| k.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
    else {
        return source.to_owned();
    };

    let knot_start: usize = knot.syntax().text_range().start().into();
    let knot_end: usize = if ki + 1 < knots.len() {
        knots[ki + 1].syntax().text_range().start().into()
    } else {
        source.len()
    };

    let (region_start, region_end) = if let Some(sname) = stitch_name {
        let Some(body) = knot.body() else {
            return source.to_owned();
        };
        let stitches: Vec<_> = body.stitches().collect();
        let Some((si, stitch)) = stitches
            .iter()
            .enumerate()
            .find(|(_, s)| s.header().and_then(|h| h.name()).as_deref() == Some(sname))
        else {
            return source.to_owned();
        };
        let start: usize = stitch.syntax().text_range().start().into();
        let end: usize = if si + 1 < stitches.len() {
            stitches[si + 1].syntax().text_range().start().into()
        } else {
            knot_end
        };
        (start, end)
    } else {
        (knot_start, knot_end)
    };

    // Format the whole file
    let config = brink_fmt::FormatConfig::default();
    let formatted = brink_fmt::format(source, &config);

    // Splice: keep original before/after region, use formatted for the region.
    // Because formatting is line-based and preserves structure, the byte offsets
    // in the original source correctly delimit the region to replace.
    // The formatted output has the same structure, so we re-parse it to find the
    // matching region boundaries.
    let fmt_parse = brink_syntax::parse(&formatted);
    let fmt_tree = fmt_parse.tree();

    let fmt_knots: Vec<_> = fmt_tree.knots().collect();
    let Some((fki, fmt_knot)) = fmt_knots
        .iter()
        .enumerate()
        .find(|(_, k)| k.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
    else {
        return source.to_owned();
    };

    let fmt_knot_start: usize = fmt_knot.syntax().text_range().start().into();
    let fmt_knot_end: usize = if fki + 1 < fmt_knots.len() {
        fmt_knots[fki + 1].syntax().text_range().start().into()
    } else {
        formatted.len()
    };

    let (fmt_region_start, fmt_region_end) = if let Some(sname) = stitch_name {
        let Some(body) = fmt_knot.body() else {
            return source.to_owned();
        };
        let fmt_stitches: Vec<_> = body.stitches().collect();
        let Some((fsi, fmt_stitch)) = fmt_stitches
            .iter()
            .enumerate()
            .find(|(_, s)| s.header().and_then(|h| h.name()).as_deref() == Some(sname))
        else {
            return source.to_owned();
        };
        let start: usize = fmt_stitch.syntax().text_range().start().into();
        let end: usize = if fsi + 1 < fmt_stitches.len() {
            fmt_stitches[fsi + 1].syntax().text_range().start().into()
        } else {
            fmt_knot_end
        };
        (start, end)
    } else {
        (fmt_knot_start, fmt_knot_end)
    };

    let mut result = String::with_capacity(formatted.len());
    result.push_str(&source[..region_start]);
    result.push_str(&formatted[fmt_region_start..fmt_region_end]);
    result.push_str(&source[region_end..]);
    result
}

/// Sort knot definitions in the source alphabetically by name.
///
/// Returns the full source with knots reordered. The preamble (everything before
/// the first knot) is preserved. Each knot's slice runs from its start to just
/// before the next knot (or EOF).
pub fn sort_knots_in_source(source: &str) -> String {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let knots: Vec<_> = tree.knots().collect();
    if knots.len() < 2 {
        return source.to_owned();
    }

    // Separate trailing whitespace after the last knot's AST node so it
    // stays in place after sorting.
    let last_knot_ast_end: usize = knots
        .last()
        .map_or(source.len(), |k| k.syntax().text_range().end().into());
    let trailing = &source[last_knot_ast_end..];

    // Build (name, source_slice) pairs. Each knot owns the text from its start
    // to just before the next knot (or the last knot's AST end).
    let mut knot_slices: Vec<(String, &str)> = Vec::with_capacity(knots.len());
    for (i, knot) in knots.iter().enumerate() {
        let name = knot.header().and_then(|h| h.name()).unwrap_or_default();
        let start: usize = knot.syntax().text_range().start().into();
        let end: usize = if i + 1 < knots.len() {
            knots[i + 1].syntax().text_range().start().into()
        } else {
            last_knot_ast_end
        };
        knot_slices.push((name, &source[start..end]));
    }

    // Preamble: everything before the first knot
    let preamble_end: usize = knots[0].syntax().text_range().start().into();
    let preamble = &source[..preamble_end];

    // Sort by name, case-insensitive
    knot_slices.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    let mut result = String::with_capacity(source.len());
    result.push_str(preamble);
    for (_, slice) in &knot_slices {
        result.push_str(slice);
    }
    result.push_str(trailing);

    result
}

/// Sort stitch definitions within the named knot alphabetically.
///
/// Preserves the knot's preamble content (everything before the first stitch).
/// Each stitch's slice runs from its start to just before the next stitch (or end
/// of the knot body).
pub fn sort_stitches_in_knot(source: &str, knot_name: &str) -> String {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let Some(knot) = tree
        .knots()
        .find(|k| k.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
    else {
        return source.to_owned();
    };

    let Some(body) = knot.body() else {
        return source.to_owned();
    };

    let stitches: Vec<_> = body.stitches().collect();
    if stitches.len() < 2 {
        return source.to_owned();
    }

    // The knot body region we'll rewrite: from first stitch start to the end of
    // the knot's AST node (which is just before the next knot or EOF — the knot
    // owns trailing content up to the next knot boundary).
    let knot_end: usize = knot.syntax().text_range().end().into();
    let region_start: usize = stitches[0].syntax().text_range().start().into();
    let region_end: usize = knot_end;

    // The last stitch's slice would extend to knot_end, which may include
    // trailing whitespace that belongs to the file structure, not the stitch.
    // Separate that trailing whitespace so it stays in place after sorting.
    let last_stitch_ast_end: usize = stitches
        .last()
        .map_or(region_end, |s| s.syntax().text_range().end().into());
    let trailing = &source[last_stitch_ast_end..region_end];

    let mut stitch_slices: Vec<(String, &str)> = Vec::with_capacity(stitches.len());
    for (i, stitch) in stitches.iter().enumerate() {
        let name = stitch.header().and_then(|h| h.name()).unwrap_or_default();
        let start: usize = stitch.syntax().text_range().start().into();
        let end: usize = if i + 1 < stitches.len() {
            stitches[i + 1].syntax().text_range().start().into()
        } else {
            last_stitch_ast_end
        };
        stitch_slices.push((name, &source[start..end]));
    }

    stitch_slices.sort_by(|a, b| a.0.to_lowercase().cmp(&b.0.to_lowercase()));

    let mut result = String::with_capacity(source.len());
    result.push_str(&source[..region_start]);
    for (_, slice) in &stitch_slices {
        result.push_str(slice);
    }
    result.push_str(trailing);
    result.push_str(&source[region_end..]);

    result
}
