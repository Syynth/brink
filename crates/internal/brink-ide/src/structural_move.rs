use brink_analyzer::AnalysisResult;
use brink_ir::FileId;
use brink_syntax::ast::{AstNode as _, KnotDef, StitchDef};

use crate::doc_extended_start;
use crate::rename::FileEdit;

/// A declaration's region start for slicing: extended backward over its
/// attached `///` doc block, so docs travel with the declaration they
/// document (per the decision log).
fn decl_region_start(source: &str, node: &brink_syntax::SyntaxNode) -> usize {
    doc_extended_start(source, node.text_range().start().into())
}

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
    #[error("invalid reorder: list is not a permutation of the existing names")]
    InvalidReorder,
}

/// The result of a structural move operation.
pub struct MoveResult {
    /// The new full source text for the primary file.
    pub new_source: String,
    /// Reference edits in other files that must be applied.
    pub cross_file_edits: Vec<FileEdit>,
}

/// Direction for reorder operations.
#[derive(Clone, Copy, serde::Serialize, serde::Deserialize)]
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

    // Compute the end of the knot region (next knot's ownership start or EOF).
    let knot_end: usize = knot_end_offset(source, &knots, ki);

    // Build stitch slices: each stitch owns text from its ownership start
    // (including its doc block) to the next stitch's ownership start (or to
    // the end of the knot region for the last one).
    let last_ast_end: usize = stitches.last().map_or(knot_end, |s| {
        usize::from(s.syntax().text_range().end()).min(knot_end)
    });

    let mut slices: Vec<&str> = Vec::with_capacity(stitches.len());
    for (i, stitch) in stitches.iter().enumerate() {
        let start: usize = decl_region_start(source, stitch.syntax());
        let end: usize = if i + 1 < stitches.len() {
            decl_region_start(source, stitches[i + 1].syntax())
        } else {
            last_ast_end
        };
        slices.push(&source[start..end]);
    }

    // Swap the two adjacent slices.
    slices.swap(si, target_idx);

    // Reassemble: preamble (before first stitch) + reordered slices + trailing.
    let region_start: usize = decl_region_start(source, stitches[0].syntax());
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
/// Each knot owns text from its ownership start (its `///` doc block, if any)
/// to the next knot's ownership start (or EOF).
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

    // Preamble: everything before the first knot's ownership region.
    let preamble_end: usize = decl_region_start(source, knots[0].syntax());

    // Build knot slices: each knot owns text from its ownership start
    // (including its doc block) to the next knot's ownership start (or EOF).
    let mut slices: Vec<&str> = Vec::with_capacity(knots.len());
    for (i, knot) in knots.iter().enumerate() {
        let start: usize = decl_region_start(source, knot.syntax());
        let end: usize = knot_end_offset(source, &knots, i);
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

// ── Phase 1c: order-based reorder ───────────────────────────────────

/// Resolve a target name `order` into indices that permute `current`.
///
/// `order` must be a permutation of `current` (same set, each used once).
/// Returns the index into `current` for each entry of `order`.
fn resolve_permutation(current: &[String], order: &[String]) -> Result<Vec<usize>, MoveError> {
    if order.len() != current.len() {
        return Err(MoveError::InvalidReorder);
    }
    let mut used = vec![false; current.len()];
    let mut out = Vec::with_capacity(current.len());
    for name in order {
        let idx = current
            .iter()
            .position(|n| n == name)
            .ok_or(MoveError::InvalidReorder)?;
        if used[idx] {
            return Err(MoveError::InvalidReorder);
        }
        used[idx] = true;
        out.push(idx);
    }
    Ok(out)
}

/// Reorder all stitches within a knot to match `order` (a permutation of the
/// knot's stitch names).
///
/// Unlike [`reorder_stitch`] (a single ±1 step), this moves stitches to
/// arbitrary positions in one operation — the form drag-and-drop needs, since
/// the drop knows the full destination order, and the form multi-select moves
/// need. Pure text slice/reassemble; whitespace within each stitch is
/// preserved.
pub fn reorder_stitches(
    source: &str,
    knot_name: &str,
    order: &[String],
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
        return Err(MoveError::SourceNotFound);
    };
    let stitches: Vec<_> = body.stitches().collect();
    if stitches.is_empty() {
        return Ok(source.to_owned());
    }

    let names: Vec<String> = stitches
        .iter()
        .map(|s| s.header().and_then(|h| h.name()).unwrap_or_default())
        .collect();
    let new_order = resolve_permutation(&names, order)?;

    // Build stitch slices (each owns text from its ownership start — including
    // its doc block — to the next stitch's ownership start, or to the end of
    // the knot region for the last one).
    let knot_end: usize = knot_end_offset(source, &knots, ki);
    let last_ast_end: usize = stitches.last().map_or(knot_end, |s| {
        usize::from(s.syntax().text_range().end()).min(knot_end)
    });
    let mut slices: Vec<&str> = Vec::with_capacity(stitches.len());
    for (i, stitch) in stitches.iter().enumerate() {
        let start: usize = decl_region_start(source, stitch.syntax());
        let end: usize = if i + 1 < stitches.len() {
            decl_region_start(source, stitches[i + 1].syntax())
        } else {
            last_ast_end
        };
        slices.push(&source[start..end]);
    }

    let region_start: usize = decl_region_start(source, stitches[0].syntax());
    let trailing = &source[last_ast_end..knot_end];

    let mut result = String::with_capacity(source.len());
    result.push_str(&source[..region_start]);
    for &idx in &new_order {
        result.push_str(slices[idx]);
    }
    result.push_str(trailing);
    result.push_str(&source[knot_end..]);
    Ok(result)
}

