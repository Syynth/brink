//! `deleteSymbol` — remove a knot or stitch region, reporting the breakage (#316).
//!
//! Deleting a knot removes its whole region (header, body, and every nested
//! stitch); deleting a stitch removes just that stitch's region. Both extend
//! backward over the declaration's attached `///` doc block, mirroring the
//! structural-move ownership rules. The op runs the op-agnostic breakage gate:
//! every divert / thread / tunnel / function call that targeted the removed
//! symbol now dangles, and those introduced diagnostics travel out so the studio
//! can show a safe-by-default report before the delete is applied.

use brink_syntax::ast::{AstNode as _, KnotDef};

use crate::doc_extended_start;
use crate::session::IdeSession;
use crate::structural_move::MoveError;
use crate::structural_result::{StructuralResult, gate_with_source};

/// A declaration's region start: extended backward over its attached `///` doc
/// block, so docs are removed with the declaration they document.
fn decl_region_start(source: &str, node: &brink_syntax::SyntaxNode) -> usize {
    doc_extended_start(source, node.text_range().start().into())
}

/// The byte offset where a knot's region ends: the next knot's ownership start
/// (its doc block, if any) or EOF.
fn knot_region_end(source: &str, knots: &[KnotDef], ki: usize) -> usize {
    if ki + 1 < knots.len() {
        decl_region_start(source, knots[ki + 1].syntax())
    } else {
        source.len()
    }
}

/// Delete a knot (`stitch` = `None`) or a stitch within a knot, returning the
/// rewritten primary source plus the breakage the delete introduces.
///
/// `path` is the primary file's project-relative path; `knot`/`stitch` name the
/// region to remove. The op is pure — it computes the new source and gates the
/// project for introduced diagnostics; the session is not mutated.
///
/// # Errors
/// [`MoveError::SourceNotFound`] if the knot doesn't exist;
/// [`MoveError::StitchNotFound`] if the named stitch doesn't exist in it.
pub fn delete_symbol(
    session: &IdeSession,
    path: &str,
    knot: &str,
    stitch: Option<&str>,
) -> Result<StructuralResult, MoveError> {
    let file_id = session.file_id(path).ok_or(MoveError::SourceNotFound)?;
    let source = session.source(file_id).ok_or(MoveError::SourceNotFound)?;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let knots: Vec<_> = tree.knots().collect();

    let (ki, knot_node) = knots
        .iter()
        .enumerate()
        .find(|(_, k)| k.header().and_then(|h| h.name()).as_deref() == Some(knot))
        .ok_or(MoveError::SourceNotFound)?;

    let (del_start, del_end) = match stitch {
        // Knot delete: the whole region, including every nested stitch.
        None => (
            decl_region_start(source, knot_node.syntax()),
            knot_region_end(source, &knots, ki),
        ),
        // Stitch delete: just that stitch's ownership region, clamped to the
        // knot region (the stitch's region runs to the next stitch's ownership
        // start, or to the end of the knot for the last stitch).
        Some(stitch_name) => {
            let body = knot_node.body().ok_or(MoveError::StitchNotFound {
                name: stitch_name.to_owned(),
            })?;
            let stitches: Vec<_> = body.stitches().collect();
            let si = stitches
                .iter()
                .position(|s| s.header().and_then(|h| h.name()).as_deref() == Some(stitch_name))
                .ok_or(MoveError::StitchNotFound {
                    name: stitch_name.to_owned(),
                })?;
            let region_end = knot_region_end(source, &knots, ki);
            let start = decl_region_start(source, stitches[si].syntax());
            let end = if si + 1 < stitches.len() {
                decl_region_start(source, stitches[si + 1].syntax())
            } else {
                usize::from(stitches[si].syntax().text_range().end()).min(region_end)
            };
            (start, end)
        }
    };

    let mut new_source = String::with_capacity(source.len() - (del_end - del_start));
    new_source.push_str(&source[..del_start]);
    new_source.push_str(&source[del_end..]);

    // Breakage gate (#316): every reference that resolved to the removed symbol
    // now dangles. Overlay the rewritten primary source and re-analyze.
    let introduced = gate_with_source(session, path, &new_source, &[]);

    Ok(StructuralResult {
        new_source: Some(new_source),
        cross_file_edits: Vec::new(),
        safe: introduced.is_empty(),
        introduced,
    })
}

