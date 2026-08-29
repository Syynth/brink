//! HIR normalization pass: lift inline sequences/conditionals to block-level.
//!
//! This runs on a **cloned** HIR before LIR lowering. The stored HIR in the
//! project DB stays pristine — the LSP sees the original structure.
//!
//! The transform expands inline `InlineSequence` / `InlineConditional` content
//! parts into block-level `Sequence` / `Conditional` statements. Each branch
//! gets the surrounding text spliced in, producing complete content lines that
//! the recognizer can match as `Plain` or `Template`.
//!
//! ## Post-stage-3 role: the fallback for lines the variant model declines
//!
//! Since the #3274 flip, a line that [`claims_variant_line`] admits — every
//! inline alternative plain-kinded and textual, no conditional, no glue —
//! is NOT lifted: it passes through whole and LIR lowering enumerates it
//! into one variant group over shared alternative containers. The lift is
//! the compilation model for everything the claim declines, and stage 3
//! (#3275) established by reachability analysis that **all of it is
//! load-bearing residue** — none of it retired:
//!
//! * combo-kind lines (`shuffle|once`, `shuffle|stopping`) keep the lift
//!   **by ruling** (2026-08-29): each rendering stays a whole line in the
//!   line table — a translation unit and a VO slot — which moving them to
//!   the shared-inline fragment path would break;
//! * conditional-bearing and structural-branch (divert/glue/nested) lines
//!   can never be whole-line variants, so they lift; the once→stopping
//!   exhausted-branch synthesis stays reachable through exactly these
//!   lines (e.g. a plain `{!…}` beside an inline conditional), as does
//!   [`synthesized_else_branch`] through every lifted no-else conditional;
//! * a cloned **stateful** alternative keeps its stamped container id in
//!   every branch (shared visit-count state, the #3275 mixed-line ruling),
//!   revoked per lift level by [`revoke_sharing_if_unclaimed`] when a
//!   branch fails to reassemble into a claimable variant line.
//!
//! [`claims_variant_line`]: crate::lir::lower::recognize::claims_variant_line

use super::types::{
    Block, CondBranch, Conditional, Content, ContentPart, HirFile, Sequence, SequenceBranch,
    SequenceType, Stmt, Tag,
};

// ─── Public entry point ─────────────────────────────────────────────

/// Normalize an entire HIR file by lifting inline sequences/conditionals
/// in all blocks (root, knot bodies, stitch bodies).
pub fn normalize_file(hir: &mut HirFile) {
    normalize_block(&mut hir.root_content);
    for knot in &mut hir.knots {
        normalize_block(&mut knot.body);
        for stitch in &mut knot.stitches {
            normalize_block(&mut stitch.body);
        }
    }
}

// ─── Block normalization ────────────────────────────────────────────

/// Walk a block's statements, lifting inline constructs to block-level
/// and recursing into contained blocks.
fn normalize_block(block: &mut Block) {
    let old_stmts = std::mem::take(&mut block.stmts);
    let mut new_stmts = Vec::with_capacity(old_stmts.len());

    let mut iter = old_stmts.into_iter().peekable();
    while let Some(stmt) = iter.next() {
        match stmt {
            Stmt::Content(content) => {
                // #3274 (stage-2 flip): a line the variant model claims —
                // all inline alternatives textual and plain-kinded, every
                // enumerated variant recognizable — is NOT lifted. It
                // passes through whole so LIR lowering can enumerate it
                // into one variant group over SHARED alternative
                // containers, which is what makes two stateful
                // alternatives on one line advance together (ink's
                // documented semantics, #3271) instead of the cartesian
                // clone giving each spliced copy its own visit count.
                if crate::lir::lower::recognize::claims_variant_line(&content) {
                    new_stmts.push(Stmt::Content(content));
                    continue;
                }

                // Check if the next stmt is EndOfLine — we absorb it into branches.
                let trailing_eol = matches!(iter.peek(), Some(Stmt::EndOfLine));

                match try_lift_inline(content, trailing_eol) {
                    Ok(lifted_stmts) => {
                        // Consume the EndOfLine we peeked at.
                        if trailing_eol {
                            let _ = iter.next();
                        }
                        new_stmts.extend(lifted_stmts);
                    }
                    Err(content) => {
                        // No inline construct — pass through.
                        new_stmts.push(Stmt::Content(content));
                    }
                }
            }
            // Recurse into contained blocks for all structural statements.
            Stmt::ChoiceSet(mut cs) => {
                for choice in &mut cs.choices {
                    normalize_block(&mut choice.body);
                }
                normalize_block(&mut cs.continuation);
                new_stmts.push(Stmt::ChoiceSet(cs));
            }
            Stmt::LabeledBlock(mut lb) => {
                normalize_block(&mut lb);
                new_stmts.push(Stmt::LabeledBlock(lb));
            }
            Stmt::Conditional(mut cond) => {
                for branch in &mut cond.branches {
                    normalize_block(&mut branch.body);
                }
                new_stmts.push(Stmt::Conditional(cond));
            }
            Stmt::Sequence(mut seq) => {
                for branch in &mut seq.branches {
                    normalize_block(&mut branch.body);
                }
                new_stmts.push(Stmt::Sequence(seq));
            }
            other => new_stmts.push(other),
        }
    }

    block.stmts = new_stmts;

    // Recurse into any newly created Sequence/Conditional branches
    // (handles cartesian product from multiple inline constructs).
    for stmt in &mut block.stmts {
        match stmt {
            Stmt::Sequence(seq) => {
                for branch in &mut seq.branches {
                    normalize_block(&mut branch.body);
                }
            }
            Stmt::Conditional(cond) => {
                for branch in &mut cond.branches {
                    normalize_block(&mut branch.body);
                }
            }
            _ => {}
        }
    }
}

// ─── Inline lifting ─────────────────────────────────────────────────

