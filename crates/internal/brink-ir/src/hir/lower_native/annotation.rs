//! The per-declaration `@[name(args)]` annotation channel for the native
//! `.brink` surface (issue #1563).
//!
//! `brink-syntax-native` has parsed `@[…]` since B0.5 (`parser/annotation.rs`,
//! the ruled Rust-meta-item paren-clause grammar), but until this slice the
//! only annotation any native lowering *consumed* was the file-level
//! `@[was("old::path")]` module-rename record ([`super::module`]). Every other
//! annotation — `@[effects(pure)]` above a `fn` most of all — reached
//! [`super::walk_top_level`]'s catch-all and hard-failed the compile with
//! `E129` ("parses but has no HIR lowering yet"). This module closes that gap
//! for the **ruled** tenant.
//!
//! # What is ruled, and what is not
//!
//! Three annotation names have a ruled native meaning today:
//!
//! - **`effects`** — `@[effects(pure, silent, total, reads(a), writes(b),
//!   calls(c))]`, the assertion final form (`docs/effects-spec.md` §10,
//!   `docs/directive-annotations-spec.md` §5b; clause grammar amended to the
//!   paren shape 2026-07-19, issue #1120). B0.6's own scope line names
//!   `effects_assertion` as one of the directive channels native lowering
//!   owes (`docs/b0-sequencing.md` §B0.6). **This module delivers it.**
//! - **`was`** — the file-level module-rename record, already delivered by
//!   [`super::module`] (issue #1286/#1355). This module only keeps it out of
//!   the unknown-name error and rejects it at non-file-level positions.
//! - **`allow`** — `@[allow(E151, E014)]`, source-level diagnostic
//!   suppression scoped to the annotated declaration (issue #1161; the
//!   issue's own framing: "the `@[…]` directive namespace … is the ruled
//!   home for suppression"). Warning-tier codes only; see
//!   [`allow_scopes`] for the three ways it is rejected and
//!   [`crate::suppressions`] for the filter and the
//!   source-`allow`-beats-project-`deny` ordering. **This module delivers
//!   it.**
//!
//! Everything else the specs mention is either *deferred* or *not yet ruled
//! for a native declaration*, and is deliberately NOT invented here:
//! `@[element(pattern)]` / `@[style(…)]` (ruled in principle by the prose
//! sitting-4 addenda, but they are the prose-dispatch feature itself, not a
//! lowering channel), a per-*declaration* `@[was(old_name)]` rename (ink's
//! `#@was` on a knot, `docs/modules-spec.md` §5 — no native ruling, and it
//! would need the alias-table half too), and `directive-annotations-spec.md`
//! §6's remaining non-normative future tenants (`@world`, `@returns`,
//! `@notranslate`, `@maxlen`, `@deprecated`, …). All of them land
//! on `E111`, which is the reserved-namespace rule (§1.1) working as
//! designed: an unknown annotation is loud, never a silent no-op.
//!
//! `is_local` and per-declaration `visibility` stay unwired for a different
//! reason — they are *keyword* channels on the native surface (no
//! `KW_LOCAL`/`KW_PUB` token exists yet), not annotation names, so there is
//! nothing for this module to read.
//!
//! # Placement
//!
//! Native's placement is the Rust one and follows from the grammar: an
//! annotation line attaches to the declaration it immediately precedes,
//! with only trivia and further annotation/doc lines allowed to intervene.
//! (This is *not* ink's placement — ink's `@[effects]` rides the leading run
//! at the top of a knot/stitch **body**, because an ink tag line above a knot
//! header structurally belongs to the previous scope. Native has real
//! declaration nodes, so the annotation sits above the head.)
//!
//! Recognition is split the same way `hir::lower::directive` splits it: the
//! **owner** ([`super::container`]) calls [`effects_assertion`] to read its
//! own attached run, and every walk that can encounter an annotation line
//! ([`super::walk_top_level`], [`super::body::lower_one_item`]) calls
//! [`handle_line`] so a misplaced or unknown annotation is diagnosed exactly
//! once and never silently dropped.

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use rowan::TextRange;

use crate::hir::FileId;
use crate::suppressions::AllowScope;
use crate::{Diagnostic, DiagnosticCode, EffectsAssertion, Severity};

use super::SyntaxNode;

/// The effects-assertion annotation name.
const EFFECTS: &str = "effects";

/// The module-rename annotation name — consumed by [`super::module`] at file
/// level; named here only so it is not reported as an unknown name.
const WAS: &str = "was";

/// The source-level diagnostic-suppression annotation name (issue #1161).
const ALLOW: &str = "allow";

