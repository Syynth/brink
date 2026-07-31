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
//! Five annotation names have a ruled native meaning today:
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
//! - **`element`** — `@[element(args = "…")]`, the prose-dialect "second
//!   authoring surface"'s pattern declaration (issue #1719,
//!   `docs/prose-dialect-spec.md` §3.5b sitting 4 addenda 2–3). **This
//!   module delivers the declaration surface** — [`element_annotation`]
//!   parses the `args`/`name` clauses, validates the pattern compiles as a
//!   portable regex (`E159`), and validates its named captures against the
//!   declaration's own params (`E160`, the spec's "compile-checked" capture
//!   contract). Also parses the bare `block` clause (issue #1839,
//!   `docs/decision-log.md` 2026-07-31 "Conventions are annotated
//!   handlers"): `@[element(args = "…", block)]` declares that the handler
//!   captures the run *following* its matched line into a trailing
//!   `content`-typed param, and this module validates that receiver exists
//!   (`E166`) — see [`ElementAnnotation::block`]'s doc. The `!name` sigil
//!   dispatch rewrite the annotation exists to drive — matching a content
//!   line, binding captures, finding a block's terminator, and lowering to
//!   a call — is **not** implemented here; see [`ElementAnnotation`]'s own
//!   doc for why, and `docs/prose-dialect-spec.md` §3.5b's Deferred list.
//!   Issue #1838 added the **other** natural-notation spelling, `claims =
//!   "…"` beside `args = "…"` — a pattern that claims a prose line
//!   carrying no `!name` sigil. A claim is validated in both directions
//!   (`E160` *and* `E167`: params ≡ captures, since every argument of the
//!   rewritten call comes from a capture) and is legal only above a
//!   top-level `fn` (`E112` otherwise — see [`is_consumed_position`]). The
//!   dispatch itself lives in [`super::element`]; the `!name` *sigil*
//!   rewrite remains unimplemented for both spellings, per the deferred
//!   list above.
//! - **`style`** — `@[style(…)]`, the companion editor-presentation
//!   annotation (same spec section, addenda 3–4). **This module delivers
//!   it** as a pure declaration surface — [`style_annotation`] requires a
//!   paired `element` on the same declaration (`E163`), parses `key =
//!   "value"` clauses (`E161`), validates each key against `element`'s
//!   captures plus `line`/`dispatch` (`E162`), and classifies each value
//!   against the closed built-in presentation vocabulary
//!   ([`crate::StyleToken`]). Nothing downstream reads it yet — the
//!   consumer is the held editor track (NS-T, issues #1131/#1350); see
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
use crate::{
    Diagnostic, DiagnosticCode, EffectsAssertion, ElementAnnotation, Param, Severity,
    StyleAnnotation, StyleEntry, StyleToken, TypeExpr,
};

use super::SyntaxNode;

/// The effects-assertion annotation name.
const EFFECTS: &str = "effects";

/// The module-rename annotation name — consumed by [`super::module`] at file
/// level; named here only so it is not reported as an unknown name.
const WAS: &str = "was";

/// The source-level diagnostic-suppression annotation name (issue #1161).
const ALLOW: &str = "allow";

/// The prose-dispatch pattern-declaration annotation name (issue #1719,
/// `docs/prose-dialect-spec.md` §3.5b).
const ELEMENT: &str = "element";

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
        // A *claiming* `@[element(claims = "…")]` (issue #1838) is narrower
        // than its `args` sibling: the rewrite is an expression call, and
        // only a top-level `fn` is callable as one. A claim on a `flow`, or
        // on a nested `fn`, would parse and validate and then claim
        // nothing — exactly the silent no-op the `@`-namespace rule exists
        // to prevent — so it is reported misplaced (`E112`) instead.
        ELEMENT if declares_claim(line) => attached_declaration(line)
            .is_some_and(|d| d.kind() == N::FN_DECL && container_nesting_depth(&d) == 0),
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