/// Try to lift the first `InlineSequence` or `InlineConditional` from a
/// Content's parts into a block-level statement.
///
/// Returns `Ok(stmts)` with the replacement statements, or `Err(content)`
/// if no inline construct was found (caller passes through unchanged).
fn try_lift_inline(content: Content, trailing_eol: bool) -> Result<Vec<Stmt>, Content> {
    let Some(idx) = lift_index(&content.parts) else {
        return Err(content);
    };

    let prefix: Vec<ContentPart> = content.parts[..idx].to_vec();
    let suffix: Vec<ContentPart> = content.parts[idx + 1..].to_vec();
    let tags = &content.tags;
    let ptr = content.ptr;

    match &content.parts[idx] {
        ContentPart::InlineSequence(seq) => {
            let mut branches = Vec::with_capacity(seq.branches.len() + 1);
            for (branch_idx, branch) in seq.branches.iter().enumerate() {
                let mut b = branch.body.clone();
                // #3275 (stage 3a): ids are stamped BEFORE this lift, so
                // the prefix/suffix spliced into each branch can carry
                // stamped container/lambda ids — clones after the first
                // re-derive them, except a cloned stateful alternative,
                // which keeps its id in every clone (shared visit-count
                // state, the ruled ink semantics).
                let (p, s, t) = salted_splice_sources(&prefix, &suffix, tags, branch_idx as u64);
                splice_around(&mut b, &p, &s, &t, ptr);
                if trailing_eol {
                    b.stmts.push(Stmt::EndOfLine);
                }
                // `splice_around` (suffix) and the EndOfLine push can both
                // change the trailing stmt, so the cloned branch's `tail` may
                // be stale — recompute it (harmless today, load-bearing at the
                // S3 cutover). This pass runs on cloned HIR right before LIR.
                b.recompute_tail();
                branches.push(SequenceBranch {
                    ptr: branch.ptr,
                    body: b,
                });
            }

            // `once` sequences exhaust their branches and then produce nothing.
            // When prefix/suffix text exists, it must still be emitted after
            // exhaustion. Add an extra "exhausted" branch with just prefix+suffix
            // and change to `stopping` so the last branch repeats forever.
            //
            // This is only valid for plain `once` (sequential). `shuffle | once`
            // would shuffle the extra branch into the pool — skip the conversion
            // and fall back to the existing inline sequence lowering for that case.
            let is_plain_once =
                seq.kind.contains(SequenceType::ONCE) && !seq.kind.contains(SequenceType::SHUFFLE);
            let kind = if is_plain_once && (!prefix.is_empty() || !suffix.is_empty()) {
                let mut exhausted = Block::default();
                let (p, s, t) =
                    salted_splice_sources(&prefix, &suffix, tags, seq.branches.len() as u64);
                splice_around(&mut exhausted, &p, &s, &t, ptr);
                if trailing_eol {
                    exhausted.stmts.push(Stmt::EndOfLine);
                }
                exhausted.recompute_tail();
                // Synthesized branch, not sourced from a real arm — the
                // whole sequence's own span is the narrowest available
                // fallback (matches the "no dedicated source node" posture
                // documented on `SequenceBranch`). Its container id is
                // derived from the wrapper's (#3275): no pristine node
                // exists to have been stamped, and the stamp walk cannot
                // predict this synthesis (it depends on prefix/suffix).
                exhausted.container_id = seq
                    .container_id
                    .map(|id| super::stamp::derive_id(id, "exhausted", 0));
                branches.push(SequenceBranch {
                    ptr: seq.ptr,
                    body: exhausted,
                });
                // Replace `once` with `stopping` so the exhausted branch repeats.
                (seq.kind & !SequenceType::ONCE) | SequenceType::STOPPING
            } else {
                seq.kind
            };

            revoke_sharing_if_unclaimed(&prefix, &suffix, branches.iter_mut().map(|b| &mut b.body));

            Ok(vec![Stmt::Sequence(Sequence {
                ptr: seq.ptr,
                kind,
                branches,
                // Inherited from the pristine stamp (#3275) — the lift
                // never re-mints ids.
                container_id: seq.container_id,
            })])
        }
        ContentPart::InlineConditional(cond) => {
            let mut branches = Vec::with_capacity(cond.branches.len() + 1);
            for (branch_idx, branch) in cond.branches.iter().enumerate() {
                let mut body = branch.body.clone();
                let (p, s, t) = salted_splice_sources(&prefix, &suffix, tags, branch_idx as u64);
                splice_around(&mut body, &p, &s, &t, ptr);
                if trailing_eol {
                    body.stmts.push(Stmt::EndOfLine);
                }
                body.recompute_tail();
                branches.push(CondBranch {
                    ptr: branch.ptr,
                    // B1b (issue #1475): a lifted inline `{if EXPR as n: …}`
                    // keeps its binding — this rebuild is a body rewrite
                    // (prefix/suffix splice), not a re-lowering, so dropping
                    // it here would silently unbind the arm.
                    condition: branch.condition.clone(),
                    binding: branch.binding.clone(),
                    body,
                    // Inherited from the pristine stamp (#3275).
                    container_id: branch.container_id,
                });
            }

            // If no else branch exists and there's prefix/suffix text that
            // must be emitted even when all conditions are false, add an else
            // branch with just the surrounding text. Without this, text like
            // "A " in `A {cond:B}` would be lost when `cond` is false.
            let has_else = branches.iter().any(|b| b.condition.is_none());
            if !has_else && (!prefix.is_empty() || !suffix.is_empty()) {
                branches.push(synthesized_else_branch(
                    cond,
                    &prefix,
                    &suffix,
                    tags,
                    ptr,
                    trailing_eol,
                ));
            }

            revoke_sharing_if_unclaimed(&prefix, &suffix, branches.iter_mut().map(|b| &mut b.body));

            Ok(vec![Stmt::Conditional(Conditional {
                ptr: cond.ptr,
                kind: cond.kind.clone(),
                branches,
            })])
        }
        _ => unreachable!("position() matched only InlineSequence/InlineConditional"),
    }
}