#[cfg(test)]
mod tests {
    use super::delete_symbol;
    use crate::session::IdeSession;

    fn session(files: &[(&str, &str)]) -> IdeSession {
        let mut s = IdeSession::new();
        for (path, src) in files {
            s.update_and_analyze(path, (*src).to_owned());
        }
        s
    }

    #[test]
    fn delete_unreferenced_knot_is_safe() {
        let s = session(&[("main.ink", "=== a ===\n-> END\n=== b ===\n-> END\n")]);
        let r = delete_symbol(&s, "main.ink", "b", None).unwrap();
        let out = r.new_source.as_deref().unwrap();
        assert!(!out.contains("=== b ==="), "b removed: {out}");
        assert!(out.contains("=== a ==="), "a kept: {out}");
        assert!(
            r.safe,
            "no references, so no breakage: {:?}",
            introduced(&r)
        );
        assert!(r.introduced.is_empty());
    }

    #[test]
    fn delete_referenced_knot_reports_dangling_refs() {
        // `start` diverts to `target`; deleting `target` makes that divert dangle.
        let s = session(&[(
            "main.ink",
            "=== start ===\n-> target\n=== target ===\n-> END\n",
        )]);
        let r = delete_symbol(&s, "main.ink", "target", None).unwrap();
        assert!(!r.safe, "deleting a referenced knot is not safe");
        assert!(
            !r.introduced.is_empty(),
            "the dangling divert is reported: {:?}",
            introduced(&r)
        );
    }

    #[test]
    fn delete_referenced_knot_reports_cross_file_dangle() {
        let s = session(&[
            ("main.ink", "INCLUDE lib.ink\n-> helper\n-> END\n"),
            ("lib.ink", "=== helper ===\n-> END\n"),
        ]);
        let r = delete_symbol(&s, "lib.ink", "helper", None).unwrap();
        assert!(!r.safe, "a cross-file reference still dangles");
        assert!(
            !r.introduced.is_empty(),
            "cross-file dangle reported: {:?}",
            introduced(&r)
        );
    }

    #[test]
    fn delete_stitch_keeps_siblings() {
        let s = session(&[(
            "main.ink",
            "=== k ===\n= alpha\nA.\n= beta\nB.\n= gamma\nG.\n",
        )]);
        let r = delete_symbol(&s, "main.ink", "k", Some("beta")).unwrap();
        let out = r.new_source.as_deref().unwrap();
        assert!(!out.contains("= beta"), "beta removed: {out}");
        assert!(out.contains("= alpha") && out.contains("= gamma"), "{out}");
    }

    #[test]
    fn delete_knot_removes_nested_stitches() {
        let s = session(&[(
            "main.ink",
            "=== outer ===\nIntro.\n= inner\nNested.\n=== other ===\n-> END\n",
        )]);
        let r = delete_symbol(&s, "main.ink", "outer", None).unwrap();
        let out = r.new_source.as_deref().unwrap();
        assert!(!out.contains("=== outer ==="), "knot removed: {out}");
        assert!(!out.contains("= inner"), "nested stitch removed too: {out}");
        assert!(out.contains("=== other ==="), "neighbor kept: {out}");
    }

    #[test]
    fn delete_unknown_symbol_errors() {
        let s = session(&[("main.ink", "=== a ===\n-> END\n")]);
        assert!(delete_symbol(&s, "main.ink", "ghost", None).is_err());
        assert!(delete_symbol(&s, "main.ink", "a", Some("ghost")).is_err());
    }

    fn introduced(r: &crate::structural_result::StructuralResult) -> Vec<&str> {
        r.introduced.iter().map(|d| d.code.as_str()).collect()
    }
}