/// Reorder all top-level knots to match `order` (a permutation of the knot
/// names). The order-based counterpart of [`reorder_knot`]. Preamble before
/// the first knot is preserved.
pub fn reorder_knots(source: &str, order: &[String]) -> Result<String, MoveError> {
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let knots: Vec<_> = tree.knots().collect();
    if knots.is_empty() {
        return Err(MoveError::SourceNotFound);
    }

    let names: Vec<String> = knots
        .iter()
        .map(|k| k.header().and_then(|h| h.name()).unwrap_or_default())
        .collect();
    let new_order = resolve_permutation(&names, order)?;

    let preamble_end: usize = decl_region_start(source, knots[0].syntax());
    let mut slices: Vec<&str> = Vec::with_capacity(knots.len());
    for (i, knot) in knots.iter().enumerate() {
        let start: usize = decl_region_start(source, knot.syntax());
        let end: usize = knot_end_offset(source, &knots, i);
        slices.push(&source[start..end]);
    }

    let mut result = String::with_capacity(source.len());
    result.push_str(&source[..preamble_end]);
    for &idx in &new_order {
        result.push_str(slices[idx]);
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

    // The analyzer records a self-resolution at the definition's own name token
    // (target == def_id, range == the header name). That is the declaration, not
    // a reference — the header is rewritten structurally by the move op, so it
    // must not be treated as a reference edit.
    let def_site = analysis
        .index
        .symbols
        .get(&def_id)
        .map(|info| (info.file, info.range));

    // Parse the source to find which knot each reference lives in.
    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let knots: Vec<_> = tree.knots().collect();

    let mut edits = Vec::new();

    for resolved in &analysis.resolutions {
        if resolved.target != def_id {
            continue;
        }

        // Skip the definition's own name token (see `def_site` above).
        if def_site == Some((resolved.file, resolved.range)) {
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

/// Partition reference edits into same-file and cross-file groups.
///
/// Same-file edits must be folded into the rebuilt `new_source` (see
/// [`apply_window`]); cross-file edits travel out as [`MoveResult::cross_file_edits`].
fn split_same_file(ref_edits: Vec<FileEdit>, file_id: FileId) -> (Vec<FileEdit>, Vec<FileEdit>) {
    ref_edits.into_iter().partition(|e| e.file == file_id)
}

/// Apply the same-file ref edits that fall within `[base, limit)` to the slice
/// `source[base..limit]`, rebasing each edit's offset to the slice, and return
/// the edited slice.
///
/// Structural-move ops rebuild `new_source` by concatenating verbatim slices of
/// the original source plus the relocated/header-rewritten moved text. Routing
/// every such slice through this helper folds the requalified references into
/// the output without disturbing the slice boundaries or whitespace handling.
/// References are atomic tokens fully contained in one knot body, so they never
/// straddle a slice boundary and the windows partition cleanly. Edits are
/// applied in descending offset order so earlier offsets stay valid.
fn apply_window(source: &str, base: usize, limit: usize, same_file: &[FileEdit]) -> String {
    let mut local: Vec<(usize, usize, &str)> = same_file
        .iter()
        .filter_map(|e| {
            let start: usize = e.range.start().into();
            let end: usize = e.range.end().into();
            (start >= base && end <= limit).then(|| (start - base, end - base, e.new_text.as_str()))
        })
        .collect();
    local.sort_by_key(|e| std::cmp::Reverse(e.0));

    let mut slice = source[base..limit].to_owned();
    for (start, end, text) in local {
        slice.replace_range(start..end, text);
    }
    slice
}

/// Insertion point at the end of a destination knot's region, for appending a
/// stitch. Clamped before the next knot's doc block (node ends swallow trailing
/// trivia), and placed after the knot's last existing stitch if it has one.
fn dest_insert_offset(source: &str, knots: &[KnotDef], dki: usize, dest: &KnotDef) -> usize {
    let region_end = knot_end_offset(source, knots, dki);
    let end: usize = match dest.body().and_then(|b| b.stitches().last()) {
        Some(last) => last.syntax().text_range().end().into(),
        None => dest.syntax().text_range().end().into(),
    };
    end.min(region_end)
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
    let (dki, dest_knot_node) =
        find_knot(&knots, dest_knot).ok_or(MoveError::DestinationNotFound)?;

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

    // Extract the stitch text slice (ownership region: doc block included,
    // clamped before the next stitch's doc block).
    let stitch_start: usize = decl_region_start(source, stitch.syntax());
    let src_knot_end: usize = knot_end_offset(source, &knots, ski);
    let stitch_end: usize = if si + 1 < stitches.len() {
        decl_region_start(source, stitches[si + 1].syntax())
    } else {
        stitches.last().map_or(src_knot_end, |s| {
            usize::from(s.syntax().text_range().end()).min(src_knot_end)
        })
    };

    let stitch_text = &source[stitch_start..stitch_end];

    // Compute reference edits before modifying source. Same-file edits are
    // folded into `new_source` via `apply_window`; cross-file edits travel out.
    let old_qual = format!("{src_knot}.{stitch_name}");
    let new_qual = format!("{dest_knot}.{stitch_name}");
    let (same_file, cross_file_edits) = split_same_file(
        compute_reference_edits(source, analysis, file_id, &old_qual, &new_qual),
        file_id,
    );

    // Find the insertion point: end of the destination knot's region.
    let insert_offset = dest_insert_offset(source, &knots, dki, dest_knot_node);

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

    // The moved stitch carries any references that live inside it (e.g. a
    // self/recursive divert), requalified for its new parent.
    let moved_stitch = apply_window(source, stitch_start, stitch_end, &same_file);

    let mut insert_text = String::new();
    if needs_newline_before {
        insert_text.push('\n');
    }
    insert_text.push_str(&moved_stitch);
    if !needs_newline_after && !stitch_text.ends_with('\n') {
        insert_text.push('\n');
    }

    // Apply edits in reverse offset order to preserve positions. Each verbatim
    // slice is routed through `apply_window` so same-file references outside the
    // moved stitch are requalified in place.
    let new_source = if stitch_start > insert_offset {
        // Destination is before source: insert first, then remove.
        let mut s = String::with_capacity(source.len());
        s.push_str(&apply_window(source, 0, insert_offset, &same_file));
        s.push_str(&insert_text);
        s.push_str(&apply_window(
            source,
            insert_offset,
            stitch_start,
            &same_file,
        ));
        s.push_str(&apply_window(source, stitch_end, source.len(), &same_file));
        s
    } else {
        // Source is before destination: remove first, then insert.
        // Adjust insert offset by the removed length.
        let removed_len = stitch_end - stitch_start;
        let adjusted_insert = insert_offset - removed_len;
        let middle_end =
            stitch_end + (adjusted_insert - stitch_start).min(source.len() - stitch_end);
        let mut s = String::with_capacity(source.len());
        s.push_str(&apply_window(source, 0, stitch_start, &same_file));
        s.push_str(&apply_window(source, stitch_end, middle_end, &same_file));
        s.push_str(&insert_text);
        if insert_offset < source.len() {
            s.push_str(&apply_window(
                source,
                insert_offset,
                source.len(),
                &same_file,
            ));
        }
        s
    };

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

    let stitch_start: usize = decl_region_start(source, stitch.syntax());
    let knot_region_end: usize = knot_end_offset(source, &knots, ki);
    let stitch_end: usize = if si + 1 < stitches.len() {
        decl_region_start(source, stitches[si + 1].syntax())
    } else {
        stitches.last().map_or(knot_region_end, |s| {
            usize::from(s.syntax().text_range().end()).min(knot_region_end)
        })
    };

    // Compute reference edits. Same-file edits are folded into `new_source`;
    // cross-file edits travel out.
    let old_qual = format!("{knot_name}.{stitch_name}");
    let new_qual = stitch_name.to_owned();
    let (same_file, cross_file_edits) = split_same_file(
        compute_reference_edits(source, analysis, file_id, &old_qual, &new_qual),
        file_id,
    );

    // Rewrite the header: `= name` or `= name(params)` → `=== name ===` or
    // `=== name(params) ===`. References inside the promoted stitch (e.g. a
    // self-divert) are requalified first, then the header line is rewritten.
    let edited_stitch = apply_window(source, stitch_start, stitch_end, &same_file);
    let promoted_text = rewrite_stitch_to_knot_header(&edited_stitch, stitch_name);

    // Remove stitch from parent knot, insert as new knot after the parent.
    // Verbatim slices are routed through `apply_window` so same-file references
    // outside the moved stitch are requalified in place.
    let mut new_source = String::with_capacity(source.len() + 10);
    new_source.push_str(&apply_window(source, 0, stitch_start, &same_file));
    // Skip removed stitch text, continue with rest of knot.
    new_source.push_str(&apply_window(
        source,
        stitch_end,
        knot_region_end,
        &same_file,
    ));
    // Insert promoted knot.
    if !new_source.ends_with('\n') {
        new_source.push('\n');
    }
    new_source.push_str(&promoted_text);
    if !promoted_text.ends_with('\n') {
        new_source.push('\n');
    }
    // Rest of file after the original knot.
    new_source.push_str(&apply_window(
        source,
        knot_region_end,
        source.len(),
        &same_file,
    ));

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
    let (dki, dest) = find_knot(&knots, dest_knot).ok_or(MoveError::DestinationNotFound)?;

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

    let knot_start: usize = decl_region_start(source, knot.syntax());
    let knot_end: usize = knot_end_offset(source, &knots, ki);

    // Compute reference edits. Same-file edits are folded into `new_source`;
    // cross-file edits travel out.
    let old_qual = knot_name.to_owned();
    let new_qual = format!("{dest_knot}.{knot_name}");
    let (same_file, cross_file_edits) = split_same_file(
        compute_reference_edits(source, analysis, file_id, &old_qual, &new_qual),
        file_id,
    );

    // Rewrite the header: `=== name ===` → `= name`. References inside the
    // demoted knot (e.g. a self-divert) are requalified first, then the header
    // line is rewritten.
    let edited_knot = apply_window(source, knot_start, knot_end, &same_file);
    let demoted_text = rewrite_knot_to_stitch_header(&edited_knot, knot_name);

    // Find insertion point at the end of the destination knot's region.
    let dest_insert = dest_insert_offset(source, &knots, dki, dest);

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

    // Verbatim slices are routed through `apply_window` so same-file references
    // outside the demoted knot are requalified in place.
    let new_source = if knot_start < dest_insert {
        // Source knot is before destination.
        let removed_len = knot_end - knot_start;
        let adjusted_insert = dest_insert - removed_len;
        let mut s = String::with_capacity(source.len());
        s.push_str(&apply_window(source, 0, knot_start, &same_file));
        let middle_end = knot_end + (adjusted_insert - knot_start).min(source.len() - knot_end);
        s.push_str(&apply_window(source, knot_end, middle_end, &same_file));
        s.push_str(&insert_text);
        if dest_insert < source.len() {
            s.push_str(&apply_window(source, dest_insert, source.len(), &same_file));
        }
        s
    } else {
        // Source knot is after destination.
        let mut s = String::with_capacity(source.len());
        s.push_str(&apply_window(source, 0, dest_insert, &same_file));
        s.push_str(&insert_text);
        s.push_str(&apply_window(source, dest_insert, knot_start, &same_file));
        s.push_str(&apply_window(source, knot_end, source.len(), &same_file));
        s
    };

    Ok(MoveResult {
        new_source,
        cross_file_edits,
    })
}

// ── Header rewriting helpers ────────────────────────────────────────

/// Rewrite a stitch header line (`= name` or `= name(params)`) to a knot
/// header (`=== name ===` or `=== name(params) ===`). Leading `///` doc lines
/// (the stitch's ownership region includes its doc block) pass through
/// unchanged before the header.
fn rewrite_stitch_to_knot_header(stitch_text: &str, _name: &str) -> String {
    let mut result = String::with_capacity(stitch_text.len() + 10);
    let mut header_done = false;

    for line in stitch_text.lines() {
        if !header_done {
            let trimmed = line.trim_start();
            if let Some(rest) = trimmed.strip_prefix('=') {
                // The stitch header: `= name` or `= name(params)`
                let rest = rest.trim_start();
                result.push_str("=== ");
                result.push_str(rest.trim_end());
                result.push_str(" ===");
                result.push('\n');
                header_done = true;
                continue;
            }
            // Leading doc/comment lines before the header pass through.
        }
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
/// to a stitch header (`= name` or `= name(params)`). Leading `///` doc lines
/// (the knot's ownership region includes its doc block) pass through
/// unchanged before the header.
fn rewrite_knot_to_stitch_header(knot_text: &str, _name: &str) -> String {
    let mut result = String::with_capacity(knot_text.len());
    let mut header_done = false;

    for line in knot_text.lines() {
        if !header_done {
            let trimmed = line.trim_start();
            if trimmed.starts_with('=') {
                // Strip leading and trailing ='s around "name(params)".
                let rest = trimmed.trim_start_matches('=').trim_start();
                let rest = rest.trim_end().trim_end_matches('=').trim_end();
                result.push_str("= ");
                result.push_str(rest);
                result.push('\n');
                header_done = true;
                continue;
            }
            // Leading doc/comment lines before the header pass through.
        }
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

/// Get the byte offset where a knot's text region ends: the start of the next
/// knot's ownership region (its doc block, if any) or EOF.
fn knot_end_offset(source: &str, knots: &[KnotDef], ki: usize) -> usize {
    if ki + 1 < knots.len() {
        decl_region_start(source, knots[ki + 1].syntax())
    } else {
        source.len()
    }
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    // ── doc-block attachment tests ──────────────────────────────────

    /// Analysis for a single file, for ops that need reference edits.
    fn analyzed(src: &str) -> AnalysisResult {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        brink_analyzer::analyze(&[(FileId(0), &hir, &manifest)])
    }

    #[test]
    fn reorder_knot_carries_doc_block() {
        let source = "\
=== alpha ===
Alpha.
/// Beta's doc.
=== beta ===
Beta.
";
        let result = reorder_knot(source, "beta", Direction::Up).unwrap();
        let doc = result.find("/// Beta's doc.").unwrap();
        let beta = result.find("=== beta ===").unwrap();
        let alpha = result.find("=== alpha ===").unwrap();
        assert!(
            doc < beta && beta < alpha,
            "doc travels with beta: {result}"
        );
        assert!(
            result.starts_with("/// Beta's doc.\n=== beta ==="),
            "doc stays directly attached: {result}"
        );
    }

    #[test]
    fn reorder_knots_keeps_each_doc_attached() {
        let source = "\
/// A doc.
=== a ===
A.
/// B doc.
=== b ===
B.
";
        let result = reorder_knots(source, &["b", "a"].map(String::from)).unwrap();
        assert!(
            result.contains("/// B doc.\n=== b ==="),
            "b keeps its doc: {result}"
        );
        assert!(
            result.contains("/// A doc.\n=== a ==="),
            "a keeps its doc: {result}"
        );
    }

    #[test]
    fn reorder_stitch_carries_doc_block() {
        let source = "\
=== k ===
= alpha
Alpha.
/// Beta's doc.
= beta
Beta.
";
        let result = reorder_stitch(source, "k", "beta", Direction::Up).unwrap();
        assert!(
            result.contains("/// Beta's doc.\n= beta"),
            "doc stays attached to beta: {result}"
        );
        assert!(
            result.find("= beta").unwrap() < result.find("= alpha").unwrap(),
            "{result}"
        );
    }

    #[test]
    fn move_stitch_carries_doc_and_leaves_neighbors() {
        let source = "\
=== src ===
/// Mine.
= movable
Content.
/// Stays here.
= keeper
Kept.
=== dst ===
Dest.
";
        let analysis = analyzed(source);
        let result = move_stitch(source, &analysis, FileId(0), "src", "movable", "dst").unwrap();
        let s = &result.new_source;
        assert!(
            s.contains("/// Mine.\n= movable"),
            "doc travels with the moved stitch: {s}"
        );
        assert!(
            s.contains("/// Stays here.\n= keeper"),
            "the neighbor keeps its doc: {s}"
        );
        let dst = s.find("=== dst ===").unwrap();
        assert!(
            s.find("/// Mine.").unwrap() > dst,
            "moved doc lives in the destination knot: {s}"
        );
    }

    #[test]
    fn promote_stitch_keeps_doc_above_new_knot_header() {
        let source = "\
=== parent ===
Intro.
/// Promoted doc.
= riser
Body.
";
        let analysis = analyzed(source);
        let result =
            promote_stitch_to_knot(source, &analysis, FileId(0), "parent", "riser").unwrap();
        assert!(
            result
                .new_source
                .contains("/// Promoted doc.\n=== riser ==="),
            "doc precedes the promoted header: {}",
            result.new_source
        );
    }

    #[test]
    fn demote_knot_keeps_doc_and_does_not_steal_neighbors() {
        let source = "\
=== dest ===
Dest.
/// Sinking doc.
=== sinker ===
Body.
/// Neighbor doc.
=== neighbor ===
N.
";
        let analysis = analyzed(source);
        let result = demote_knot_to_stitch(source, &analysis, FileId(0), "sinker", "dest").unwrap();
        let s = &result.new_source;
        assert!(
            s.contains("/// Sinking doc.\n= sinker"),
            "doc precedes the demoted header: {s}"
        );
        assert!(
            s.contains("/// Neighbor doc.\n=== neighbor ==="),
            "the following knot keeps its doc: {s}"
        );
    }

    // ── same-file reference requalification ─────────────────────────

    #[test]
    fn promote_requalifies_same_file_reference() {
        let source = "\
=== intro ===
Intro.
= evidence
The evidence.
-> END
=== other ===
-> intro.evidence
";
        let analysis = analyzed(source);
        let result =
            promote_stitch_to_knot(source, &analysis, FileId(0), "intro", "evidence").unwrap();
        let s = &result.new_source;
        assert!(
            s.contains("=== evidence ==="),
            "stitch promoted to a top-level knot: {s}"
        );
        assert!(
            s.contains("-> evidence\n"),
            "the same-file reference is rewritten to the bare promoted name: {s}"
        );
        assert!(
            !s.contains("intro.evidence"),
            "no dangling qualified reference to the old stitch remains: {s}"
        );
        // The same-file edit is folded into new_source, not emitted as cross-file.
        assert!(
            result.cross_file_edits.is_empty(),
            "same-file edits do not leak into cross_file_edits (count {})",
            result.cross_file_edits.len()
        );
    }

    #[test]
    fn demote_requalifies_same_file_reference() {
        let source = "\
=== dest ===
Dest.
=== mover ===
Body.
-> END
=== other ===
-> mover
";
        let analysis = analyzed(source);
        let result = demote_knot_to_stitch(source, &analysis, FileId(0), "mover", "dest").unwrap();
        let s = &result.new_source;
        assert!(
            s.contains("= mover"),
            "knot demoted to a stitch of dest: {s}"
        );
        assert!(
            s.contains("-> dest.mover"),
            "the same-file reference is requalified to dest.mover: {s}"
        );
        assert!(
            result.cross_file_edits.is_empty(),
            "same-file edits do not leak into cross_file_edits (count {})",
            result.cross_file_edits.len()
        );
    }

    #[test]
    fn move_stitch_requalifies_same_file_reference() {
        let source = "\
=== src ===
= movable
Body.
-> END
=== dst ===
Dest.
=== other ===
-> src.movable
";
        let analysis = analyzed(source);
        let result = move_stitch(source, &analysis, FileId(0), "src", "movable", "dst").unwrap();
        let s = &result.new_source;
        assert!(
            s.contains("-> dst.movable"),
            "the same-file reference follows the stitch to its new parent: {s}"
        );
        assert!(
            !s.contains("src.movable"),
            "no dangling reference to the old qualification remains: {s}"
        );
        assert!(
            result.cross_file_edits.is_empty(),
            "same-file edits do not leak into cross_file_edits (count {})",
            result.cross_file_edits.len()
        );
    }

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

    // ── order-based reorder tests ───────────────────────────────────

    #[test]
    fn reorder_stitches_moves_first_to_last() {
        let source = "\
=== my_knot ===
= alpha
Alpha content.
= beta
Beta content.
= gamma
Gamma content.
";
        // Move alpha to the end in one op (a multi-slot move the ±1 API can't do).
        let order = ["beta", "gamma", "alpha"].map(String::from);
        let result = reorder_stitches(source, "my_knot", &order).unwrap();
        let a = result.find("= alpha").unwrap();
        let b = result.find("= beta").unwrap();
        let g = result.find("= gamma").unwrap();
        assert!(b < g && g < a, "order should be beta, gamma, alpha");
        // Content travels with each stitch.
        assert!(result.contains("= alpha\nAlpha content."));
    }

    #[test]
    fn reorder_stitches_identity_is_noop() {
        let source = "\
=== k ===
= a
A.
= b
B.
";
        let order = ["a", "b"].map(String::from);
        assert_eq!(reorder_stitches(source, "k", &order).unwrap(), source);
    }

    #[test]
    fn reorder_stitches_rejects_non_permutation() {
        let source = "=== k ===\n= a\nA.\n= b\nB.\n";
        // Wrong length, unknown name, and duplicate all rejected.
        assert!(reorder_stitches(source, "k", &["a".to_owned()]).is_err());
        assert!(reorder_stitches(source, "k", &["a", "zzz"].map(String::from)).is_err());
        assert!(reorder_stitches(source, "k", &["a", "a"].map(String::from)).is_err());
    }

    #[test]
    fn reorder_knots_moves_to_arbitrary_position() {
        let source = "\
=== one ===
One.
=== two ===
Two.
=== three ===
Three.
";
        let order = ["three", "one", "two"].map(String::from);
        let result = reorder_knots(source, &order).unwrap();
        let o = result.find("=== one ===").unwrap();
        let t = result.find("=== two ===").unwrap();
        let th = result.find("=== three ===").unwrap();
        assert!(th < o && o < t, "order should be three, one, two");
    }

    #[test]
    fn reorder_knots_preserves_preamble() {
        let source = "\
VAR x = 1
// preamble comment
=== a ===
A.
=== b ===
B.
";
        let result = reorder_knots(source, &["b", "a"].map(String::from)).unwrap();
        assert!(result.starts_with("VAR x = 1\n// preamble comment\n"));
        assert!(result.find("=== b ===").unwrap() < result.find("=== a ===").unwrap());
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
