//! E194 — a knot's `~ temp` (native `~ let`) read from one of that knot's
//! stitches (issue #3373, RULED 2026-09-01) — the compat-deny tier's first
//! member, split out of `E193`'s former shape 4 during PR #3369's review.
//!
//! # Why this is not `E193`
//!
//! `brink_ir::lir::lower::temps::alloc_temps` treats a knot and every one
//! of its stitches as one shared call frame with one `TempMap`, so a
//! stitch reading a name the knot's root declares resolves to that same
//! slot — the program compiles and plays. PR #3369's review found this
//! pass (then `temp_dominance`'s shape 4) warning on exactly that: a
//! knot/stitch divert that runs the declaration first and then plays
//! correctly (`-> k`, `~ temp n = 7`, `-> s`, `= s`, `Stitch sees {n}.`
//! plays `Stitch sees 7.`, no defect). That is not a dominance question —
//! ink's own compiler simply never extends a knot's `~ temp` visibility
//! into its stitches at all, regardless of dominance or path: the
//! identical program is a compile-time `Unresolved variable` error in
//! inklecate. Brink accepting it is a genuine superset, which is exactly
//! what the compat-deny tier (`docs/compiler-spec.md` "Compat-deny
//! diagnostics") exists for.
//!
//! # What fires
//!
//! Every bare-name read inside a stitch's body that:
//!
//! 1. is not a parameter of that stitch,
//! 2. is not shadowed by a `~ temp`/`~ let` the stitch declares itself
//!    (that is `E193`'s question — whether the stitch's own declaration
//!    dominates its own reads — not this one), and
//! 3. names a `~ temp`/`~ let` declared somewhere in the knot's own root
//!    body (anywhere in its block tree — a top-level statement or nested
//!    inside a choice/conditional/sequence/labeled branch),
//!
//! fires regardless of whether the divert that entered the stitch ran the
//! knot root's declaration first or skipped it entirely — both are
//! equally rejected by inklecate, so both are equally flagged here.

use brink_ir::hir::visit;
use brink_ir::{Diagnostic, DiagnosticCode, FileId, HirFile, Knot};

use crate::determinism::{LookupMap, LookupSet};
use crate::temp_dominance::{DeclSite, ReadCollector, collect_decls};

/// Run the E194 check over one file.
pub fn check(file: FileId, hir: &HirFile, is_native: bool) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for knot in &hir.knots {
        if knot.stitches.is_empty() {
            continue;
        }
        check_knot(file, knot, is_native, &mut out);
    }
    out
}

fn check_knot(file: FileId, knot: &Knot, is_native: bool, out: &mut Vec<Diagnostic>) {
    let knot_noun = if is_native { "flow" } else { "knot" };
    let decl_keyword = if is_native { "let" } else { "temp" };
    let knot_owner = format!("{knot_noun} `{}`", knot.name.text);

    // The knot's own root-level declarations — never a stitch's. Reuses
    // `temp_dominance::collect_decls` so "what counts as a declaration in
    // this block" can never drift between the two passes.
    let mut knot_decls: LookupMap<String, DeclSite> = LookupMap::new();
    collect_decls(&knot.body, &knot_owner, &mut knot_decls);
    if knot_decls.is_empty() {
        return;
    }

    for stitch in &knot.stitches {
        let stitch_owner = format!("stitch `{}.{}`", knot.name.text, stitch.name.text);
        // A stitch that declares this name itself shadows the knot's — its
        // own reads are `E193`'s question (does the stitch's own
        // declaration dominate them?), not this one.
        let mut stitch_decls: LookupMap<String, DeclSite> = LookupMap::new();
        collect_decls(&stitch.body, &stitch_owner, &mut stitch_decls);

        let stitch_params: Vec<&str> = stitch.params.iter().map(|p| p.name.text.as_str()).collect();

        let mut reads = Vec::new();
        let mut skipped = LookupSet::new();
        let mut v = ReadCollector {
            reads: &mut reads,
            skipped: &mut skipped,
            lambda_depth: 0,
        };
        visit::walk_block(&stitch.body, &mut v);

        for read in &reads {
            if skipped.contains(&read.range) {
                continue;
            }
            if stitch_params.contains(&read.name.as_str()) {
                continue;
            }
            if stitch_decls.contains_key(&read.name) {
                continue;
            }
            if !knot_decls.contains_key(&read.name) {
                continue;
            }
            let name = &read.name;
            out.push(Diagnostic {
                file,
                range: read.range,
                message: format!(
                    "{}: `{name}` is read here, but the `~ {decl_keyword} {name}` that \
                     declares it belongs to {knot_owner}'s own root — ink does not consider \
                     a {knot_noun}'s `~ {decl_keyword}` visible from its stitches, so this \
                     compiles and plays here but inklecate rejects it (`Unresolved variable: \
                     {name}`)",
                    DiagnosticCode::E194.title(),
                ),
                code: DiagnosticCode::E194,
            });
        }
    }
}