/// Which inline construct [`try_lift_inline`] lifts.
///
/// A label-bearing `InlineConditional` lifts FIRST when one exists
/// (#3272): whichever construct lifts first is the one that is NOT
/// cloned — every other construct on the line gets spliced into each of
/// its branches. Cloning a labeled construct (a `(dup)` choice inside an
/// `{if …}`) stamps one label's `DefinitionId` onto two containers, which
/// codegen's #1673 uniqueness guard correctly refuses as E060 — an
/// internal-error wording for legal-looking source. Lifting the label
/// carrier first keeps the label on exactly one container; the constructs
/// cloned into its branches are then handled by the recursive normalize
/// pass (a textual alternative becomes a per-branch variant line — an
/// accepted per-branch state split on these mixed lines, #3274's item 3).
/// Otherwise: the first inline construct, the long-standing order.
fn lift_index(parts: &[ContentPart]) -> Option<usize> {
    parts
        .iter()
        .position(|p| match p {
            ContentPart::InlineConditional(cond) => {
                cond.branches.iter().any(|b| block_contains_label(&b.body))
            }
            _ => false,
        })
        .or_else(|| {
            parts.iter().position(|p| {
                matches!(
                    p,
                    ContentPart::InlineSequence(_) | ContentPart::InlineConditional(_)
                )
            })
        })
}

// ─── Label detection (#3272) ────────────────────────────────────────

/// Whether a block contains any labeled construct — a labeled choice, a
/// labeled gather/continuation, or a labeled block — at any depth.
///
/// Used by [`try_lift_inline`] to decide lift order: a construct carrying
/// a label must never be CLONED by the lift (one authored label must name
/// exactly one container), so the inline construct containing it lifts
/// first. Inline constructs nested in content parts recurse too — a label
/// can hide inside a branch's own inline conditional.
fn block_contains_label(block: &Block) -> bool {
    block.label.is_some() || block.stmts.iter().any(stmt_contains_label)
}

fn stmt_contains_label(stmt: &Stmt) -> bool {
    match stmt {
        Stmt::ChoiceSet(cs) => {
            cs.continuation.label.is_some()
                || cs
                    .choices
                    .iter()
                    .any(|c| c.label.is_some() || block_contains_label(&c.body))
                || block_contains_label(&cs.continuation)
        }
        Stmt::LabeledBlock(b) => block_contains_label(b),
        Stmt::Conditional(cond) => cond.branches.iter().any(|b| block_contains_label(&b.body)),
        Stmt::Sequence(seq) => seq.branches.iter().any(|b| block_contains_label(&b.body)),
        Stmt::Content(c) => c.parts.iter().any(content_part_contains_label),
        _ => false,
    }
}

fn content_part_contains_label(part: &ContentPart) -> bool {
    match part {
        ContentPart::InlineConditional(cond) => {
            cond.branches.iter().any(|b| block_contains_label(&b.body))
        }
        ContentPart::InlineSequence(seq) => {
            seq.branches.iter().any(|b| block_contains_label(&b.body))
        }
        ContentPart::Span(span) => span.children.iter().any(content_part_contains_label),
        _ => false,
    }
}

// ─── Splice helper ──────────────────────────────────────────────────

/// Append `extra` onto `parts`, merging into the last element when both the
/// last element of `parts` and the next element of `extra` are `Text`
/// (collapsing doubled whitespace at the seam, e.g. `"Hello "` + `" world"`
/// → `"Hello world"`) — otherwise identical to `parts.extend_from_slice`.
///
/// Splicing prefix/branch/suffix content parts (below) used to leave them
/// as separate adjacent `Text` entries — structurally fine, but it meant a
/// spliced branch's recognizer pass (`lir::lower::recognize::try_recognize`,
/// which only matches a *single* `Text` part as `Plain`, or an
/// interpolation-bearing run as `Template`) could never match a spliced
/// line, no matter how plain its text was. Every branch fell back to
/// `EmitContent`, which still emits one line-table entry **per fragment**
/// — the exact "runtime assembles text from parts, translators see shredded
/// fragments" shape the 2026-03-15 ruling (issue #1667) retired. Merging
/// here, at the one place every splice funnels through, is what actually
/// lets the recognizer see one flat line and produce one `LineEntry` per
/// branch — this pass already did the structural half of the ruling (branch
/// lifting + splicing, added the same day as the ruling); merging was the
/// missing half.
fn extend_merging_text(parts: &mut Vec<ContentPart>, extra: &[ContentPart]) {
    for part in extra {
        if let (Some(ContentPart::Text(last)), ContentPart::Text(next)) = (parts.last_mut(), part) {
            if last.ends_with(char::is_whitespace) && next.starts_with(char::is_whitespace) {
                last.push_str(next.trim_start());
            } else {
                last.push_str(next);
            }
        } else {
            parts.push(part.clone());
        }
    }
}

/// The synthesized else branch a no-else lifted conditional gets when
/// prefix/suffix text must still emit on the all-false path. Not sourced
/// from a real arm — falls back to the whole conditional's own span (see
/// `SequenceBranch`'s doc for the same posture). Its id is derived from
/// the last authored branch's (#3275): a `hir::Conditional` has no
/// wrapper id to derive from, and the stamp walk cannot predict this
/// synthesis (it depends on prefix/suffix).
fn synthesized_else_branch(
    cond: &Conditional,
    prefix: &[ContentPart],
    suffix: &[ContentPart],
    tags: &[Tag],
    ptr: Option<crate::Provenance>,
    trailing_eol: bool,
) -> CondBranch {
    let mut else_body = Block::default();
    let (p, s, t) = salted_splice_sources(prefix, suffix, tags, cond.branches.len() as u64);
    splice_around(&mut else_body, &p, &s, &t, ptr);
    if trailing_eol {
        else_body.stmts.push(Stmt::EndOfLine);
    }
    else_body.recompute_tail();
    CondBranch {
        ptr: cond.ptr,
        condition: None,
        binding: None,
        body: else_body,
        container_id: cond
            .branches
            .last()
            .and_then(|b| b.container_id)
            .map(|id| super::stamp::derive_id(id, "synth-else", 0)),
    }
}

