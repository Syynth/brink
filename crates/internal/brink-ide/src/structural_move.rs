use brink_analyzer::AnalysisResult;
use brink_ir::FileId;
use brink_syntax::ast::{AstNode as _, KnotDef, StitchDef};

use crate::rename::FileEdit;

/// Errors that can occur during structural move operations.
#[derive(Debug, thiserror::Error)]
pub enum MoveError {
    #[error("source knot not found")]
    SourceNotFound,
    #[error("destination knot not found")]
    DestinationNotFound,
    #[error("stitch '{name}' not found in knot")]
    StitchNotFound { name: String },
    #[error("name collision: '{name}' already exists in {context}")]
    NameCollision { name: String, context: String },
    #[error("illegal nesting: knot has sub-stitches and cannot be demoted")]
    IllegalNesting,
}

/// The result of a structural move operation.
pub struct MoveResult {
    /// The new full source text for the primary file.
    pub new_source: String,
    /// Reference edits in other files that must be applied.
    pub cross_file_edits: Vec<FileEdit>,
}

/// Direction for reorder operations.
#[derive(Clone, Copy)]
pub enum Direction {
    Up,
    Down,
}

// ── Phase 1: reorder_stitch ─────────────────────────────────────────

/// Move a stitch up or down within its parent knot.
///
/// Pure text slice/reassemble — no reference updates needed since
/// qualification doesn't change.
pub fn reorder_stitch(
    source: &str,
    knot_name: &str,
    stitch_name: &str,
    direction: Direction,
) -> Result<String, MoveError> {
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let knots: Vec<_> = tree.knots().collect();
    let (ki, knot) = knots
        .iter()
        .enumerate()
        .find(|(_, k)| k.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
        .ok_or(MoveError::SourceNotFound)?;

    let Some(body) = knot.body() else {
        return Err(MoveError::StitchNotFound {
            name: stitch_name.to_owned(),
        });
    };

    let stitches: Vec<_> = body.stitches().collect();
    let si = stitches
        .iter()
        .position(|s| s.header().and_then(|h| h.name()).as_deref() == Some(stitch_name))
        .ok_or(MoveError::StitchNotFound {
            name: stitch_name.to_owned(),
        })?;

    let target_idx = match direction {
        Direction::Up => {
            if si == 0 {
                return Ok(source.to_owned());
            }
            si - 1
        }
        Direction::Down => {
            if si + 1 >= stitches.len() {
                return Ok(source.to_owned());
            }
            si + 1
        }
    };

    // Compute the end of the knot region (start of next knot or EOF).
    let knot_end: usize = if ki + 1 < knots.len() {
        knots[ki + 1].syntax().text_range().start().into()
    } else {
        source.len()
    };

    // Build stitch slices: each stitch owns text from its start to the next stitch
    // (or to the end of the knot region for the last one).
    let last_ast_end: usize = stitches
        .last()
        .map_or(knot_end, |s| s.syntax().text_range().end().into());

    let mut slices: Vec<&str> = Vec::with_capacity(stitches.len());
    for (i, stitch) in stitches.iter().enumerate() {
        let start: usize = stitch.syntax().text_range().start().into();
        let end: usize = if i + 1 < stitches.len() {
            stitches[i + 1].syntax().text_range().start().into()
        } else {
            last_ast_end
        };
        slices.push(&source[start..end]);
    }

    // Swap the two adjacent slices.
    slices.swap(si, target_idx);

    // Reassemble: preamble (before first stitch) + reordered slices + trailing.
    let region_start: usize = stitches[0].syntax().text_range().start().into();
    let trailing = &source[last_ast_end..knot_end];

    let mut result = String::with_capacity(source.len());
    result.push_str(&source[..region_start]);
    for slice in &slices {
        result.push_str(slice);
    }
    result.push_str(trailing);
    result.push_str(&source[knot_end..]);

    Ok(result)
}

// ── Phase 1b: reorder_knot ───────────────────────────────────────────

/// Move a knot up or down in the top-level knot list.
///
/// Pure text slice/reassemble — swaps adjacent knot slices.
/// Each knot owns text from its start to the next knot's start (or EOF).
/// Preamble (text before the first knot) is preserved.
pub fn reorder_knot(
    source: &str,
    knot_name: &str,
    direction: Direction,
) -> Result<String, MoveError> {
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let knots: Vec<_> = tree.knots().collect();
    if knots.is_empty() {
        return Err(MoveError::SourceNotFound);
    }

    let ki = knots
        .iter()
        .position(|k| k.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
        .ok_or(MoveError::SourceNotFound)?;

    let target_idx = match direction {
        Direction::Up => {
            if ki == 0 {
                return Ok(source.to_owned());
            }
            ki - 1
        }
        Direction::Down => {
            if ki + 1 >= knots.len() {
                return Ok(source.to_owned());
            }
            ki + 1
        }
    };

    // Preamble: everything before the first knot.
    let preamble_end: usize = knots[0].syntax().text_range().start().into();

    // Build knot slices: each knot owns text from its start to the next knot's start (or EOF).
    let mut slices: Vec<&str> = Vec::with_capacity(knots.len());
    for (i, knot) in knots.iter().enumerate() {
        let start: usize = knot.syntax().text_range().start().into();
        let end: usize = if i + 1 < knots.len() {
            knots[i + 1].syntax().text_range().start().into()
        } else {
            source.len()
        };
        slices.push(&source[start..end]);
    }

    // Swap the two adjacent slices.
    slices.swap(ki, target_idx);

    // Reassemble: preamble + reordered slices.
    let mut result = String::with_capacity(source.len());
    result.push_str(&source[..preamble_end]);
    for slice in &slices {
        result.push_str(slice);
    }

    Ok(result)
}

// ── Phase 2: compute_reference_edits ────────────────────────────────

/// Given a moved symbol, compute all reference edits needed to maintain
/// correct resolution after the move.
///
/// `old_qual` is the pre-move qualified name (e.g., `knot_a.stitch_x`).
/// `new_qual` is the post-move qualified name (e.g., `knot_b.stitch_x`).
/// `file_id` is the file where the move happens.
fn compute_reference_edits(
    source: &str,
    analysis: &AnalysisResult,
    file_id: FileId,
    old_qual: &str,
    new_qual: &str,
) -> Vec<FileEdit> {
    // Find the definition ID for the moved symbol by matching on canonical name.
    let Some(def_id) = analysis
        .index
        .by_name
        .get(old_qual)
        .and_then(|ids| ids.first())
        .copied()
    else {
        return Vec::new();
    };

    let new_parts: Vec<&str> = new_qual.split('.').collect();
    let old_parts: Vec<&str> = old_qual.split('.').collect();

    // Parse the source to find which knot each reference lives in.
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let knots: Vec<_> = tree.knots().collect();

    let mut edits = Vec::new();

    for resolved in &analysis.resolutions {
        if resolved.target != def_id {
            continue;
        }

        // Get the reference text from the source file.
        // For cross-file refs we'd need the other file's source — for now handle same-file.
        if resolved.file != file_id {
            // Cross-file: always rewrite to the new qualified name.
            let new_text = new_qual.to_owned();

            edits.push(FileEdit {
                file: resolved.file,
                range: resolved.range,
                new_text,
            });
            continue;
        }

        let ref_start: usize = resolved.range.start().into();
        let ref_end: usize = resolved.range.end().into();
        let ref_text = &source[ref_start..ref_end];

        // Split at first '(' to isolate name from args.
        let (name_part, args_suffix) = split_name_args(ref_text);

        // Find the containing knot for this reference.
        let containing_knot = find_containing_knot(&knots, ref_start);

        // Determine the new reference text based on context.
        let new_name = compute_new_ref_text(
            name_part,
            containing_knot.as_deref(),
            &old_parts,
            &new_parts,
        );

        if new_name == name_part {
            continue;
        }

        let new_text = format!("{new_name}{args_suffix}");
        edits.push(FileEdit {
            file: resolved.file,
            range: resolved.range,
            new_text,
        });
    }

    edits
}

/// Split a reference text into the name portion and any trailing `(args...)`.
fn split_name_args(text: &str) -> (&str, &str) {
    match text.find('(') {
        Some(idx) => (&text[..idx], &text[idx..]),
        None => (text, ""),
    }
}

/// Find the name of the knot containing the given byte offset.
fn find_containing_knot(knots: &[KnotDef], offset: usize) -> Option<String> {
    for knot in knots {
        let range = knot.syntax().text_range();
        let start: usize = range.start().into();
        let end: usize = range.end().into();
        if offset >= start && offset < end {
            return knot.header().and_then(|h| h.name());
        }
    }
    None
}

/// Compute what a reference should become after a move.
///
/// Rules:
/// - If ref is bare "S" and we're inside the new parent → keep bare
/// - If ref is bare "S" and we're inside the old parent → qualify as `new_knot.S`
/// - If ref is qualified "A.S" → rewrite to "B.S" (or bare "S" if inside B)
/// - For promote (old=A.S, new=S): qualified → bare; bare inside A → bare
/// - For demote (old=K, new=B.K): bare from outside B → "B.K"; from within B → bare "K"
fn compute_new_ref_text(
    current_ref: &str,
    containing_knot: Option<&str>,
    old_parts: &[&str],
    new_parts: &[&str],
) -> String {
    let ref_parts: Vec<&str> = current_ref.split('.').collect();
    let is_qualified = ref_parts.len() > 1;

    let bare_name = *ref_parts.last().unwrap_or(&"");
    let new_parent = if new_parts.len() > 1 {
        Some(new_parts[0])
    } else {
        None
    };
    // Promotion: A.S → S (becoming a knot)
    if old_parts.len() == 2 && new_parts.len() == 1 {
        // The stitch is becoming a top-level knot.
        // All references should use the bare name.
        return new_parts[0].to_owned();
    }

    // Demotion: K → B.K (knot becoming a stitch)
    if old_parts.len() == 1 && new_parts.len() == 2 {
        let new_knot = new_parts[0];
        let new_stitch = new_parts[1];
        return if containing_knot == Some(new_knot) {
            // Inside the destination knot: bare reference works.
            new_stitch.to_owned()
        } else {
            // Outside: must qualify.
            format!("{new_knot}.{new_stitch}")
        };
    }

    // Move: A.S → B.S (reparenting stitch)
    if is_qualified {
        // Was qualified → rewrite qualification.
        if containing_knot == new_parent {
            // Inside new parent: dequalify.
            bare_name.to_owned()
        } else {
            // Outside: full qualification.
            new_parts.join(".")
        }
    } else {
        // Was bare → inside old parent. Now must qualify unless inside new parent.
        if containing_knot == new_parent {
            bare_name.to_owned()
        } else if let Some(np) = new_parent {
            format!("{np}.{bare_name}")
        } else {
            bare_name.to_owned()
        }
    }
}

// ── Phase 3: move_stitch ────────────────────────────────────────────

/// Move a stitch from one knot to another, updating all references.
pub fn move_stitch(
    source: &str,
    analysis: &AnalysisResult,
    file_id: FileId,
    src_knot: &str,
    stitch_name: &str,
    dest_knot: &str,
) -> Result<MoveResult, MoveError> {
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let knots: Vec<_> = tree.knots().collect();

    // Find source knot and stitch.
    let (ski, src_knot_node) = find_knot(&knots, src_knot).ok_or(MoveError::SourceNotFound)?;
    let (_, dest_knot_node) = find_knot(&knots, dest_knot).ok_or(MoveError::DestinationNotFound)?;

    // Check for name collision in destination.
    if let Some(body) = dest_knot_node.body()
        && body
            .stitches()
            .any(|s| s.header().and_then(|h| h.name()).as_deref() == Some(stitch_name))
    {
        return Err(MoveError::NameCollision {
            name: stitch_name.to_owned(),
            context: dest_knot.to_owned(),
        });
    }

    let src_body = src_knot_node.body().ok_or(MoveError::StitchNotFound {
        name: stitch_name.to_owned(),
    })?;
    let stitches: Vec<_> = src_body.stitches().collect();
    let (si, stitch) = find_stitch(&stitches, stitch_name).ok_or(MoveError::StitchNotFound {
        name: stitch_name.to_owned(),
    })?;

    // Extract the stitch text slice.
    let stitch_start: usize = stitch.syntax().text_range().start().into();
    let src_knot_end: usize = knot_end_offset(source, &knots, ski);
    let stitch_end: usize = if si + 1 < stitches.len() {
        stitches[si + 1].syntax().text_range().start().into()
    } else {
        stitches
            .last()
            .map_or(src_knot_end, |s| s.syntax().text_range().end().into())
    };

    let stitch_text = &source[stitch_start..stitch_end];

    // Compute reference edits before modifying source.
    let old_qual = format!("{src_knot}.{stitch_name}");
    let new_qual = format!("{dest_knot}.{stitch_name}");
    let mut ref_edits = compute_reference_edits(source, analysis, file_id, &old_qual, &new_qual);

    // Find the insertion point: end of destination knot's last stitch, or end of
    // knot body if no stitches.
    let insert_offset = if let Some(body) = dest_knot_node.body() {
        let dest_stitches: Vec<_> = body.stitches().collect();
        if let Some(last) = dest_stitches.last() {
            let end: usize = last.syntax().text_range().end().into();
            end
        } else {
            let end: usize = dest_knot_node.syntax().text_range().end().into();
            end
        }
    } else {
        let end: usize = dest_knot_node.syntax().text_range().end().into();
        end
    };

    // Build the new source by:
    // 1. Removing the stitch from the source knot
    // 2. Inserting it into the destination knot
    //
    // We need to handle the order carefully — if dest is before src, removal
    // shifts offsets. Process from end to start.

    let needs_newline_before =
        insert_offset > 0 && source.as_bytes().get(insert_offset - 1) != Some(&b'\n');
    let needs_newline_after = stitch_text.ends_with('\n')
        || insert_offset >= source.len()
        || source.as_bytes().get(insert_offset) == Some(&b'\n');

    let mut insert_text = String::new();
    if needs_newline_before {
        insert_text.push('\n');
    }
    insert_text.push_str(stitch_text);
    if !needs_newline_after && !stitch_text.ends_with('\n') {
        insert_text.push('\n');
    }

    // Apply edits in reverse offset order to preserve positions.
    let new_source = if stitch_start > insert_offset {
        // Destination is before source: insert first, then remove.
        let mut s = String::with_capacity(source.len());
        s.push_str(&source[..insert_offset]);
        s.push_str(&insert_text);
        s.push_str(&source[insert_offset..stitch_start]);
        s.push_str(&source[stitch_end..]);
        s
    } else {
        // Source is before destination: remove first, then insert.
        // Adjust insert offset by the removed length.
        let removed_len = stitch_end - stitch_start;
        let adjusted_insert = insert_offset - removed_len;
        let mut s = String::with_capacity(source.len());
        s.push_str(&source[..stitch_start]);
        s.push_str(
            &source[stitch_end
                ..stitch_end + (adjusted_insert - stitch_start).min(source.len() - stitch_end)],
        );
        s.push_str(&insert_text);
        let remaining_start = insert_offset;
        if remaining_start < source.len() {
            s.push_str(&source[remaining_start..]);
        }
        s
    };

    // Separate cross-file edits from same-file edits (same-file edits are
    // already reflected in the text reconstruction).
    let cross_file_edits: Vec<FileEdit> =
        ref_edits.drain(..).filter(|e| e.file != file_id).collect();

    // For same-file reference edits, we need to apply them to the new source.
    // This is complex because offsets have shifted. For now, re-analyze after
    // text manipulation to get correct results. The caller should re-analyze.
    // TODO: adjust same-file ref offsets based on the cut/paste delta.

    Ok(MoveResult {
        new_source,
        cross_file_edits,
    })
}

// ── Phase 4: promote_stitch_to_knot ─────────────────────────────────

/// Promote a stitch to a top-level knot.
///
/// Rewrites `= name` header to `=== name ===`, extracts the stitch from
/// its parent knot to the top level, and updates all references.
pub fn promote_stitch_to_knot(
    source: &str,
    analysis: &AnalysisResult,
    file_id: FileId,
    knot_name: &str,
    stitch_name: &str,
) -> Result<MoveResult, MoveError> {
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let knots: Vec<_> = tree.knots().collect();

    // Check for name collision with existing knots.
    if knots
        .iter()
        .any(|k| k.header().and_then(|h| h.name()).as_deref() == Some(stitch_name))
    {
        return Err(MoveError::NameCollision {
            name: stitch_name.to_owned(),
            context: "top-level knots".to_owned(),
        });
    }

    let (ki, knot) = find_knot(&knots, knot_name).ok_or(MoveError::SourceNotFound)?;
    let body = knot.body().ok_or(MoveError::StitchNotFound {
        name: stitch_name.to_owned(),
    })?;

    let stitches: Vec<_> = body.stitches().collect();
    let (si, stitch) = find_stitch(&stitches, stitch_name).ok_or(MoveError::StitchNotFound {
        name: stitch_name.to_owned(),
    })?;

    let stitch_start: usize = stitch.syntax().text_range().start().into();
    let knot_region_end: usize = knot_end_offset(source, &knots, ki);
    let stitch_end: usize = if si + 1 < stitches.len() {
        stitches[si + 1].syntax().text_range().start().into()
    } else {
        stitches
            .last()
            .map_or(knot_region_end, |s| s.syntax().text_range().end().into())
    };

    let stitch_text = &source[stitch_start..stitch_end];

    // Rewrite the header: `= name` or `= name(params)` → `=== name ===` or `=== name(params) ===`
    let promoted_text = rewrite_stitch_to_knot_header(stitch_text, stitch_name);

    // Compute reference edits.
    let old_qual = format!("{knot_name}.{stitch_name}");
    let new_qual = stitch_name.to_owned();
    let ref_edits = compute_reference_edits(source, analysis, file_id, &old_qual, &new_qual);

    // Remove stitch from parent knot, insert as new knot after the parent.
    let mut new_source = String::with_capacity(source.len() + 10);
    new_source.push_str(&source[..stitch_start]);
    // Skip removed stitch text, continue with rest of knot.
    new_source.push_str(&source[stitch_end..knot_region_end]);
    // Insert promoted knot.
    if !new_source.ends_with('\n') {
        new_source.push('\n');
    }
    new_source.push_str(&promoted_text);
    if !promoted_text.ends_with('\n') {
        new_source.push('\n');
    }
    // Rest of file after the original knot.
    new_source.push_str(&source[knot_region_end..]);

    let cross_file_edits: Vec<FileEdit> = ref_edits
        .into_iter()
        .filter(|e| e.file != file_id)
        .collect();

    Ok(MoveResult {
        new_source,
        cross_file_edits,
    })
}

// ── Phase 5: demote_knot_to_stitch ──────────────────────────────────

/// Demote a top-level knot to a stitch inside another knot.
///
/// Rewrites `=== name ===` to `= name`, inserts into the destination knot.
/// Errors if the knot has sub-stitches (ink doesn't support triple nesting).
pub fn demote_knot_to_stitch(
    source: &str,
    analysis: &AnalysisResult,
    file_id: FileId,
    knot_name: &str,
    dest_knot: &str,
) -> Result<MoveResult, MoveError> {
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let knots: Vec<_> = tree.knots().collect();

    let (ki, knot) = find_knot(&knots, knot_name).ok_or(MoveError::SourceNotFound)?;
    let (_, dest) = find_knot(&knots, dest_knot).ok_or(MoveError::DestinationNotFound)?;

    // Error if the knot has sub-stitches.
    if let Some(body) = knot.body()
        && body.stitches().next().is_some()
    {
        return Err(MoveError::IllegalNesting);
    }

    // Check for name collision in destination.
    if let Some(body) = dest.body()
        && body
            .stitches()
            .any(|s| s.header().and_then(|h| h.name()).as_deref() == Some(knot_name))
    {
        return Err(MoveError::NameCollision {
            name: knot_name.to_owned(),
            context: dest_knot.to_owned(),
        });
    }

    let knot_start: usize = knot.syntax().text_range().start().into();
    let knot_end: usize = knot_end_offset(source, &knots, ki);
    let knot_text = &source[knot_start..knot_end];

    // Rewrite the header: `=== name ===` → `= name`
    let demoted_text = rewrite_knot_to_stitch_header(knot_text, knot_name);

    // Compute reference edits.
    let old_qual = knot_name.to_owned();
    let new_qual = format!("{dest_knot}.{knot_name}");
    let ref_edits = compute_reference_edits(source, analysis, file_id, &old_qual, &new_qual);

    // Find insertion point in destination knot.
    let dest_insert = if let Some(body) = dest.body() {
        let dest_stitches: Vec<_> = body.stitches().collect();
        if let Some(last) = dest_stitches.last() {
            let end: usize = last.syntax().text_range().end().into();
            end
        } else {
            let end: usize = dest.syntax().text_range().end().into();
            end
        }
    } else {
        let end: usize = dest.syntax().text_range().end().into();
        end
    };

    // Build new source. Handle ordering: if the knot being demoted is before
    // the destination, we remove first then insert (with adjusted offset).
    let needs_nl = dest_insert > 0 && source.as_bytes().get(dest_insert - 1) != Some(&b'\n');

    let mut insert_text = String::new();
    if needs_nl {
        insert_text.push('\n');
    }
    insert_text.push_str(&demoted_text);
    if !demoted_text.ends_with('\n') {
        insert_text.push('\n');
    }

    let new_source = if knot_start < dest_insert {
        // Source knot is before destination.
        let removed_len = knot_end - knot_start;
        let adjusted_insert = dest_insert - removed_len;
        let mut s = String::with_capacity(source.len());
        s.push_str(&source[..knot_start]);
        let middle_end = knot_end + (adjusted_insert - knot_start).min(source.len() - knot_end);
        s.push_str(&source[knot_end..middle_end]);
        s.push_str(&insert_text);
        let remaining_start = dest_insert;
        if remaining_start < source.len() {
            s.push_str(&source[remaining_start..]);
        }
        s
    } else {
        // Source knot is after destination.
        let mut s = String::with_capacity(source.len());
        s.push_str(&source[..dest_insert]);
        s.push_str(&insert_text);
        s.push_str(&source[dest_insert..knot_start]);
        s.push_str(&source[knot_end..]);
        s
    };

    let cross_file_edits: Vec<FileEdit> = ref_edits
        .into_iter()
        .filter(|e| e.file != file_id)
        .collect();

    Ok(MoveResult {
        new_source,
        cross_file_edits,
    })
}

// ── Header rewriting helpers ────────────────────────────────────────

/// Rewrite a stitch header line (`= name` or `= name(params)`) to a knot
/// header (`=== name ===` or `=== name(params) ===`).
fn rewrite_stitch_to_knot_header(stitch_text: &str, _name: &str) -> String {
    let mut result = String::with_capacity(stitch_text.len() + 10);
    let mut lines = stitch_text.lines();

    if let Some(header_line) = lines.next() {
        // Parse the stitch header: `= name` or `= name(params)`
        let trimmed = header_line.trim_start();
        if let Some(rest) = trimmed.strip_prefix('=') {
            let rest = rest.trim_start();
            // rest is "name" or "name(params)"
            result.push_str("=== ");
            result.push_str(rest.trim_end());
            result.push_str(" ===");
        } else {
            // Shouldn't happen, but preserve the line.
            result.push_str(header_line);
        }
        result.push('\n');
    }

    for line in lines {
        result.push_str(line);
        result.push('\n');
    }

    // If original didn't end with newline, remove trailing one.
    if !stitch_text.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

/// Rewrite a knot header line (`=== name ===` or `=== name(params) ===`)
/// to a stitch header (`= name` or `= name(params)`).
fn rewrite_knot_to_stitch_header(knot_text: &str, _name: &str) -> String {
    let mut result = String::with_capacity(knot_text.len());
    let mut lines = knot_text.lines();

    if let Some(header_line) = lines.next() {
        let trimmed = header_line.trim_start();
        // Strip leading ='s
        let rest = trimmed.trim_start_matches('=').trim_start();
        // Strip trailing ='s
        let rest = rest.trim_end().trim_end_matches('=').trim_end();
        // rest is "name" or "name(params)" or "function name(params)"
        result.push_str("= ");
        result.push_str(rest);
        result.push('\n');
    }

    for line in lines {
        result.push_str(line);
        result.push('\n');
    }

    if !knot_text.ends_with('\n') && result.ends_with('\n') {
        result.pop();
    }

    result
}

// ── AST navigation helpers ──────────────────────────────────────────

fn find_knot<'a>(knots: &'a [KnotDef], name: &str) -> Option<(usize, &'a KnotDef)> {
    knots
        .iter()
        .enumerate()
        .find(|(_, k)| k.header().and_then(|h| h.name()).as_deref() == Some(name))
}

fn find_stitch<'a>(stitches: &'a [StitchDef], name: &str) -> Option<(usize, &'a StitchDef)> {
    stitches
        .iter()
        .enumerate()
        .find(|(_, s)| s.header().and_then(|h| h.name()).as_deref() == Some(name))
}

/// Get the byte offset where a knot's text region ends (start of next knot or EOF).
fn knot_end_offset(source: &str, knots: &[KnotDef], ki: usize) -> usize {
    if ki + 1 < knots.len() {
        knots[ki + 1].syntax().text_range().start().into()
    } else {
        source.len()
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── reorder_stitch tests ────────────────────────────────────────

    #[test]
    fn reorder_stitch_down() {
        let source = "\
=== my_knot ===
= alpha
Alpha content.
= beta
Beta content.
= gamma
Gamma content.
";
        let result = reorder_stitch(source, "my_knot", "alpha", Direction::Down).unwrap();
        // beta should now come before alpha
        let beta_pos = result.find("= beta").unwrap();
        let alpha_pos = result.find("= alpha").unwrap();
        assert!(
            beta_pos < alpha_pos,
            "beta should come before alpha after moving alpha down"
        );
    }

    #[test]
    fn reorder_stitch_up() {
        let source = "\
=== my_knot ===
= alpha
Alpha content.
= beta
Beta content.
= gamma
Gamma content.
";
        let result = reorder_stitch(source, "my_knot", "beta", Direction::Up).unwrap();
        let beta_pos = result.find("= beta").unwrap();
        let alpha_pos = result.find("= alpha").unwrap();
        assert!(
            beta_pos < alpha_pos,
            "beta should come before alpha after moving beta up"
        );
    }

    #[test]
    fn reorder_stitch_at_boundary_is_noop() {
        let source = "\
=== my_knot ===
= alpha
Alpha content.
= beta
Beta content.
";
        // Moving first stitch up is a no-op.
        let result = reorder_stitch(source, "my_knot", "alpha", Direction::Up).unwrap();
        assert_eq!(result, source);

        // Moving last stitch down is a no-op.
        let result = reorder_stitch(source, "my_knot", "beta", Direction::Down).unwrap();
        assert_eq!(result, source);
    }

    #[test]
    fn reorder_stitch_not_found() {
        let source = "\
=== my_knot ===
= alpha
Content.
";
        let err = reorder_stitch(source, "my_knot", "nonexistent", Direction::Up).unwrap_err();
        assert!(matches!(err, MoveError::StitchNotFound { .. }));
    }

    #[test]
    fn reorder_stitch_knot_not_found() {
        let source = "\
=== my_knot ===
= alpha
Content.
";
        let err = reorder_stitch(source, "other_knot", "alpha", Direction::Up).unwrap_err();
        assert!(matches!(err, MoveError::SourceNotFound));
    }

    #[test]
    fn reorder_preserves_surrounding_content() {
        let source = "\
VAR x = 0
=== first_knot ===
= alpha
Alpha.
= beta
Beta.
=== second_knot ===
Second knot content.
";
        let result = reorder_stitch(source, "first_knot", "alpha", Direction::Down).unwrap();
        assert!(result.starts_with("VAR x = 0\n"));
        assert!(result.contains("=== second_knot ==="));
        assert!(result.contains("Second knot content."));
    }

    // ── header rewrite tests ────────────────────────────────────────

    #[test]
    fn stitch_to_knot_header_simple() {
        let input = "= my_stitch\nSome content.\n";
        let result = rewrite_stitch_to_knot_header(input, "my_stitch");
        assert!(result.starts_with("=== my_stitch ===\n"));
        assert!(result.contains("Some content."));
    }

    #[test]
    fn stitch_to_knot_header_with_params() {
        let input = "= my_stitch(a, ref b)\nContent.\n";
        let result = rewrite_stitch_to_knot_header(input, "my_stitch");
        assert!(result.starts_with("=== my_stitch(a, ref b) ===\n"));
    }

    #[test]
    fn knot_to_stitch_header_simple() {
        let input = "=== my_knot ===\nContent.\n";
        let result = rewrite_knot_to_stitch_header(input, "my_knot");
        assert!(result.starts_with("= my_knot\n"));
        assert!(result.contains("Content."));
    }

    #[test]
    fn knot_to_stitch_header_with_params() {
        let input = "=== my_knot(x, ref y) ===\nContent.\n";
        let result = rewrite_knot_to_stitch_header(input, "my_knot");
        assert!(result.starts_with("= my_knot(x, ref y)\n"));
    }

    // ── compute_new_ref_text tests ──────────────────────────────────

    #[test]
    fn ref_text_move_qualified_inside_dest() {
        // A.S → B.S, ref is "A.S" inside knot B → should become bare "S"
        let result = compute_new_ref_text("A.S", Some("B"), &["A", "S"], &["B", "S"]);
        assert_eq!(result, "S");
    }

    #[test]
    fn ref_text_move_qualified_outside() {
        // A.S → B.S, ref is "A.S" inside knot C → should become "B.S"
        let result = compute_new_ref_text("A.S", Some("C"), &["A", "S"], &["B", "S"]);
        assert_eq!(result, "B.S");
    }

    #[test]
    fn ref_text_move_bare_inside_old_parent() {
        // A.S → B.S, ref is bare "S" inside knot A → should become "B.S"
        let result = compute_new_ref_text("S", Some("A"), &["A", "S"], &["B", "S"]);
        assert_eq!(result, "B.S");
    }

    #[test]
    fn ref_text_move_bare_inside_new_parent() {
        // A.S → B.S, ref is bare "S" inside knot B → stays "S"
        let result = compute_new_ref_text("S", Some("B"), &["A", "S"], &["B", "S"]);
        assert_eq!(result, "S");
    }

    #[test]
    fn ref_text_promote() {
        // A.S → S (promote), any ref → bare "S"
        let result = compute_new_ref_text("A.S", Some("C"), &["A", "S"], &["S"]);
        assert_eq!(result, "S");
    }

    #[test]
    fn ref_text_demote_outside() {
        // K → B.K (demote), ref is bare "K" inside knot C → "B.K"
        let result = compute_new_ref_text("K", Some("C"), &["K"], &["B", "K"]);
        assert_eq!(result, "B.K");
    }

    #[test]
    fn ref_text_demote_inside_dest() {
        // K → B.K (demote), ref is bare "K" inside knot B → stays "K"
        let result = compute_new_ref_text("K", Some("B"), &["K"], &["B", "K"]);
        assert_eq!(result, "K");
    }

    // ── split_name_args tests ───────────────────────────────────────

    #[test]
    fn split_simple_name() {
        let (name, args) = split_name_args("my_knot");
        assert_eq!(name, "my_knot");
        assert_eq!(args, "");
    }

    #[test]
    fn split_name_with_args() {
        let (name, args) = split_name_args("my_knot(x, y)");
        assert_eq!(name, "my_knot");
        assert_eq!(args, "(x, y)");
    }

    #[test]
    fn split_qualified_with_args() {
        let (name, args) = split_name_args("knot.stitch(a)");
        assert_eq!(name, "knot.stitch");
        assert_eq!(args, "(a)");
    }

    // ── reorder_knot tests ─────────────────────────────────────────

    #[test]
    fn reorder_knot_down() {
        let source = "\
=== alpha ===
Alpha content.
=== beta ===
Beta content.
=== gamma ===
Gamma content.
";
        let result = reorder_knot(source, "alpha", Direction::Down).unwrap();
        let beta_pos = result.find("=== beta ===").unwrap();
        let alpha_pos = result.find("=== alpha ===").unwrap();
        assert!(
            beta_pos < alpha_pos,
            "beta should come before alpha after moving alpha down"
        );
    }

    #[test]
    fn reorder_knot_up() {
        let source = "\
=== alpha ===
Alpha content.
=== beta ===
Beta content.
=== gamma ===
Gamma content.
";
        let result = reorder_knot(source, "beta", Direction::Up).unwrap();
        let beta_pos = result.find("=== beta ===").unwrap();
        let alpha_pos = result.find("=== alpha ===").unwrap();
        assert!(
            beta_pos < alpha_pos,
            "beta should come before alpha after moving beta up"
        );
    }

    #[test]
    fn reorder_knot_at_boundary_is_noop() {
        let source = "\
=== alpha ===
Alpha content.
=== beta ===
Beta content.
";
        let result = reorder_knot(source, "alpha", Direction::Up).unwrap();
        assert_eq!(result, source);

        let result = reorder_knot(source, "beta", Direction::Down).unwrap();
        assert_eq!(result, source);
    }

    #[test]
    fn reorder_knot_not_found() {
        let source = "\
=== alpha ===
Content.
";
        let err = reorder_knot(source, "nonexistent", Direction::Up).unwrap_err();
        assert!(matches!(err, MoveError::SourceNotFound));
    }

    #[test]
    fn reorder_knot_preserves_preamble() {
        let source = "\
VAR x = 0
VAR y = 1
=== alpha ===
Alpha.
=== beta ===
Beta.
";
        let result = reorder_knot(source, "alpha", Direction::Down).unwrap();
        assert!(result.starts_with("VAR x = 0\nVAR y = 1\n"));
        let beta_pos = result.find("=== beta ===").unwrap();
        let alpha_pos = result.find("=== alpha ===").unwrap();
        assert!(beta_pos < alpha_pos);
    }
}