/// `true` when `line` is an `@[element(…)]` carrying a `claims = "…"`
/// clause — the natural-notation spelling (issue #1838), which has a
/// narrower legal placement than `args = "…"`.
///
/// A syntactic read of the clause *key* only: whether the value is a valid
/// pattern, and whether its captures line up with the declaration's params,
/// stays [`parse_element`]'s job. Placement must be decidable before any of
/// that, so a malformed claim is still reported at the right position.
fn declares_claim(line: &SyntaxNode) -> bool {
    ast::AnnotationLine::cast(line.clone())
        .and_then(|l| l.args())
        .is_some_and(|args| {
            args.args()
                .any(|a| a.name_token().is_some_and(|t| t.text() == ELEMENT_CLAIMS))
        })
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
    if !matches!(name.text(), EFFECTS | WAS | ALLOW | ELEMENT | STYLE) {
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

// ─── `@[element]` / `@[style]` declaration surface (issue #1719) ─────────
//
// The `!name` sigil dispatch rewrite these annotations exist to declare is
// NOT implemented here — only the declaration surface is (parse, validate,
// store on the `Knot`/`Stitch`). See this module's doc comment for why.

/// The `@[element(…)]` clause keys. Exactly one of `args = "…"` (the
/// `!name`-dispatched remainder pattern) and `claims = "…"` (the
/// natural-notation whole-line claim, issue #1838) is required; `name =
/// "…"` is the optional dispatch-name alias.
const ELEMENT_ARGS: &str = "args";
const ELEMENT_CLAIMS: &str = "claims";
const ELEMENT_NAME: &str = "name";

/// The `@[element(…)]` bare `block` clause (issue #1839, `docs/decision-
/// log.md` 2026-07-31): a flag, not a `key = "value"` pair — present or
/// absent, never assigned. See [`ElementAnnotation::block`]'s doc for what
/// it declares.
const ELEMENT_BLOCK: &str = "block";

/// The native-type-system spelling `@[element(…, block)]`'s trailing
/// capture parameter must carry (`text: content`, per the ruled example).
/// Checked as plain annotation text here, at the declaration surface —
/// this module never calls into the type checker, and `content` is not a
/// resolvable native type yet either (`docs/prose-dialect-spec.md` §3.5b's
/// own Deferred list); the check below is deliberately shallow, exactly
/// the "declared, not resolved" posture the rest of this file's `element`/
/// `style` surface already takes.
const ELEMENT_CONTENT_TYPE: &str = "content";

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

/// Read the `@[element(args = "…")]` annotation attached to `decl` (a
/// `flow`/`fn` declaration node), if it declares one.
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
/// to the block capture contract. A `claims = "…"` pattern (issue #1838)
/// naming a parameter no capture matches is the converse check, `E167` —
/// the rewritten call has no other source of arguments, so every parameter
/// must come from a capture.
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
    let mut claims = false;
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
            // `args` and `claims` are two spellings of the same slot, and
            // the slot fills at most once — a declaration carrying both is
            // asking to be dispatched two incompatible ways.
            ELEMENT_ARGS if pattern.is_none() => pattern = Some(value),
            ELEMENT_CLAIMS if pattern.is_none() => {
                pattern = Some(value);
                claims = true;
            }
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

    // The claiming half needs the *converse* check too (`E167`, issue
    // #1838): a claimed line is rewritten to exactly one call whose every
    // argument comes from a named capture, so a parameter no capture names
    // has nothing to bind it to. A `!name` handler is exempt — it stays
    // callable by hand with ordinary arguments.
    if claims && let Some(p) = params.iter().find(|p| !captures.contains(&p.name.text)) {
        diags.push(diag(file_id, p.name.range, DiagnosticCode::E167));
        return None;
    }

    Some(ElementAnnotation {
        pattern,
        claims,
        captures,
        alias,
        block,
        range,
    })
}

/// `true` when `params` ends with a `content`-typed parameter that is not
/// one of `captures` — the structural shape `block` requires (`E166`
/// otherwise). Declaration-surface-only, like the rest of this module: this
/// checks the raw `TypeExpr` text, not a resolved type, because `content`
/// is not a resolvable native type yet (see [`ELEMENT_CONTENT_TYPE`]'s
/// doc).
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

/// Read the `@[style(…)]` annotation attached to `decl`, if it declares
/// one. Requires a paired [`element_annotation`] on the same declaration
/// (`E163`) — pass the already-lowered `element` (or `None`) so this
/// doesn't re-parse the `@[element(…)]` line itself.
///
/// A second `@[style]` on the same declaration is `E048`. A clause that
/// isn't a clean `key = "value"` pair, or an argument list that is
/// missing/empty, is `E161`. A key that is neither `line`, `dispatch`,
/// nor one of `element`'s named captures is `E162`.
pub(super) fn style_annotation(
    file_id: FileId,
    decl: &SyntaxNode,
    element: Option<&ElementAnnotation>,
    diags: &mut Vec<Diagnostic>,
) -> Option<StyleAnnotation> {
    let mut chosen: Option<StyleAnnotation> = None;
    for line in annotations_before(decl) {
        if line.name_token().is_none_or(|t| t.text() != STYLE) {
            continue;
        }
        let range = line.syntax().text_range();
        let Some(parsed) = parse_style(file_id, &line, range, element, diags) else {
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
    diags: &mut Vec<Diagnostic>,
) -> Option<StyleAnnotation> {
    let Some(element) = element else {
        diags.push(diag(file_id, range, DiagnosticCode::E163));
        return None;
    };

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
            && !element.captures.contains(&key_text)
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