/// A cloned stateful alternative keeps its stamped id in every branch
/// (shared visit-count state, ruled 2026-08-29 on #3275) — but that is
/// only sound while every branch's assembled line claims as a variant
/// line, because the variant model emits the shared container as one
/// empty stub (deduped at emission), while an unclaimed line's inline
/// lowering builds a BODIED container per site: one id cannot name both.
/// So the sharing is per-lift-level: if any branch fails to claim
/// immediately, branches 1.. re-derive EVERY id ([`rederive_block_all`]),
/// stateful included — a per-branch state split in this structural-mixed
/// corner, the documented pre-#3275 behavior. The recursive normalize
/// pass then applies the same rule at each deeper lift.
fn revoke_sharing_if_unclaimed<'a>(
    prefix: &[ContentPart],
    suffix: &[ContentPart],
    branches: impl Iterator<Item = &'a mut Block>,
) {
    let has_stateful_clone = prefix
        .iter()
        .chain(suffix.iter())
        .any(|p| matches!(p, ContentPart::InlineSequence(_)));
    if !has_stateful_clone {
        return;
    }
    let mut branches: Vec<&mut Block> = branches.collect();
    let all_claim = branches.iter().all(|b| {
        let ([Stmt::Content(c)] | [Stmt::Content(c), Stmt::EndOfLine]) = b.stmts.as_slice() else {
            return false;
        };
        crate::lir::lower::recognize::claims_variant_line(c)
    });
    if all_claim {
        return;
    }
    for (k, b) in branches.iter_mut().enumerate().skip(1) {
        super::stamp::rederive_block_all(b, k as u64);
    }
}

/// Clone the spliced prefix/suffix/tags for the branch at `salt`,
/// re-deriving cloned container/lambda ids (#3275 stage 3a — see
/// `stamp.rs`'s clone-id section). `salt == 0` keeps the stamped ids: the
/// first branch's clone is the one container each stamped id stays live
/// on. A cloned stateful alternative keeps its id at EVERY salt (shared
/// visit-count state, ruled 2026-08-29); LIR emits that shared container
/// once. Un-stamped parts (in-crate tests that normalize without
/// stamping) pass through unchanged — derivation only rewrites ids that
/// exist.
fn salted_splice_sources(
    prefix: &[ContentPart],
    suffix: &[ContentPart],
    tags: &[Tag],
    salt: u64,
) -> (Vec<ContentPart>, Vec<ContentPart>, Vec<Tag>) {
    let mut p = prefix.to_vec();
    let mut s = suffix.to_vec();
    let mut t = tags.to_vec();
    super::stamp::rederive_cloned_parts(&mut p, salt);
    super::stamp::rederive_cloned_parts(&mut s, salt);
    for tag in &mut t {
        super::stamp::rederive_cloned_parts(&mut tag.parts, salt);
    }
    (p, s, t)
}

/// Splice prefix/suffix text around a branch block's content.
///
/// Handles these cases:
/// - **Single Content stmt**: parts = prefix + original + suffix, merge tags
/// - **Empty block**: create new Content with prefix + suffix
/// - **Multiple stmts, first is Content**: prepend prefix to first Content's parts
/// - **Multiple stmts, last is Content**: append suffix to last Content's parts
/// - **No Content stmts** (e.g., just Divert): insert new Content at position 0
fn splice_around(
    block: &mut Block,
    prefix: &[ContentPart],
    suffix: &[ContentPart],
    tags: &[Tag],
    ptr: Option<crate::Provenance>,
) {
    let has_prefix = !prefix.is_empty();
    let has_suffix = !suffix.is_empty();

    if !has_prefix && !has_suffix && tags.is_empty() {
        return;
    }

    // Empty block — create a new Content with prefix + suffix.
    if block.stmts.is_empty() {
        let mut parts = prefix.to_vec();
        extend_merging_text(&mut parts, suffix);
        if !parts.is_empty() || !tags.is_empty() {
            block.stmts.push(Stmt::Content(Content {
                ptr,
                parts,
                tags: tags.to_vec(),
            }));
        }
        return;
    }

    // Single Content stmt — splice into it directly.
    if block.stmts.len() == 1
        && let Stmt::Content(ref mut c) = block.stmts[0]
    {
        let mut new_parts = prefix.to_vec();
        let original = std::mem::take(&mut c.parts);
        extend_merging_text(&mut new_parts, &original);
        extend_merging_text(&mut new_parts, suffix);
        c.parts = new_parts;
        c.tags.extend_from_slice(tags);
        // The branch's own `ptr` (if any) covers only the branch body's own
        // node — narrower than the whole spliced line once prefix/suffix
        // text is actually merged in (review finding, #3202). When that's
        // happening and the caller handed us a real enclosing-line `ptr`,
        // it is the more correct answer and must win over the branch's own,
        // even though the branch's own is already `Some` (e.g.
        // `wrap_content_as_block`/native's per-branch provenance) — the
        // `c.ptr.is_none()` half of this condition is what covers the
        // no-splice case, where the branch's own body genuinely is the
        // whole line and there is nothing to prefer over it except an
        // actual gap.
        if ((has_prefix || has_suffix) && ptr.is_some()) || c.ptr.is_none() {
            c.ptr = ptr;
        }
        return;
    }

    // Multiple stmts — find first and last Content to splice prefix/suffix.
    let first_content_idx = block
        .stmts
        .iter()
        .position(|s| matches!(s, Stmt::Content(_)));
    let last_content_idx = block
        .stmts
        .iter()
        .rposition(|s| matches!(s, Stmt::Content(_)));

    if let (Some(first), Some(last)) = (first_content_idx, last_content_idx) {
        // Prepend prefix to first Content.
        if has_prefix && let Stmt::Content(ref mut c) = block.stmts[first] {
            let mut new_parts = prefix.to_vec();
            let original = std::mem::take(&mut c.parts);
            extend_merging_text(&mut new_parts, &original);
            c.parts = new_parts;
            c.tags.extend_from_slice(tags);
            // Same enclosing-line-wins rule as the single-Content-stmt case
            // above (review finding, #3202) — `has_prefix` is always true
            // in this branch, so the splice is genuinely happening here.
            if ptr.is_some() || c.ptr.is_none() {
                c.ptr = ptr;
            }
        } else if !tags.is_empty()
            && let Stmt::Content(ref mut c) = block.stmts[first]
        {
            c.tags.extend_from_slice(tags);
        }
        // Append suffix to last Content.
        if has_suffix && let Stmt::Content(ref mut c) = block.stmts[last] {
            extend_merging_text(&mut c.parts, suffix);
        }
    } else {
        // No Content stmts at all — insert a new Content at position 0.
        let mut parts = prefix.to_vec();
        extend_merging_text(&mut parts, suffix);
        if !parts.is_empty() || !tags.is_empty() {
            block.stmts.insert(
                0,
                Stmt::Content(Content {
                    ptr,
                    parts,
                    tags: tags.to_vec(),
                }),
            );
        }
    }
}

