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
//! Six annotation names have a ruled native meaning today:
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
//! - **`element`** — `@[element(args = "…", block)]`, the `!name`-dispatch
//!   pattern declaration (issue #1719, `docs/prose-dialect-spec.md` §3.5b
//!   sitting 4 addenda 2–3). **This module delivers the declaration
//!   surface** — [`element_annotation`] parses the `args`/`name` clauses,
//!   validates the pattern compiles as a portable regex (`E159`), and
//!   validates its named captures against the declaration's own params
//!   (`E160`, the spec's "compile-checked" capture contract). Also parses
//!   the bare `block` clause (issue #1839, `docs/decision-log.md`
//!   2026-07-31 "Conventions are annotated handlers"): `@[element(args =
//!   "…", block)]` declares that the handler captures the run *following*
//!   its matched line into a trailing `content`-typed param, and this
//!   module validates that receiver exists (`E166`) — see
//!   [`ElementAnnotation::block`]'s doc. The `!name` sigil dispatch
//!   rewrite the annotation exists to drive now dispatches (issue #2004,
//!   `super::element::try_dispatch`) for the plain, non-`block` case on a
//!   top-level `fn` — matching a content line, binding captures, and
//!   lowering to a call; a `block`-declared handler's trailing receiver
//!   still has nothing dispatching into it (that binding is issue #1839's
//!   own scope, not this one's — see [`ElementAnnotation`]'s own doc).
//!   Self-announcing and legal anywhere: `@[element]` carries **no
//!   `order`** — a handler that names itself never competes for a line.
//! - **`convention`** — `@[convention(claims = "…", order = N)]`, the
//!   natural-notation pattern-claiming declaration (issue #1838, split out
//!   of `@[element]` by issue #2164's 2026-08-03 ruling, "Claiming and
//!   `!name` dispatch split into two annotations"). **This module
//!   delivers it** — [`convention_annotation`] parses the `claims`/`order`
//!   clauses and the bare `block` clause, validates the pattern compiles
//!   as a portable regex (`E159`), validates named captures against
//!   params in both directions (`E160` *and* `E167`: params ≡ captures,
//!   since every argument of the rewritten call comes from a capture),
//!   and requires `order` (`E178` if absent — there is no default,
//!   because a claiming handler's precedence against every sibling in its
//!   module must be total and authored, never inferred). Legal only above
//!   a top-level `fn` (`E112` otherwise — see [`is_consumed_position`]):
//!   the rewrite is an expression call, and a claiming pattern can take a
//!   line that looks like ordinary prose, so it stays **confined** to the
//!   `brink.toml`-named conventions module (issue #1844's `E169`, a
//!   project-layer check this module doesn't perform). The dispatch
//!   itself, and the `order`-based precedence walk, live in
//!   [`super::element`].
//! - **`style`** — `@[style(…)]`, the companion editor-presentation
//!   annotation (same spec section, addenda 3–4). **This module delivers
//!   it** as a pure declaration surface — [`style_annotation`] requires a
//!   paired `element` or `convention` on the same declaration (`E163`),
//!   parses `key = "value"` clauses (`E161`), validates each key against
//!   the paired annotation's captures plus `line`/`dispatch` (`E162`), and
//!   classifies each value against the closed built-in presentation
//!   vocabulary ([`crate::StyleToken`]). Nothing downstream reads it yet —
//!   the consumer is the held editor track (NS-T, issues #1131/#1350); see
//!   [`crate::StyleAnnotation`]'s doc.
//!
//! Everything else the specs mention is either *deferred* or *not yet ruled
//! for a native declaration*, and is deliberately NOT invented here: a
//! per-*declaration* `@[was(old_name)]` rename (ink's `#@was` on a knot,
//! `docs/modules-spec.md` §5 — no native ruling, and it would need the
//! alias-table half too), and `directive-annotations-spec.md` §6's
//! remaining non-normative future tenants (`@world`, `@returns`,
//! `@notranslate`, `@maxlen`, `@deprecated`, …). All of them land
//! on `E111`, which is the reserved-namespace rule (§1.1) working as
//! designed: an unknown annotation is loud, never a silent no-op.
//!
//! `is_local` stays unwired for a different reason — it is a *keyword*
//! channel on the native surface (no `KW_LOCAL` token exists yet), not an
//! annotation name, so there is nothing for this module to read.
//! Per-declaration `visibility` is no longer in that category: issue #1582
//! (RULED 2026-08-03) gave native its own `pub` keyword, wired in
//! `decl::visibility_mark`/`container::lower_top_level_container` — also a
//! keyword channel, and also not something this module reads, but it is
//! not unwired anymore.
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
use crate::{
    ConventionAnnotation, Diagnostic, DiagnosticCode, EffectsAssertion, ElementAnnotation, Name,
    Param, Severity, StyleAnnotation, StyleEntry, StyleToken, TypeExpr,
};