fn diag(file: FileId, range: TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

/// The contiguous run of `@[…]` annotation lines immediately above `decl`, in
/// source order.
///
/// Trivia (whitespace, newlines, comments) and `///` doc-comment nodes may
/// intervene; any other node ends the run. Blank lines do **not** break
/// attachment, matching Rust's attribute attachment (the native surface's
/// declared north star) rather than ink's stricter tag-line adjacency.
fn annotations_before(decl: &SyntaxNode) -> Vec<ast::AnnotationLine> {
    let mut collected = Vec::new();
    let mut cursor = decl.prev_sibling_or_token();
    while let Some(el) = cursor {
        match &el {
            rowan::NodeOrToken::Token(tok) => {
                if !tok.kind().is_trivia() && tok.kind() != N::NEWLINE {
                    break;
                }
            }
            rowan::NodeOrToken::Node(node) => match node.kind() {
                N::ANNOTATION_LINE => {
                    // Walking backwards — prepend to keep source order.
                    if let Some(line) = ast::AnnotationLine::cast(node.clone()) {
                        collected.insert(0, line);
                    }
                }
                N::DOC_COMMENT => {}
                _ => break,
            },
        }
        cursor = el.prev_sibling_or_token();
    }
    collected
}

/// The declaration an annotation line attaches to: the next non-trivia
/// sibling, skipping further annotation and doc-comment lines.
///
/// `None` when nothing follows (a trailing annotation at end of file or end
/// of a body) — the misplacement [`handle_line`] reports.
fn attached_declaration(line: &SyntaxNode) -> Option<SyntaxNode> {
    let mut cursor = line.next_sibling_or_token();
    while let Some(el) = cursor {
        match &el {
            rowan::NodeOrToken::Token(tok) => {
                if !tok.kind().is_trivia() && tok.kind() != N::NEWLINE {
                    return None;
                }
            }
            rowan::NodeOrToken::Node(node) => match node.kind() {
                N::ANNOTATION_LINE | N::DOC_COMMENT => {}
                _ => return Some(node.clone()),
            },
        }
        cursor = el.next_sibling_or_token();
    }
    None
}

/// The number of enclosing `flow`/`fn` containers between `decl` and the
/// file root. Native has no HIR "module" container node (judgment call #4)
/// — a `MODULE_DECL`'s body is walked flat by `walk_top_level`, so a
/// `MODULE_DECL` ancestor does not count as a nesting level here, only
/// `FLOW_DECL`/`FN_DECL` ones do.
///
/// `0` is top-level (lowers to a `Knot` via
/// `container::lower_top_level_container`); `1` is a `flow` nested exactly
/// one level inside another's body (lowers to a `Stitch` via
/// `container::lower_stitch`); `2` or deeper is the depth-3 fence
/// (`DiagnosticCode::E130`) — no container lowers it.
fn container_nesting_depth(decl: &SyntaxNode) -> usize {
    let mut depth = 0;
    let mut cursor = decl.parent();
    while let Some(node) = cursor {
        if matches!(node.kind(), N::FLOW_DECL | N::FN_DECL) {
            depth += 1;
        }
        cursor = node.parent();
    }
    depth
}

/// `true` when `line` sits in a placement some lowering pass consumes, so
/// [`handle_line`] must not report it misplaced.
///
/// This must track exactly what `container.rs` actually lowers, not merely
/// "attached to a `flow`/`fn` head" — a nested `fn` (no HIR container below
/// `Knot` carries `is_function`, `container.rs`'s E129 fence) and a `flow`
/// nested three levels deep (the E130 depth fence) are never lowered, so an
/// `@[effects(…)]` attached to either of them must be diagnosed here too,
/// not waved through as "consumed" only to be read by nothing.
fn is_consumed_position(name: &str, line: &SyntaxNode) -> bool {
    match name {
        // The module-rename record is a *file-level* fact — `module::
        // lower_file_module` scans `SOURCE_FILE`'s own children for it.
        WAS => line.parent().is_some_and(|p| p.kind() == N::SOURCE_FILE),
        // The effects assertion attaches to a callable container that
        // `container.rs` actually lowers.
        EFFECTS => attached_declaration(line).is_some_and(|d| {
            let depth = container_nesting_depth(&d);
            match d.kind() {
                // A `flow` lowers at top level (→ `Knot`) and nested exactly
                // one level (→ `Stitch`); depth-3+ is the E130 fence.
                N::FLOW_DECL => depth <= 1,
                // A `fn` only ever lowers at top level; any nested `fn` is
                // the E129 fence — there is nowhere for it to go.
                N::FN_DECL => depth == 0,
                _ => false,
            }
        }),
        // A suppression scope is the annotated declaration's own span, so
        // any declaration this annotation can sit above is a consumed
        // placement — unlike `effects`, nothing downstream has to be able
        // to *lower* the target for the scope to be meaningful (an
        // `@[allow]` over a construct that itself only produces `E129` is
        // still a well-formed scope; it just cannot silence that error).
        // Only a trailing annotation with nothing after it is misplaced.
        ALLOW => attached_declaration(line).is_some(),
        _ => false,
    }
}

/// The erasure chokepoint: called for every `ANNOTATION_LINE` a body or
/// top-level walk encounters, so an annotation is *either* consumed by its
/// owner *or* diagnosed here — never lowered to content, never dropped.
///
/// An unrecognized name is `E111` (the reserved-`@`-namespace rule,
/// `docs/directive-annotations-spec.md` §1.1); a recognized name outside its
/// placement is `E112`.
pub(super) fn handle_line(file_id: FileId, node: &SyntaxNode, diags: &mut Vec<Diagnostic>) {
    let Some(name) = ast::AnnotationLine::cast(node.clone()).and_then(|l| l.name_token()) else {
        // The parser already reported the malformed `@[…]` itself.
        return;
    };
    let range = node.text_range();
    if !matches!(name.text(), EFFECTS | WAS | ALLOW) {
        diags.push(diag(file_id, range, DiagnosticCode::E111));
    } else if !is_consumed_position(name.text(), node) {
        diags.push(diag(file_id, range, DiagnosticCode::E112));
    }
}

/// Collect every `@[allow(Exxx, …)]` suppression scope in the file (issue
/// #1161), diagnosing the malformed ones.
///
/// A whole-tree walk rather than an owner lookback, because the *owner* of a
/// suppression scope is not a HIR node at all — the scope is a `(span,
/// codes)` fact about the file that [`crate::suppressions::apply_suppressions`]
/// consumes, and it must be collected identically whether the annotation sits
/// above a top-level `fn`, a nested `flow`, or a `var` inside a body. The
/// placement/erasure half still runs through [`handle_line`] like every other
/// annotation, so a *misplaced* `@[allow]` is `E112` there and is skipped
/// here (no double report, no scope recorded for it).
///
/// Three ways an `@[allow]` fails, all hard errors so a suppression that does
/// nothing can never be silent (the `@`-namespace rule,
/// `docs/directive-annotations-spec.md` §1.1):
///
/// - `E155` — no argument list, an empty one, or an argument that is not a
///   bare identifier (a string, a nested clause);
/// - `E153` — an identifier that is not a known diagnostic code (a typo);
/// - `E154` — a known code whose default severity is `Error`, which is not
///   suppressible at any tier.
///
/// A line with any of these produces no scope at all: partial silencing off a
/// broken directive would be worse than none.
pub(super) fn allow_scopes(
    file_id: FileId,
    root: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Vec<AllowScope> {
    let mut out = Vec::new();
    for node in root.descendants() {
        if node.kind() != N::ANNOTATION_LINE {
            continue;
        }
        let Some(line) = ast::AnnotationLine::cast(node.clone()) else {
            continue;
        };
        if line.name_token().is_none_or(|t| t.text() != ALLOW) {
            continue;
        }
        // Misplaced — `handle_line` already reported `E112`.
        let Some(target) = attached_declaration(&node) else {
            continue;
        };
        if let Some(codes) = parse_allow(file_id, &line, node.text_range(), diags) {
            out.push(AllowScope {
                range: target.text_range(),
                codes,
            });
        }
    }
    out
}

/// Parse and validate one `@[allow(…)]` line's argument list into the codes
/// it silences. `None` (with a diagnostic pushed) when the line is malformed
/// in any of the three ways [`allow_scopes`] documents.
fn parse_allow(
    file_id: FileId,
    line: &ast::AnnotationLine,
    range: TextRange,
    diags: &mut Vec<Diagnostic>,
) -> Option<Vec<DiagnosticCode>> {
    let Some(args) = line.args() else {
        diags.push(diag(file_id, range, DiagnosticCode::E155));
        return None;
    };

    let mut codes = Vec::new();
    let mut ok = true;
    let mut any = false;
    for arg in args.args() {
        any = true;
        // A bare identifier and nothing else: no nested clause, no literal.
        let name = match arg.name_token() {
            Some(t) if arg.nested_args().is_none() => t,
            _ => {
                diags.push(diag(file_id, range, DiagnosticCode::E155));
                ok = false;
                continue;
            }
        };
        let Some(code) = DiagnosticCode::from_str_code(name.text()) else {
            diags.push(diag(file_id, name.text_range(), DiagnosticCode::E153));
            ok = false;
            continue;
        };
        if code.severity() == Severity::Error {
            diags.push(diag(file_id, name.text_range(), DiagnosticCode::E154));
            ok = false;
            continue;
        }
        codes.push(code);
    }

    if !any {
        // `@[allow()]` — parses, silences nothing.
        diags.push(diag(file_id, range, DiagnosticCode::E155));
        return None;
    }
    ok.then_some(codes)
}

/// Read the `@[effects(…)]` assertion attached to `decl` (a `flow`/`fn`
/// declaration node), if it declares one.
///
/// A second `@[effects]` on the same declaration is `E048` and the first
/// wins — the duplicate-directive discipline the ink channel already uses.
/// Grammar failures diagnose per [`parse_effects`] and yield `None`.
/// Unknown annotation names in the run are *not* reported here; the walk's
/// [`handle_line`] chokepoint owns that so it fires exactly once.
pub(super) fn effects_assertion(
    file_id: FileId,
    decl: &SyntaxNode,
    diags: &mut Vec<Diagnostic>,
) -> Option<EffectsAssertion> {
    let mut chosen: Option<EffectsAssertion> = None;
    for line in annotations_before(decl) {
        if line.name_token().is_none_or(|t| t.text() != EFFECTS) {
            continue;
        }
        let range = line.syntax().text_range();
        let Some(parsed) = parse_effects(file_id, &line, range, diags) else {
            continue; // already diagnosed
        };
        if chosen.is_some() {
            diags.push(diag(file_id, range, DiagnosticCode::E048));
            continue;
        }
        chosen = Some(parsed);
    }
    chosen
}

/// Parse one `@[effects(…)]` line's arguments off the CST.
///
/// The native grammar already produced the structure the ink recognizer has
/// to rebuild from tag text (`ANNOTATION_ARG` per item, a nested
/// `ANNOTATION_ARGS` per clause), so this walks nodes rather than re-parsing
/// a string — but the *rules* are the ruled ones, identical to
/// `hir::lower::directive::parse_effects_paren_clauses`:
///
/// - a bare top-level identifier is ALWAYS a flag — exactly `pure`,
///   `silent`, or `total`; anything else is `E101`;
/// - a parenthesized clause is `reads(…)`/`writes(…)`/`calls(…)`, each
///   naming zero or more plain identifiers; any other clause name, or a
///   non-identifier inside one, is `E101`;
/// - `pure` asserts the *empty* state row, so combining it with a clause
///   that grants state atoms is contradictory (`E101`), not a union;
/// - a missing, empty, or wholly vacuous argument list is `E100`.
///
/// Resolving the declared names against the project symbol index, and the
/// exceedance check itself, stay `brink-analyzer`'s job
/// (`effects_assertions::check`) — this only parses the annotation's grammar.
fn parse_effects(
    file_id: FileId,
    line: &ast::AnnotationLine,
    range: TextRange,
    diags: &mut Vec<Diagnostic>,
) -> Option<EffectsAssertion> {
    let mut pure = false;
    let mut silent = false;
    let mut total = false;
    let mut reads = Vec::new();
    let mut writes = Vec::new();
    let mut calls = Vec::new();
    let mut ok = true;

    let Some(args) = line.args() else {
        // Bare `@[effects]` — no argument list at all.
        diags.push(diag(file_id, range, DiagnosticCode::E100));
        return None;
    };

    for arg in args.args() {
        if let Some(nested) = arg.nested_args() {
            let target = match arg.name_token().as_ref().map(|t| t.text().to_string()) {
                Some(n) if n == "reads" => &mut reads,
                Some(n) if n == "writes" => &mut writes,
                Some(n) if n == "calls" => &mut calls,
                // An unknown clause name — flags take no parentheses.
                _ => {
                    ok = false;
                    continue;
                }
            };
            for inner in nested.args() {
                // A clause names plain cells: no nested clause, no literal,
                // no `::`-path.
                match inner.name_token() {
                    Some(t) if inner.nested_args().is_none() => target.push(t.text().to_string()),
                    _ => ok = false,
                }
            }
        } else if let Some(name) = arg.name_token() {
            // Bare top-level ident: ALWAYS a flag — the structural rule the
            // paren grammar exists to guarantee.
            match name.text() {
                "pure" => pure = true,
                "silent" => silent = true,
                "total" => total = true,
                _ => ok = false,
            }
        } else {
            // A literal or a `::`-path in flag position.
            ok = false;
        }
    }

    if pure && !(reads.is_empty() && writes.is_empty() && calls.is_empty()) {
        ok = false;
    }
    if !ok {
        diags.push(diag(file_id, range, DiagnosticCode::E101));
        return None;
    }
    if !pure && !silent && !total && reads.is_empty() && writes.is_empty() && calls.is_empty() {
        // `@[effects()]` — parsed, but asserts nothing.
        diags.push(diag(file_id, range, DiagnosticCode::E100));
        return None;
    }
    Some(EffectsAssertion {
        pure,
        silent,
        total,
        reads,
        writes,
        calls,
        range,
    })
}