#[cfg(test)]
#[expect(clippy::panic)]
mod tests {
    use super::super::types::*;
    use super::normalize_file;

    // ─── Helpers ────────────────────────────────────────────────────

    fn dummy_ptr() -> crate::Provenance {
        crate::Provenance::synthetic(
            crate::provenance::NodeClass::Content,
            rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(6)),
        )
    }

    fn dummy_tag_ptr() -> crate::Provenance {
        crate::Provenance::synthetic(
            crate::provenance::NodeClass::Tag,
            rowan::TextRange::new(rowan::TextSize::new(6), rowan::TextSize::new(10)),
        )
    }

    fn dummy_choice_ptr() -> crate::Provenance {
        crate::Provenance::synthetic(
            crate::provenance::NodeClass::Choice,
            rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(8)),
        )
    }

    fn text(s: &str) -> ContentPart {
        ContentPart::Text(s.to_string())
    }

    fn mk_content(parts: Vec<ContentPart>) -> Content {
        Content {
            ptr: Some(dummy_ptr()),
            parts,
            tags: Vec::new(),
        }
    }

    fn mk_content_with_tags(parts: Vec<ContentPart>, tags: Vec<Tag>) -> Content {
        Content {
            ptr: Some(dummy_ptr()),
            parts,
            tags,
        }
    }

    fn mk_inline_seq(kind: SequenceType, branches: Vec<Vec<ContentPart>>) -> ContentPart {
        let ptr = dummy_ptr();
        ContentPart::InlineSequence(Sequence {
            ptr,
            kind,
            branches: branches
                .into_iter()
                .map(|parts| {
                    let stmts = if parts.is_empty() {
                        Vec::new()
                    } else {
                        vec![Stmt::Content(Content {
                            ptr: Some(ptr),
                            parts,
                            tags: Vec::new(),
                        })]
                    };
                    let tail = crate::tail_from_stmts(&stmts);
                    SequenceBranch {
                        ptr,
                        body: Block {
                            label: None,
                            stmts,
                            container_id: None,
                            tail,
                        },
                    }
                })
                .collect(),
            container_id: None,
        })
    }

    fn mk_inline_cond(branches: Vec<(Option<Expr>, Vec<ContentPart>)>) -> ContentPart {
        let ptr = dummy_ptr();
        ContentPart::InlineConditional(Conditional {
            ptr,
            kind: CondKind::InitialCondition,
            branches: branches
                .into_iter()
                .map(|(condition, parts)| {
                    let stmts = if parts.is_empty() {
                        Vec::new()
                    } else {
                        vec![Stmt::Content(Content {
                            ptr: Some(ptr),
                            parts,
                            tags: Vec::new(),
                        })]
                    };
                    let tail = crate::tail_from_stmts(&stmts);
                    CondBranch {
                        ptr,
                        condition,
                        binding: None,
                        body: Block {
                            label: None,
                            stmts,
                            container_id: None,
                            tail,
                        },
                        container_id: None,
                    }
                })
                .collect(),
        })
    }

    fn mk_tag(s: &str) -> Tag {
        Tag {
            parts: vec![ContentPart::Text(s.to_string())],
            ptr: dummy_tag_ptr(),
        }
    }

    fn mk_block(stmts: Vec<Stmt>) -> Block {
        let tail = crate::tail_from_stmts(&stmts);
        Block {
            label: None,
            stmts,
            container_id: None,
            tail,
        }
    }

    fn mk_hir(stmts: Vec<Stmt>) -> HirFile {
        HirFile {
            root_content: mk_block(stmts),
            knots: Vec::new(),
            variables: Vec::new(),
            constants: Vec::new(),
            lists: Vec::new(),
            structs: Vec::new(),
            externals: Vec::new(),
            includes: Vec::new(),
            module: None,
            imports: Vec::new(),
            visibility: Vec::new(),
            was_directives: Vec::new(),
            allow_scopes: Vec::new(),
            element_matches: Vec::new(),
            cue_names: Vec::new(),
            native: false,
            claim_handlers: Vec::new(),
            dispatch_handlers: Vec::new(),
        }
    }

    /// Extract the text parts from a Content stmt, concatenated.
    fn content_text(content: &Content) -> String {
        content
            .parts
            .iter()
            .filter_map(|p| {
                if let ContentPart::Text(s) = p {
                    Some(s.as_str())
                } else {
                    None
                }
            })
            .collect()
    }

    // ─── Tests ──────────────────────────────────────────────────────

    /// Regression (S1 review F1): an inline-conditional branch whose body is
    /// a bare divert carries `tail == Diverge` at construction. The lift
    /// prepends surrounding text and appends a trailing `EndOfLine`, so the
    /// lifted branch no longer ends in a terminator — its `tail` must flip to
    /// `Unit`. `normalize.rs` runs on cloned HIR right before LIR, so a stale
    /// `tail` here is the closest one to the eventual consumer.
    #[test]
    fn lifted_conditional_branch_with_divert_recomputes_tail() {
        let divert_body = mk_block(vec![Stmt::Divert(Divert {
            ptr: None,
            target: DivertTarget {
                path: DivertPath::End,
                args: Vec::new(),
            },
        })]);
        assert!(
            matches!(divert_body.tail, Tail::Diverge(_)),
            "precondition: a bare-divert body has a Diverge tail"
        );
        let inline_cond = ContentPart::InlineConditional(Conditional {
            ptr: dummy_ptr(),
            kind: CondKind::InitialCondition,
            branches: vec![CondBranch {
                ptr: dummy_ptr(),
                condition: Some(Expr::Bool(true)),
                binding: None,
                body: divert_body,
                container_id: None,
            }],
        });
        // Surrounding text forces the prefix/else-synthesis path; the trailing
        // EndOfLine drives the trailing-eol append inside the lift.
        let content = mk_content(vec![text("A "), inline_cond]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);

        normalize_file(&mut hir);

        let cond = hir
            .root_content
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::Conditional(c) => Some(c),
                _ => None,
            })
            .expect("inline conditional lifted to a Conditional stmt");
        // Every lifted branch body's tail must match its stmts — no stale
        // Diverge left behind after the EndOfLine append.
        for branch in &cond.branches {
            assert_eq!(
                branch.body.tail,
                crate::tail_from_stmts(&branch.body.stmts),
                "lifted branch tail must match its stmts (not stale): {:?}",
                branch.body
            );
        }
    }

    #[test]
    fn simple_sequence_lift() {
        // "It's " + {stopping: "a fine", "a good"} + " day." — plus a
        // trailing Glue: an all-textual stateful line is claimed by the
        // #3274 variant path and no longer lifts, and Glue is one of the
        // shapes the claim refuses, so this keeps exercising the lift.
        let content = mk_content(vec![
            text("It's "),
            mk_inline_seq(
                SequenceType::STOPPING,
                vec![vec![text("a fine")], vec![text("a good")]],
            ),
            text(" day."),
            ContentPart::Glue,
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        // Should be a single Sequence stmt.
        assert_eq!(hir.root_content.stmts.len(), 1);
        let Stmt::Sequence(seq) = &hir.root_content.stmts[0] else {
            panic!("expected Sequence, got {:?}", hir.root_content.stmts[0]);
        };
        assert_eq!(seq.kind, SequenceType::STOPPING);
        assert_eq!(seq.branches.len(), 2);

        // Branch 0: Content("It's a fine day.") + EndOfLine
        assert_eq!(seq.branches[0].body.stmts.len(), 2);
        let Stmt::Content(c0) = &seq.branches[0].body.stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(content_text(c0), "It's a fine day.");
        assert!(matches!(seq.branches[0].body.stmts[1], Stmt::EndOfLine));

        // Branch 1: Content("It's a good day.") + EndOfLine
        let Stmt::Content(c1) = &seq.branches[1].body.stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(content_text(c1), "It's a good day.");
        assert!(matches!(seq.branches[1].body.stmts[1], Stmt::EndOfLine));
    }

    #[test]
    fn simple_conditional_lift() {
        // "I'm " + {happy: "very", "not"} + " pleased."
        let cond_expr = Expr::Bool(true);
        let content = mk_content(vec![
            text("I'm "),
            mk_inline_cond(vec![
                (Some(cond_expr), vec![text("very")]),
                (None, vec![text("not")]),
            ]),
            text(" pleased."),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        assert_eq!(hir.root_content.stmts.len(), 1);
        let Stmt::Conditional(cond) = &hir.root_content.stmts[0] else {
            panic!("expected Conditional");
        };
        assert_eq!(cond.branches.len(), 2);

        let Stmt::Content(c0) = &cond.branches[0].body.stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(content_text(c0), "I'm very pleased.");

        let Stmt::Content(c1) = &cond.branches[1].body.stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(content_text(c1), "I'm not pleased.");
    }

    #[test]
    fn tag_propagation() {
        // Trailing Glue keeps this off the #3274 variant path (see
        // simple_sequence_lift) so tag propagation through the lift stays
        // covered.
        let content = mk_content_with_tags(
            vec![
                text("Hello "),
                mk_inline_seq(
                    SequenceType::CYCLE,
                    vec![vec![text("world")], vec![text("there")]],
                ),
                ContentPart::Glue,
            ],
            vec![mk_tag("greeting")],
        );
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        let Stmt::Sequence(seq) = &hir.root_content.stmts[0] else {
            panic!("expected Sequence");
        };

        // Tags should be on the first content of each branch.
        let Stmt::Content(c0) = &seq.branches[0].body.stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(c0.tags.len(), 1);

        let Stmt::Content(c1) = &seq.branches[1].body.stmts[0] else {
            panic!("expected Content");
        };
        assert_eq!(c1.tags.len(), 1);
    }

    #[test]
    fn eol_absorption() {
        // Without trailing EOL — no EndOfLine in branches.
        // Trailing Glue keeps this off the #3274 variant path (see
        // simple_sequence_lift).
        let content = mk_content(vec![
            text("a "),
            mk_inline_seq(
                SequenceType::STOPPING,
                vec![vec![text("x")], vec![text("y")]],
            ),
            text(" b"),
            ContentPart::Glue,
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content)]);
        normalize_file(&mut hir);

        let Stmt::Sequence(seq) = &hir.root_content.stmts[0] else {
            panic!("expected Sequence");
        };
        // No EndOfLine since there was no trailing EOL.
        assert_eq!(seq.branches[0].body.stmts.len(), 1);
    }

    #[test]
    fn empty_branch_gets_prefix_suffix() {
        // "It's " + {shuffle|once: "a", "", "c"} + " fine" — a combo kind:
        // stage 1's admission routes combos to the lift (their exhaustion
        // logic lives there), so this keeps exercising the splice; the
        // plain-stopping spelling of this line is #3274 variant-claimed
        // and no longer lifts.
        let content = mk_content(vec![
            text("It's "),
            mk_inline_seq(
                SequenceType::SHUFFLE | SequenceType::ONCE,
                vec![vec![text("a")], vec![], vec![text("c")]],
            ),
            text(" fine"),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        let Stmt::Sequence(seq) = &hir.root_content.stmts[0] else {
            panic!("expected Sequence");
        };
        assert_eq!(seq.branches.len(), 3);

        // Branch 1 (empty) should still get prefix+suffix, seam-collapsed to
        // a single space — issue #1667: `extend_merging_text` now merges
        // adjacent `Text` parts at every splice seam (prefix/branch/suffix)
        // so the recognizer sees one flat `Text` part and can match `Plain`.
        // Before that fix, "It's " + "" + " fine" stayed three separate
        // parts (never recognized, always `EmitContent`); at runtime the
        // unrecognized path's `Spring` opcodes collapsed the same double
        // whitespace anyway, so this also matches actual rendered output,
        // not just the new intermediate shape.
        let Stmt::Content(c1) = &seq.branches[1].body.stmts[0] else {
            panic!("expected Content in empty branch");
        };
        assert_eq!(content_text(c1), "It's fine");
        assert_eq!(
            c1.parts.len(),
            1,
            "prefix+suffix should merge into a single Text part so the \
             recognizer can match Plain"
        );
    }

    #[test]
    fn no_inline_passes_through() {
        let content = mk_content(vec![text("Just plain text.")]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        // Should be unchanged: Content + EndOfLine.
        assert_eq!(hir.root_content.stmts.len(), 2);
        assert!(matches!(hir.root_content.stmts[0], Stmt::Content(_)));
        assert!(matches!(hir.root_content.stmts[1], Stmt::EndOfLine));
    }

    #[test]
    fn recursion_into_choice_body() {
        // A choice with an inline conditional in its body (a conditional:
        // inline sequences of this shape are #3274 variant-claimed and no
        // longer lift, and this test is about recursion into the body).
        let body_content = mk_content(vec![
            text("It's "),
            mk_inline_cond(vec![
                (Some(Expr::Bool(true)), vec![text("a")]),
                (None, vec![text("b")]),
            ]),
        ]);
        let choice = Choice {
            ptr: dummy_choice_ptr(),
            is_sticky: false,
            is_fallback: false,
            label: None,
            condition: None,
            binding: None,
            start_content: Some(mk_content(vec![text("Pick")])),
            bracket_content: None,
            inner_content: None,
            tags: Vec::new(),
            body: mk_block(vec![Stmt::Content(body_content), Stmt::EndOfLine]),
            container_id: None,
        };
        let cs = ChoiceSet {
            choices: vec![choice],
            continuation: mk_block(vec![]),
            context: ChoiceSetContext::Weave,
            depth: 1,
            gather_id: None,
        };
        let mut hir = mk_hir(vec![Stmt::ChoiceSet(Box::new(cs))]);
        normalize_file(&mut hir);

        // The choice body should have been normalized.
        let Stmt::ChoiceSet(ref cs) = hir.root_content.stmts[0] else {
            panic!("expected ChoiceSet");
        };
        assert_eq!(cs.choices[0].body.stmts.len(), 1);
        assert!(matches!(cs.choices[0].body.stmts[0], Stmt::Conditional(_)));
    }

    #[test]
    fn recursion_into_conditional_branches() {
        // Inner inline conditional, not a sequence — see
        // recursion_into_choice_body for why.
        let body_content = mk_content(vec![
            text("Hello "),
            mk_inline_cond(vec![
                (Some(Expr::Bool(true)), vec![text("x")]),
                (None, vec![text("y")]),
            ]),
        ]);
        let cond = Conditional {
            ptr: dummy_ptr(),
            kind: CondKind::IfElse,
            branches: vec![CondBranch {
                ptr: dummy_ptr(),
                condition: Some(Expr::Bool(true)),
                binding: None,
                body: mk_block(vec![Stmt::Content(body_content), Stmt::EndOfLine]),
                container_id: None,
            }],
        };
        let mut hir = mk_hir(vec![Stmt::Conditional(cond)]);
        normalize_file(&mut hir);

        let Stmt::Conditional(ref c) = hir.root_content.stmts[0] else {
            panic!("expected Conditional");
        };
        // The branch body should have been normalized — a lifted
        // Conditional instead of Content+EOL.
        assert_eq!(c.branches[0].body.stmts.len(), 1);
        assert!(matches!(c.branches[0].body.stmts[0], Stmt::Conditional(_)));
    }

    /// #3274 (stage-2 flip): a line the variant model claims — every
    /// inline alternative plain-kinded and textual — is NOT lifted. The
    /// cartesian lift is exactly what gave each spliced clone of the
    /// second alternative its own visit count (#3271); the un-lifted line
    /// reaches LIR whole, where enumeration compiles it over SHARED
    /// alternative containers.
    #[test]
    fn variant_claimed_line_is_not_lifted() {
        let content = mk_content(vec![
            text("Line: "),
            mk_inline_seq(
                SequenceType::STOPPING,
                vec![vec![text("a")], vec![text("b")]],
            ),
            text(" "),
            mk_inline_seq(
                SequenceType::STOPPING,
                vec![vec![text("x")], vec![text("y")]],
            ),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        assert_eq!(
            hir.root_content.stmts.len(),
            2,
            "claimed line passes through whole: {:?}",
            hir.root_content.stmts
        );
        assert!(matches!(hir.root_content.stmts[0], Stmt::Content(_)));
        assert!(matches!(hir.root_content.stmts[1], Stmt::EndOfLine));
    }

    /// A `shuffle|once` combination is NOT claimed (stage 1's admission
    /// routes combos to the fallback where their exhaustion logic lives) —
    /// the lift must still run for it.
    #[test]
    fn combo_kind_line_still_lifts() {
        let content = mk_content(vec![
            text("Line: "),
            mk_inline_seq(
                SequenceType::SHUFFLE | SequenceType::ONCE,
                vec![vec![text("a")], vec![text("b")]],
            ),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);
        assert!(
            matches!(hir.root_content.stmts[0], Stmt::Sequence(_)),
            "combo kinds keep the lift: {:?}",
            hir.root_content.stmts[0]
        );
    }

    /// #3272: an inline conditional whose branch carries a LABELED choice
    /// lifts FIRST, whatever its position on the line — whichever
    /// construct lifts first is the one that is not cloned, and cloning a
    /// labeled construct stamps one label onto two containers (the E060
    /// internal error on legal-looking source).
    #[test]
    fn label_bearing_conditional_lifts_first() {
        fn count_labeled(block: &Block) -> usize {
            block
                .stmts
                .iter()
                .map(|s| match s {
                    Stmt::ChoiceSet(cs) => {
                        cs.choices
                            .iter()
                            .map(|c| usize::from(c.label.is_some()) + count_labeled(&c.body))
                            .sum::<usize>()
                            + count_labeled(&cs.continuation)
                    }
                    Stmt::LabeledBlock(b) => count_labeled(b),
                    Stmt::Conditional(c) => c.branches.iter().map(|b| count_labeled(&b.body)).sum(),
                    Stmt::Sequence(sq) => sq.branches.iter().map(|b| count_labeled(&b.body)).sum(),
                    _ => 0,
                })
                .sum()
        }

        let labeled_choice = Choice {
            ptr: dummy_choice_ptr(),
            is_sticky: false,
            is_fallback: false,
            label: Some(Name {
                text: "dup".to_string(),
                range: rowan::TextRange::new(rowan::TextSize::new(0), rowan::TextSize::new(3)),
            }),
            condition: None,
            binding: None,
            start_content: Some(mk_content(vec![text("Pick me")])),
            bracket_content: None,
            inner_content: None,
            tags: Vec::new(),
            body: mk_block(vec![]),
            container_id: None,
        };
        let cs = ChoiceSet {
            choices: vec![labeled_choice],
            continuation: mk_block(vec![]),
            context: ChoiceSetContext::Weave,
            depth: 1,
            gather_id: None,
        };
        let cond_body = mk_block(vec![Stmt::ChoiceSet(Box::new(cs))]);
        let tail = crate::tail_from_stmts(&cond_body.stmts);
        let inline_cond = ContentPart::InlineConditional(Conditional {
            ptr: dummy_ptr(),
            kind: CondKind::InitialCondition,
            branches: vec![CondBranch {
                ptr: dummy_ptr(),
                condition: Some(Expr::Bool(true)),
                binding: None,
                body: Block {
                    label: None,
                    stmts: cond_body.stmts,
                    container_id: None,
                    tail,
                },
                container_id: None,
            }],
        });
        // The shuffle alternative comes FIRST in part order — the old
        // first-construct rule would lift it and clone the labeled
        // conditional into both branches.
        let content = mk_content(vec![
            text("Pre "),
            mk_inline_seq(
                SequenceType::SHUFFLE,
                vec![vec![text("one")], vec![text("two")]],
            ),
            text(" mid "),
            inline_cond,
            text(" post."),
        ]);
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);
        normalize_file(&mut hir);

        let Stmt::Conditional(cond) = &hir.root_content.stmts[0] else {
            panic!(
                "label-bearing conditional must lift first, got {:?}",
                hir.root_content.stmts[0]
            );
        };
        // The labeled choice exists exactly once across the whole tree.
        let total: usize = cond.branches.iter().map(|b| count_labeled(&b.body)).sum();
        assert_eq!(total, 1, "the labeled choice must not be cloned");
    }

    /// Regression (review finding, #3202): the enclosing line's own `ptr`
    /// must win over a lifted branch's own (narrower) `ptr` once
    /// prefix/suffix text is actually spliced in — a real branch-node `ptr`
    /// is not proof the branch already covers the whole line.
    ///
    /// Before this fix, `splice_around`'s `if c.ptr.is_none() { c.ptr = ptr }`
    /// only ever filled in a location when the branch itself had none. Once
    /// callers started stamping a real (but narrower) branch-node `ptr`
    /// (`wrap_content_as_block`/native's per-branch provenance, both fixed
    /// for #3181), that fallback stopped firing — so a lifted line like
    /// `"Ready {h: high|low} now."` kept only the branch's own sub-range
    /// (e.g. just `" high"`) instead of the whole line's byte-exact span,
    /// even though the whole-line `ptr` was sitting right there, passed to
    /// `splice_around` and ignored.
    fn range(lo: u32, hi: u32) -> rowan::TextRange {
        rowan::TextRange::new(rowan::TextSize::new(lo), rowan::TextSize::new(hi))
    }

    #[test]
    fn spliced_branch_takes_enclosing_line_location_over_its_own_narrower_one() {
        // Whole line "Ready {h: high|low} now." spans 0..25; the branch's
        // own inline-conditional-body node ("high") spans only 8..12 —
        // deliberately narrower and disjoint-looking from the enclosing
        // span's numbers, so a test failure can't be mistaken for the two
        // ranges coincidentally matching.
        let enclosing_ptr =
            crate::Provenance::synthetic(crate::provenance::NodeClass::Content, range(0, 25));
        let branch_ptr =
            crate::Provenance::synthetic(crate::provenance::NodeClass::Content, range(8, 12));

        let branch_body_stmts = vec![Stmt::Content(Content {
            ptr: Some(branch_ptr),
            parts: vec![text("high")],
            tags: Vec::new(),
        })];
        let tail = crate::tail_from_stmts(&branch_body_stmts);
        let inline_cond = ContentPart::InlineConditional(Conditional {
            ptr: dummy_ptr(),
            kind: CondKind::InitialCondition,
            branches: vec![CondBranch {
                ptr: dummy_ptr(),
                condition: Some(Expr::Bool(true)),
                binding: None,
                body: Block {
                    label: None,
                    stmts: branch_body_stmts,
                    container_id: None,
                    tail,
                },
                container_id: None,
            }],
        });
        let content = Content {
            ptr: Some(enclosing_ptr),
            parts: vec![text("Ready "), inline_cond, text(" now.")],
            tags: Vec::new(),
        };
        let mut hir = mk_hir(vec![Stmt::Content(content), Stmt::EndOfLine]);

        normalize_file(&mut hir);

        let Stmt::Conditional(cond) = &hir.root_content.stmts[0] else {
            panic!("expected Conditional, got {:?}", hir.root_content.stmts[0]);
        };
        let Stmt::Content(spliced) = &cond.branches[0].body.stmts[0] else {
            panic!(
                "expected spliced Content, got {:?}",
                cond.branches[0].body.stmts[0]
            );
        };
        assert_eq!(content_text(spliced), "Ready high now.");
        assert_eq!(
            spliced.ptr,
            Some(enclosing_ptr),
            "spliced branch must carry the whole line's location, not its own narrower one: {:?}",
            spliced.ptr
        );
    }
}