use super::SyntaxNode;

/// The effects-assertion annotation name.
const EFFECTS: &str = "effects";

/// The module-rename annotation name — consumed by [`super::module`] at file
/// level; named here only so it is not reported as an unknown name.
const WAS: &str = "was";

/// The source-level diagnostic-suppression annotation name (issue #1161).
const ALLOW: &str = "allow";

/// The `!name`-dispatch pattern-declaration annotation name (issue #1719,
/// `docs/prose-dialect-spec.md` §3.5b). Since issue #2164's 2026-08-03
/// split, this name declares dispatch only (`args = "…"`) — the claiming
/// half moved to [`CONVENTION`].
const ELEMENT: &str = "element";

/// The pattern-claiming annotation name (issue #2164, `docs/decision-
/// log.md` 2026-08-03 "Claiming and `!name` dispatch split into two
/// annotations: `@[convention]` and `@[element]`") — `@[convention(claims
/// = "…", order = N)]`. Carries what `@[element(claims = "…")]` used to
/// (issue #1838), plus the now-required `order` clause.
const CONVENTION: &str = "convention";

/// The editor-presentation companion annotation name (issue #1719, same
/// spec section, addenda 3–4).
const STYLE: &str = "style";

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
        // `@[convention(claims = "…", order = N)]` (issue #2164, formerly
        // `@[element(claims = "…")]`, issue #1838) is narrower than
        // `@[element]`'s own placement: the rewrite is an expression call,
        // and only a top-level `fn` is callable as one. A claim on a
        // `flow`, or on a nested `fn`, would parse and validate and then
        // claim nothing — exactly the silent no-op the `@`-namespace rule
        // exists to prevent — so it is reported misplaced (`E112`)
        // instead.
        //
        // "Top-level" here means exactly what [`super::element::collect`]
        // scans: a direct child of the file's `SOURCE_FILE` node, not
        // merely `container_nesting_depth == 0`. A `fn` declared inside a
        // `module { … }` block also has depth `0` — a `MODULE_DECL`
        // ancestor is not a `FLOW_DECL`/`FN_DECL`, so it does not count as
        // a nesting level (see `container_nesting_depth`'s own doc) — but
        // `collect` only iterates `root.children()`, so a claim nested in
        // a module would validate here and then never be registered as a
        // handler at all: a silent drop (issue #1847). `walk_top_level`
        // already recurses into a module's body and lowers its `fn`s to
        // ordinary `Knot`s (`module_block_is_flagged_and_flattened`), so
        // this is reachable *today*, not only once module lowering "lands"
        // — it is misplaced now, until claiming inside a module is a
        // designed, registered mechanism of its own.
        CONVENTION => attached_declaration(line).is_some_and(|d| {
            d.kind() == N::FN_DECL && d.parent().is_some_and(|p| p.kind() == N::SOURCE_FILE)
        }),
        // The module-rename record is a *file-level* fact — `module::
        // lower_file_module` scans `SOURCE_FILE`'s own children for it.
        WAS => line.parent().is_some_and(|p| p.kind() == N::SOURCE_FILE),
        // The effects assertion, and the `element`/`style` declaration-
        // surface annotations (issue #1719), attach to a callable container
        // that `container.rs` actually lowers — same consumed-position rule
        // for all three.
        EFFECTS | ELEMENT | STYLE => attached_declaration(line).is_some_and(|d| {
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
    if !matches!(
        name.text(),
        EFFECTS | WAS | ALLOW | ELEMENT | CONVENTION | STYLE
    ) {
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

// ─── `@[element]` / `@[convention]` / `@[style]` declaration surface
//     (issue #1719, split by issue #2164) ────────────────────────────────
//
// The `!name` sigil dispatch rewrite `@[element]` exists to declare
// dispatches now for the plain (non-`block`) case (issue #2004,
// `super::element::try_dispatch`); the claiming-precedence walk
// `@[convention]`'s `order` drives lives in the same module. This file
// remains only the declaration surface (parse, validate, store on the
// `Knot`/`Stitch`); the dispatch/claiming mechanism itself lives in
// `super::element`. See this module's doc comment for the exact boundary.

/// The `@[element(…)]` clause keys: `args = "…"` (the `!name`-dispatched
/// remainder pattern) is required; `name = "…"` is the optional
/// dispatch-name alias.
const ELEMENT_ARGS: &str = "args";
const ELEMENT_NAME: &str = "name";

/// The `@[convention(…)]` clause keys (issue #2164, split out of
/// `@[element(claims = "…")]`): `claims = "…"` (the natural-notation
/// whole-line claim) and `order = N` (the claiming precedence, `docs/
/// decision-log.md` 2026-08-03 "`order` is REQUIRED on `@[convention]`")
/// are both required.
const CONVENTION_CLAIMS: &str = "claims";
const CONVENTION_ORDER: &str = "order";

/// The `@[convention(…)]` optional `attach = StructName` clause (issue
/// #2178, split from #2164's backport comment "item 2") — the handler's
/// declared output schema. See [`ConventionAnnotation::attach`]'s own doc.
const CONVENTION_ATTACH: &str = "attach";

/// The `@[element(…)]` / `@[convention(…)]` shared bare `block` clause
/// (issue #1839, `docs/decision-log.md` 2026-07-31 "Conventions are
/// annotated handlers"): a flag, not a `key = "value"` pair — present or
/// absent, never assigned. See [`ElementAnnotation::block`]'s /
/// [`ConventionAnnotation::block`]'s doc for what it declares.
const ELEMENT_BLOCK: &str = "block";

/// The native-type-system spelling `@[element(…, block)]`'s trailing
/// capture parameter must carry (`text: content`, per the ruled example).
/// Checked as plain annotation text here, at the declaration surface —
/// this module never calls into the type checker, so this stays a shallow
/// text check even though `content` is now a resolvable native leaf
/// (`Ty::Content`, issue #1846, `crate::infer::resolve`) — same
/// "declared, not resolved" posture the rest of this file's `element`/
/// `style` surface already takes for every other type name. Only the
/// *type's* resolvability landed with #1846; the `!name` sigil dispatch
/// rewrite that would actually bind a captured run to this param (finding
/// the block's terminator, building the `FragmentRef`, calling the
/// handler) is still issue #1839's scope, not delivered here.
const ELEMENT_CONTENT_TYPE: &str = "content";

/// The native-type-system spelling a `claims = "…"` handler's captured
/// param can declare and still type-check under the rewrite (`E171`,
/// issue #1849) — `try_claim` binds every capture as a plain
/// `Expr::String` literal, so `string` is the one declared type a
/// captured param is actually, literally handed today (`content` also
/// passes the same check, for a different reason — see
/// [`is_satisfiable_by_a_string_capture`]'s own doc). Same shallow-text-
/// check posture as [`ELEMENT_CONTENT_TYPE`]'s own doc: this module never
/// calls into the type checker.
const ELEMENT_STRING_TYPE: &str = "string";

/// The two `@[style(…)]` keys that name the whole line rather than a
/// capture: the dispatched line's full text, and the `!name` prefix itself
/// (§3.5b addendum 4).
const STYLE_KEY_LINE: &str = "line";
const STYLE_KEY_DISPATCH: &str = "dispatch";

/// Extract an `ANNOTATION_ARG`'s `key = "value"` clause value as plain text
/// — the key/value counterpart of `module::string_lit_text` (the
/// `@[was("…")]` string-literal reader), widened to also fold
/// `L_BRACKET`/`R_BRACKET` tokens back into the text.
/// `brink-syntax-native`'s `parser::annotation::annotation_string_value`
/// deliberately doesn't break on those the way a dialogue-quoted `expr::
/// string_lit` does — see that function's doc for why a regex character
/// class (`@[element(args = "…[A-Z]…")]`) needs this. `None` when the arg
/// has no `eq_value` at all (a bare ident, a nested clause, or a
/// non-string right-hand side).
fn eq_value_text(arg: &ast::AnnotationArg) -> Option<String> {
    let value = arg.eq_value()?;
    let mut out = String::new();
    for el in value.syntax().children_with_tokens() {
        if let rowan::NodeOrToken::Token(t) = el {
            match t.kind() {
                N::STRING_TEXT | N::L_BRACKET | N::R_BRACKET => out.push_str(t.text()),
                N::STRING_ESCAPE => out.push_str(super::expr::unescape_string_token(t.text())),
                _ => {}
            }
        }
    }
    Some(out)
}

/// Read the `@[element(args = "…", block)]` annotation attached to `decl`
/// (a `flow`/`fn` declaration node), if it declares one.
///
/// A second `@[element]` on the same declaration is `E048` (the same
/// duplicate-directive discipline [`effects_assertion`] uses) and the
/// first wins. A malformed clause — a missing/non-string `args` value, an
/// unrecognized clause key, or a pattern that doesn't compile as a
/// portable regex — is `E159` and yields no `ElementAnnotation` at all
/// (never a partial one). A named capture group that doesn't match any
/// parameter on `decl` is `E160` — the capture contract (§3.5b: "named
/// captures bind params by name (compile-checked)") enforced at the
/// declaration. A bare `block` clause (issue #1839) with no qualifying
/// trailing `content`-typed parameter on `decl` is `E166` — the same
/// static-defect-in-the-declaration posture `E160` already takes, widened
/// to the block capture contract.
///
/// `!name` dispatch carries no `order` (issue #2164) and no `E167`/`E171`
/// converse checks — those are [`convention_annotation`]'s alone, since a
/// self-announcing handler stays callable by hand with ordinary arguments
/// and never competes for a line.
pub(super) fn element_annotation(
    file_id: FileId,
    decl: &SyntaxNode,
    params: &[Param],
    diags: &mut Vec<Diagnostic>,
) -> Option<ElementAnnotation> {
    let mut chosen: Option<ElementAnnotation> = None;
    for line in annotations_before(decl) {
        if line.name_token().is_none_or(|t| t.text() != ELEMENT) {
            continue;
        }
        let range = line.syntax().text_range();
        let Some(parsed) = parse_element(file_id, &line, range, params, diags) else {
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

fn parse_element(
    file_id: FileId,
    line: &ast::AnnotationLine,
    range: TextRange,
    params: &[Param],
    diags: &mut Vec<Diagnostic>,
) -> Option<ElementAnnotation> {
    let Some(args) = line.args() else {
        diags.push(diag(file_id, range, DiagnosticCode::E159));
        return None;
    };

    let mut pattern: Option<String> = None;
    let mut alias: Option<String> = None;
    let mut block = false;
    let mut ok = true;
    for arg in args.args() {
        let Some(key) = arg.name_token() else {
            ok = false;
            continue;
        };
        if key.text() == ELEMENT_BLOCK {
            // A flag, not a `key = "value"` pair: no `eq_value`, no nested
            // clause. `!block` guards the duplicate-`block` case the same
            // way `pattern.is_none()`/`alias.is_none()` guard theirs below.
            if !block && arg.eq_value().is_none() && arg.nested_args().is_none() {
                block = true;
            } else {
                ok = false;
            }
            continue;
        }
        let Some(value) = eq_value_text(&arg) else {
            ok = false;
            continue;
        };
        match key.text() {
            ELEMENT_ARGS if pattern.is_none() => pattern = Some(value),
            ELEMENT_NAME if alias.is_none() => alias = Some(value),
            _ => ok = false,
        }
    }

    if !ok {
        diags.push(diag(file_id, range, DiagnosticCode::E159));
        return None;
    }
    let Some(pattern) = pattern else {
        diags.push(diag(file_id, range, DiagnosticCode::E159));
        return None;
    };

    let Ok(compiled) = regex::Regex::new(&pattern) else {
        diags.push(diag(file_id, range, DiagnosticCode::E159));
        return None;
    };
    let captures: Vec<String> = compiled
        .capture_names()
        .flatten()
        .map(ToString::to_string)
        .collect();

    for cap in &captures {
        if !params.iter().any(|p| &p.name.text == cap) {
            diags.push(diag(file_id, range, DiagnosticCode::E160));
            return None;
        }
    }

    if block && !has_block_content_param(params, &captures) {
        diags.push(diag(file_id, range, DiagnosticCode::E166));
        return None;
    }

    Some(ElementAnnotation {
        pattern,
        captures,
        alias,
        block,
        range,
    })
}

/// Read the `@[convention(claims = "…", order = N)]` annotation attached
/// to `decl` (a top-level `fn` declaration node — placement legality is
/// [`is_consumed_position`]'s job, not this function's), if it declares
/// one.
///
/// A second `@[convention]` on the same declaration is `E048` (the same
/// duplicate-directive discipline [`effects_assertion`] uses) and the
/// first wins. A malformed clause — a missing/non-string `claims` value,
/// a missing/non-integer `order` value, an unrecognized clause key, or a
/// pattern that doesn't compile as a portable regex — is `E159` and
/// yields no `ConventionAnnotation` at all (never a partial one). A
/// missing `order` clause specifically is `E178` (issue #2164,
/// `docs/decision-log.md` 2026-08-03) — required, with no default. A
/// named capture group that doesn't match any parameter on `decl` is
/// `E160` — the capture contract (§3.5b: "named captures bind params by
/// name (compile-checked)") enforced at the declaration. A bare `block`
/// clause (issue #1839) with no qualifying trailing `content`-typed
/// parameter on `decl` is `E166` — the same static-defect-in-the-
/// declaration posture `E160` already takes, widened to the block
/// capture contract. A pattern naming a parameter no capture matches is
/// the converse check, `E167` — the rewritten call has no other source of
/// arguments, so every parameter must come from a capture. A captured
/// param declaring a type the rewrite could never satisfy (not `string`,
/// not untyped, not `content`) is `E171` (issue #1849) — the rewrite
/// binds every capture as a plain string literal, so any other declared
/// type could never actually be satisfied; `content` is exempted, see
/// [`is_satisfiable_by_a_string_capture`]'s own doc for why.
///
/// Duplicate `order` values across the module's declared handlers
/// (`E179`) are **not** checked here — only one declaration is in view at
/// a time; that check runs over the whole collected set in
/// `super::element::diagnose_duplicate_order`.
///
/// An optional `attach = StructName` clause (issue #2178) whose named
/// struct disagrees with `return_type` — the declaration's own resolved
/// return-type annotation, if any — is `E180`, the same "never a partial
/// one" posture. `return_type` is passed in (rather than re-read from
/// `decl`) because the caller has already lowered it once
/// ([`super::container::lower_top_level_container`]/`lower_stitch`);
/// re-parsing it here would duplicate that walk for a value this
/// function only ever compares by name.
pub(super) fn convention_annotation(
    file_id: FileId,
    decl: &SyntaxNode,
    params: &[Param],
    return_type: Option<&TypeExpr>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ConventionAnnotation> {
    let mut chosen: Option<ConventionAnnotation> = None;
    for line in annotations_before(decl) {
        if line.name_token().is_none_or(|t| t.text() != CONVENTION) {
            continue;
        }
        let range = line.syntax().text_range();
        let Some(parsed) = parse_convention(file_id, &line, range, params, return_type, diags)
        else {
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

/// The raw clause values one `@[convention(…)]` line's arguments parse to
/// — [`parse_convention`]'s own arg-parsing loop, factored out to keep
/// that function under clippy's line budget. `None` from
/// [`parse_convention_clauses`] means a malformed clause (an unrecognized
/// key, a wrong-shaped value, or a repeated clause) — `E159`, the same
/// "malformed annotation" catch-all every other shape here reports.
struct ConventionClauses {
    pattern: Option<String>,
    order: Option<i64>,
    attach: Option<Name>,
    block: bool,
}

fn parse_convention_clauses(args: &ast::AnnotationArgs) -> Option<ConventionClauses> {
    let mut pattern: Option<String> = None;
    let mut order: Option<i64> = None;
    let mut attach: Option<Name> = None;
    let mut block = false;
    let mut ok = true;
    for arg in args.args() {
        let Some(key) = arg.name_token() else {
            ok = false;
            continue;
        };
        if key.text() == ELEMENT_BLOCK {
            // A flag, not a `key = "value"` pair — see `parse_element`'s
            // identical guard.
            if !block && arg.eq_value().is_none() && arg.nested_args().is_none() {
                block = true;
            } else {
                ok = false;
            }
            continue;
        }
        if key.text() == CONVENTION_ORDER {
            // `order = N` is a bare-integer clause (issue #2164, RULED), not
            // a string one — `eq_int_value` reads it, never `eq_value_text`.
            match (order.is_none(), arg.eq_int_value().and_then(|v| v.value())) {
                (true, Some(n)) => order = Some(n),
                _ => ok = false,
            }
            continue;
        }
        if key.text() == CONVENTION_ATTACH {
            // `attach = StructName` is a bare-identifier clause (issue
            // #2178) — the attached schema names a declared `struct`, not
            // a quoted string or a bare integer, so this reads
            // `eq_ident_value`, never `eq_value_text`/`eq_int_value`.
            match (attach.is_none(), arg.eq_ident_value()) {
                (true, Some(tok)) => {
                    attach = Some(Name {
                        text: tok.text().to_string(),
                        range: tok.text_range(),
                    });
                }
                _ => ok = false,
            }
            continue;
        }
        let Some(value) = eq_value_text(&arg) else {
            ok = false;
            continue;
        };
        match key.text() {
            CONVENTION_CLAIMS if pattern.is_none() => pattern = Some(value),
            _ => ok = false,
        }
    }

    ok.then_some(ConventionClauses {
        pattern,
        order,
        attach,
        block,
    })
}

fn parse_convention(
    file_id: FileId,
    line: &ast::AnnotationLine,
    range: TextRange,
    params: &[Param],
    return_type: Option<&TypeExpr>,
    diags: &mut Vec<Diagnostic>,
) -> Option<ConventionAnnotation> {
    let Some(args) = line.args() else {
        diags.push(diag(file_id, range, DiagnosticCode::E159));
        return None;
    };

    let Some(ConventionClauses {
        pattern,
        order,
        attach,
        block,
    }) = parse_convention_clauses(&args)
    else {
        diags.push(diag(file_id, range, DiagnosticCode::E159));
        return None;
    };
    let Some(pattern) = pattern else {
        diags.push(diag(file_id, range, DiagnosticCode::E159));
        return None;
    };
    // `order` is required (issue #2164, RULED 2026-08-03): no default, so a
    // `@[convention]` with none is `E178` rather than silently falling back
    // to declaration position the way the pre-#2164 interim rule did.
    let Some(order) = order else {
        diags.push(diag(file_id, range, DiagnosticCode::E178));
        return None;
    };

    let Ok(compiled) = regex::Regex::new(&pattern) else {
        diags.push(diag(file_id, range, DiagnosticCode::E159));
        return None;
    };
    let captures: Vec<String> = compiled
        .capture_names()
        .flatten()
        .map(ToString::to_string)
        .collect();

    for cap in &captures {
        if !params.iter().any(|p| &p.name.text == cap) {
            diags.push(diag(file_id, range, DiagnosticCode::E160));
            return None;
        }
    }

    if block && !has_block_content_param(params, &captures) {
        diags.push(diag(file_id, range, DiagnosticCode::E166));
        return None;
    }

    // The converse check (`E167`, issue #1838): a claimed line is rewritten
    // to exactly one call whose every argument comes from a named capture,
    // so a parameter no capture names has nothing to bind it to.
    //
    // A `block`-declared handler's trailing `content` param (issue #1839)
    // is exempt, the same way: `has_block_content_param`'s own check just
    // above already guarantees it is the last param and is not one of
    // `captures` by construction (it binds the captured *run*, not a named
    // group), so re-checking it here would reject every legal block
    // declaration with a false `E167`.
    let e167_checked_params = if block {
        &params[..params.len().saturating_sub(1)]
    } else {
        params
    };
    if let Some(p) = e167_checked_params
        .iter()
        .find(|p| !captures.contains(&p.name.text))
    {
        diags.push(diag(file_id, p.name.range, DiagnosticCode::E167));
        return None;
    }

    // Issue #1849: a captured param's declared type must be satisfiable by
    // a plain string capture — `super::element::try_claim` binds every
    // capture as an `Expr::String` literal, unconditionally, regardless of
    // the receiving parameter's declared type (see that function's own
    // doc). Numeric capture coercion is `docs/prose-dialect-spec.md`
    // §3.5b's own Deferred item — the gap itself is ruled-deferred, not a
    // bug — but leaving the mismatch silent is: without this check it is,
    // and remains, silent — nothing checks a direct call's arguments
    // against the callee's declared parameter types yet (that generic
    // check, `E063` for this shape, is exactly what open issue #1864 asks
    // to build). Checking it here, at the declaration, both narrows the
    // span to the offending param's own type annotation and explains *why*
    // (deferred coercion, not a defect) — the same static-defect-in-the-
    // declaration posture `E160`/`E166`/`E167` already take just above.
    // Declining the claim (`None`, so this `fn` is never registered as a
    // claiming handler) leaves the line unclaimed rather than rewriting
    // it to a call with an argument that could never match its declared
    // type.
    if let Some(p) = params.iter().find(|p| {
        captures.contains(&p.name.text)
            && !is_satisfiable_by_a_string_capture(p.annotation.as_ref())
    }) {
        let range = p.annotation.as_ref().map_or(p.name.range, TypeExpr::range);
        diags.push(diag(file_id, range, DiagnosticCode::E171));
        return None;
    }

    // `attach = StructName` (issue #2178): the declaration's own resolved
    // return-type annotation must name the same struct — a shallow,
    // by-name comparison, the same "declaration surface only, never the
    // type checker" posture this whole module takes (see
    // `has_block_content_param`'s own doc). No return type at all, or one
    // that names something else (`int`, a different struct, a generic, a
    // fn type), is `E180`: the handler's actual output could never carry
    // the declared schema.
    if let Some(attach) = &attach {
        let matches = matches!(
            return_type,
            Some(TypeExpr::Named { name, .. }) if name == &attach.text
        );
        if !matches {
            diags.push(diag(file_id, attach.range, DiagnosticCode::E180));
            return None;
        }
    }

    Some(ConventionAnnotation {
        pattern,
        order,
        captures,
        block,
        attach,
        range,
    })
}

/// `true` when `params` ends with a `content`-typed parameter that is not
/// one of `captures` — the structural shape `block` requires (`E166`
/// otherwise). Declaration-surface-only, like the rest of this module: this
/// checks the raw `TypeExpr` text, not a resolved type, because this module
/// never calls into the type checker at all (not because `content` itself
/// is unresolvable — see [`ELEMENT_CONTENT_TYPE`]'s doc).
fn has_block_content_param(params: &[Param], captures: &[String]) -> bool {
    let Some(last) = params.last() else {
        return false;
    };
    let is_content_typed = matches!(
        &last.annotation,
        Some(TypeExpr::Named { name, .. }) if name == ELEMENT_CONTENT_TYPE
    );
    is_content_typed && !captures.contains(&last.name.text)
}

/// `true` when a `claims = "…"` handler's captured param, declared with
/// `annotation`, can accept the plain string literal [`super::element::
/// try_claim`] actually binds every capture as.
///
/// Absent (no declared type — the param takes whatever the rewrite gives
/// it, matching pre-typed-mode behavior) and exactly `string` both pass,
/// unsurprisingly. `content` **also** passes, deliberately — not because
/// a capture can actually produce a `FragmentRef` (it cannot; binding a
/// `content`-typed param to one is `docs/prose-dialect-spec.md` §3.5b's
/// own Deferred item, issue #1838/#1839's scope, not this check's), but
/// because `content` is the spec-ruled capture annotation form (§3.5b,
/// issue #1846/#1839): the spec's own worked example
/// (`fn radio(chan: string, text: content)`) and this crate's
/// `tier1-native/annotations-element` golden fixture both declare a
/// captured `content` param today and both compile clean. Flagging it
/// here would turn an already-shipped, spec-ruled pattern into a fresh
/// hard error. Every *other* declared type (`int`, `float`, `bool`, a
/// struct name, a generic, a `fn` type) has no such ruling — those are
/// `E171`'s actual target; see that code's own doc comment.
fn is_satisfiable_by_a_string_capture(annotation: Option<&TypeExpr>) -> bool {
    match annotation {
        None => true,
        Some(TypeExpr::Named { name, .. }) => {
            name == ELEMENT_STRING_TYPE || name == ELEMENT_CONTENT_TYPE
        }
        Some(_) => false,
    }
}

/// Read the `@[style(…)]` annotation attached to `decl`, if it declares
/// one. Requires a paired [`element_annotation`] OR [`convention_annotation`]
/// on the same declaration (`E163`) — pass whichever was already lowered
/// (or both `None`) so this doesn't re-parse the `@[element(…)]`/
/// `@[convention(…)]` line itself.
///
/// A second `@[style]` on the same declaration is `E048`. A clause that
/// isn't a clean `key = "value"` pair, or an argument list that is
/// missing/empty, is `E161`. A key that is neither `line`, `dispatch`,
/// nor one of the paired annotation's named captures is `E162`. When a
/// declaration carries **both** (unusual, but not itself an error — see
/// [`crate::Knot::convention_annotation`]'s doc), the two capture sets are
/// combined so either's names validate.
pub(super) fn style_annotation(
    file_id: FileId,
    decl: &SyntaxNode,
    element: Option<&ElementAnnotation>,
    convention: Option<&ConventionAnnotation>,
    diags: &mut Vec<Diagnostic>,
) -> Option<StyleAnnotation> {
    let mut chosen: Option<StyleAnnotation> = None;
    for line in annotations_before(decl) {
        if line.name_token().is_none_or(|t| t.text() != STYLE) {
            continue;
        }
        let range = line.syntax().text_range();
        let Some(parsed) = parse_style(file_id, &line, range, element, convention, diags) else {
            continue;
        };
        if chosen.is_some() {
            diags.push(diag(file_id, range, DiagnosticCode::E048));
            continue;
        }
        chosen = Some(parsed);
    }
    chosen
}

fn parse_style(
    file_id: FileId,
    line: &ast::AnnotationLine,
    range: TextRange,
    element: Option<&ElementAnnotation>,
    convention: Option<&ConventionAnnotation>,
    diags: &mut Vec<Diagnostic>,
) -> Option<StyleAnnotation> {
    if element.is_none() && convention.is_none() {
        diags.push(diag(file_id, range, DiagnosticCode::E163));
        return None;
    }
    let captures: Vec<&str> = element
        .iter()
        .flat_map(|e| e.captures.iter().map(String::as_str))
        .chain(
            convention
                .iter()
                .flat_map(|c| c.captures.iter().map(String::as_str)),
        )
        .collect();

    let Some(args) = line.args() else {
        diags.push(diag(file_id, range, DiagnosticCode::E161));
        return None;
    };

    let mut entries = Vec::new();
    let mut ok = true;
    let mut any = false;
    for arg in args.args() {
        any = true;
        let arg_range = arg.syntax().text_range();
        let key = arg.name_token();
        let value = eq_value_text(&arg);
        let (Some(key), Some(value)) = (key, value) else {
            diags.push(diag(file_id, arg_range, DiagnosticCode::E161));
            ok = false;
            continue;
        };
        let key_text = key.text().to_string();
        if key_text != STYLE_KEY_LINE
            && key_text != STYLE_KEY_DISPATCH
            && !captures.contains(&key_text.as_str())
        {
            diags.push(diag(file_id, arg_range, DiagnosticCode::E162));
            ok = false;
            continue;
        }
        entries.push(StyleEntry {
            key: key_text,
            value: parse_style_token(&value),
            range: arg_range,
        });
    }

    if !ok {
        return None;
    }
    if !any {
        // `@[style()]` — parses, silences nothing.
        diags.push(diag(file_id, range, DiagnosticCode::E161));
        return None;
    }
    Some(StyleAnnotation { entries, range })
}

/// Classify a `@[style(…)]` clause's value string against the closed
/// built-in presentation vocabulary (`docs/prose-dialect-spec.md` §3.5b
/// addenda 3–4). Never fails — "any other name is a custom hook emitting a
/// stable `brink-*` class for host CSS" is the spec's own fallback rule,
/// so an unrecognized value is [`StyleToken::Custom`], not a diagnostic.
fn parse_style_token(value: &str) -> StyleToken {
    match value {
        "left" => StyleToken::AlignLeft,
        "center" => StyleToken::AlignCenter,
        "right" => StyleToken::AlignRight,
        "bold" => StyleToken::Bold,
        "italic" => StyleToken::Italic,
        "dim" => StyleToken::Dim,
        "mono" => StyleToken::Mono,
        "uppercase" => StyleToken::Uppercase,
        "conceal" => StyleToken::Conceal,
        _ if is_hex_color(value) => StyleToken::Color(value.to_string()),
        _ => StyleToken::Custom(value.to_string()),
    }
}

/// `#rgb` or `#rrggbb` — the narrow, unambiguous raw-color shape this v1
/// recognizes (see [`StyleToken::Color`]'s doc for why nothing wider, like
/// a named-CSS-color keyword list, is attempted).
fn is_hex_color(value: &str) -> bool {
    let Some(hex) = value.strip_prefix('#') else {
        return false;
    };
    (hex.len() == 3 || hex.len() == 6) && hex.bytes().all(|b| b.is_ascii_hexdigit())
}
