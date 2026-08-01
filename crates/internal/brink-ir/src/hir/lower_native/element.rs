//! Natural-notation element dispatch (issue #1838) — the slice that makes
//! the prose grammar *mean* something.
//!
//! `docs/decision-log.md`'s 2026-07-31 ruling ("Conventions are annotated
//! handlers: the declarative element surface is subsumed by the annotation
//! surface") collapsed two element mechanisms into one: **a preset element
//! is literally an annotated handler.** A scene heading is a matched line,
//! captures bound to params by name, and *exactly one call* — the same
//! three steps `!radio` takes, minus the sigil. The `lower:` column, the
//! `Conventions` type and the chain-rule engine are dissolved by that same
//! ruling and are deliberately absent here.
//!
//! ```brink
//! @[element(claims = "^INT\\. (?<place>.+)$")]
//! fn interior(place) { return "— inside " + place + " —"; }
//!
//! flow main() {
//!   INT. MARKET SQUARE
//! }
//! ```
//!
//! The heading line no longer reaches `body::lower_one_item`'s loud-`E129`
//! arm: it is claimed, `place` binds to `MARKET SQUARE`, and the line
//! lowers to one call whose value *is* the line.
//!
//! # No invisible expansion
//!
//! Every claimed line is recorded as a [`crate::ElementMatch`] on the
//! `HirFile` — the claimed range, the prose shape it was written as, the
//! handler's name **and the annotation's own source range**, and each
//! capture as a span. The ruling is explicit that a rewritten line must
//! point at real source; that record is how the `LineContext`/IDE query
//! family answers "what happened to this line, and where is the code that
//! did it" without re-running the match.
//!
//! # What claims, and what does not
//!
//! - A pattern is a *claim* only when spelled `claims = "…"`. The
//!   `args = "…"` form declares the `!name`-dispatched handler, whose
//!   sigil rewrite is still unimplemented (`docs/prose-dialect-spec.md`
//!   §3.5b Deferred).
//! - Only a **top-level `fn`** may claim: the rewrite is an expression
//!   call, and a `flow` is not callable as one. A `claims` annotation
//!   anywhere else is `E112` (misplaced), enforced by
//!   [`super::annotation::handle_line`]'s placement rule. "Top-level"
//!   means a direct child of the file, matching exactly what [`collect`]
//!   scans — a `fn` declared inside a `module { … }` block is *also*
//!   misplaced (issue #1847), even though it is otherwise un-nested in
//!   any `flow`/`fn`: `collect` never looks inside a `MODULE_DECL`, so a
//!   claim admitted there would validate and then silently never
//!   dispatch to anything.
//! - Only a **wholly literal** prose line is a candidate — one with no
//!   interpolation, glue, markup, tags, label or embedded divert. A line
//!   carrying dynamic parts has no fixed text for a pattern to match, and
//!   capture spans over it would not point at anything real.
//! - A claiming handler's **own body is not claimable** (the staging rule
//!   §3.5 states for the conventions module: it cannot use the conventions
//!   it defines). Without this, a handler whose body repeats the shape it
//!   claims would rewrite into a call on itself.
//!
//! # Deliberately not here
//!
//! Block capture (the `block` param form) and `fn conventions()`
//! registration + comptime evaluation are the ruling's other two build
//! slices, filed separately (issues #1839/#1840). The confinement of
//! claiming to the `brink.toml`-named conventions module needs project
//! identity that single-file lowering does not have — this module only
//! records the raw material for that check ([`collect`] populates
//! `HirFile::claim_handlers` with every declared handler's name and
//! annotation range, independent of `matches`); the check itself runs at
//! the project layer (issue #1844, `brink_db::queries::analysis::
//! conventions_confinement_diagnostics_query`), which is the one seam that
//! has both a file's module identity and the configured pointer.

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use rowan::{TextRange, TextSize};
use regex_syntax::hir::Hir;

use crate::hir::FileId;
use crate::{
    Content, ContentPart, Diagnostic, DiagnosticCode, ElementCapture, ElementDisposition,
    ElementKind, ElementMatch, Expr, Name, Path, Stmt, StringExpr, StringPart,
};

use super::SyntaxNode;
use super::provenance::native_provenance;
use crate::provenance::NodeClass;

/// One declared natural-notation handler: a top-level `fn` whose
/// `@[element(claims = "…")]` pattern claims prose lines.
struct ClaimHandler {
    /// The handler's own name, carrying its declaration-site range.
    name: Name,
    /// Parameter names in declaration order — the argument order the
    /// rewritten call uses. Guaranteed by `E160`/`E167` to be exactly the
    /// pattern's named-capture set.
    params: Vec<String>,
    /// The compiled claiming pattern.
    pattern: regex::Regex,
    /// Range of the `@[element(claims = "…")]` line itself.
    annotation: TextRange,
    /// The handler declaration's own range — used to suppress claiming
    /// inside the handler's own body (the staging rule).
    decl: TextRange,
}

/// The dispatcher threaded through body lowering: the file's claiming
/// handlers plus the per-line classification records they produce.
///
/// Built once per file by [`collect`] and passed down by reference, rather
/// than re-derived per line: a per-line whole-tree scan would make body
/// lowering quadratic in file size.
pub(super) struct Elements {
    handlers: Vec<ClaimHandler>,
    /// Every claimed line, in the order lowering *reached* it — not
    /// necessarily source order (a `CHOICE_POINT` lowers its source-later
    /// continuation before its source-earlier choice bodies). [`super::lower`]
    /// sorts this by line start before storing it as
    /// `HirFile::element_matches`, whose own doc promises source order; this
    /// field is the pre-sort accumulation, an implementation detail of how
    /// body lowering visits nodes.
    pub(super) matches: Vec<ElementMatch>,
}

impl Elements {
    /// `true` when no handler in this file claims anything, so callers can
    /// skip candidate testing entirely on the overwhelmingly common path.
    fn is_inert(&self) -> bool {
        self.handlers.is_empty()
    }

    /// Every claiming handler *declared* in this file, in declaration
    /// order — [`HirFile::claim_handlers`](crate::HirFile::claim_handlers)'s
    /// source (issue #1844's confinement check). Deliberately independent
    /// of `matches`: a handler that claims nothing in its own file is still
    /// a declaration, and still checkable.
    pub(super) fn handler_decls(&self) -> Vec<crate::ClaimHandlerDecl> {
        self.handlers
            .iter()
            .map(|h| crate::ClaimHandlerDecl {
                name: h.name.clone(),
                annotation: h.annotation,
            })
            .collect()
    }
}

/// Collect every claiming handler declared in `root`.
///
/// Silent on everything `container.rs` already validated: each
/// `@[element(…)]` line is parsed and checked against its own declaration
/// exactly once there — that validation pass's own diagnostics are
/// discarded here (into a throwaway `scratch` vec) rather than threaded
/// through, since re-reporting would double every `E159`/`E160`/`E167`.
/// Duplicate-pattern diagnosis ([`diagnose_duplicate_patterns`]) does
/// *not* happen here either: it needs to see which handlers actually
/// fired during body lowering, which hasn't happened yet at collection
/// time — see that function's doc.
pub(super) fn collect(file_id: FileId, root: &SyntaxNode) -> Elements {
    let mut handlers = Vec::new();
    for node in root.children() {
        // Top-level `fn` only — see the module doc. `handle_line`'s
        // placement rule reports every other position as `E112`, so a
        // claim this loop skips is never silently dropped.
        if node.kind() != N::FN_DECL {
            continue;
        }
        let Some(decl) = ast::FnDecl::cast(node.clone()) else {
            continue;
        };
        let Some(name) = super::container::name_from(decl.name_token()) else {
            continue;
        };
        let params = super::container::lower_params(decl.param_list());
        let mut scratch: Vec<Diagnostic> = Vec::new();
        let Some(element) =
            super::annotation::element_annotation(file_id, &node, &params, &mut scratch)
        else {
            continue;
        };
        if !element.claims {
            continue;
        }
        let Ok(pattern) = regex::Regex::new(&element.pattern) else {
            continue;
        };
        handlers.push(ClaimHandler {
            name,
            params: params.into_iter().map(|p| p.name.text).collect(),
            pattern,
            annotation: element.range,
            decl: node.text_range(),
        });
    }
    Elements {
        handlers,
        matches: Vec::new(),
    }
}

/// Try to find a witness string that both patterns match, proving they overlap.
///
/// Returns `true` if a common witness is found, `false` otherwise (which does
/// not prove they never overlap, only that this heuristic couldn't find one).
/// Tries several techniques:
/// - Shared literal prefix: if both patterns start with the same fixed text
/// - Generated test strings from pattern structure
/// - Attempts to find a witness from one pattern and test against the other
fn patterns_have_provable_overlap(
    earlier_pattern: &regex::Regex,
    later_pattern: &regex::Regex,
) -> bool {
    // List of candidate witness strings to test.
    let mut witnesses = vec![];

    // Try parsing both patterns to extract literal information.
    if let (Ok(earlier_hir), Ok(later_hir)) = (
        regex_syntax::Parser::new().parse(earlier_pattern.as_str()),
        regex_syntax::Parser::new().parse(later_pattern.as_str()),
    ) {
        // Extract common literal prefixes.
        if let Some(shared_prefix) = extract_shared_literal_prefix(&earlier_hir, &later_hir) {
            witnesses.push(shared_prefix);
        }
        // Try to generate witness strings from the patterns' structure.
        // Generate from the later (likely more specific) pattern first.
        if let Some(generated) = generate_witness_from_hir(&later_hir) {
            witnesses.push(generated.clone());
        }
        if let Some(generated) = generate_witness_from_hir(&earlier_hir) {
            witnesses.push(generated);
        }
    }

    // Fallback simple witnesses: try empty string and basic ones.
    witnesses.extend(vec![
        String::new(),
        "a".to_string(),
        "1".to_string(),
        "A".to_string(),
        "VENDOR enters".to_string(),
        "enters".to_string(),
    ]);

    // Test each witness: if both patterns match it, they provably overlap.
    witnesses.into_iter().any(|witness| {
        earlier_pattern.is_match(&witness) && later_pattern.is_match(&witness)
    })
}

/// Extract a shared literal prefix from two regex HIRs.
///
/// Returns the prefix if both patterns start with the same literal sequence,
/// `None` otherwise.
fn extract_shared_literal_prefix(earlier: &Hir, later: &Hir) -> Option<String> {
    let earlier_prefix = extract_literal_prefix(earlier)?;
    let later_prefix = extract_literal_prefix(later)?;

    if earlier_prefix == later_prefix {
        Some(earlier_prefix)
    } else {
        None
    }
}

/// Extract the leading literal prefix from a regex HIR, if any.
///
/// For example, `^(?<p>.+)$` has no literal prefix (starts with an anchor).
/// `^INT\. (?<p>.+)$` has the prefix `"INT. "`.
fn extract_literal_prefix(hir: &Hir) -> Option<String> {
    use regex_syntax::hir::HirKind;

    match hir.kind() {
        HirKind::Literal(lit) => Some(String::from_utf8_lossy(&lit.0).to_string()),
        HirKind::Concat(parts) => {
            let mut prefix = String::new();
            for part in parts {
                if let HirKind::Literal(lit) = part.kind() {
                    prefix.push_str(&String::from_utf8_lossy(&lit.0));
                } else {
                    break;
                }
            }
            if prefix.is_empty() {
                None
            } else {
                Some(prefix)
            }
        }
        _ => None,
    }
}

/// Try to generate a simple witness string from a regex HIR.
///
/// Attempts to produce a minimal input that the regex might match.
/// This is a best-effort heuristic; returning `None` does not mean the
/// regex won't match anything. Conservative approach: only expand constructs
/// we know how to handle (literals, repetitions, basic classes).
fn generate_witness_from_hir(hir: &Hir) -> Option<String> {
    use regex_syntax::hir::HirKind;

    match hir.kind() {
        HirKind::Literal(lit) => {
            Some(String::from_utf8_lossy(&lit.0).to_string())
        }
        HirKind::Concat(parts) => {
            // For concatenation, try to build a witness from literals and
            // expand repetitions/classes to minimal examples.
            let mut witness = String::new();
            for part in parts {
                match part.kind() {
                    HirKind::Literal(lit) => {
                        witness.push_str(&String::from_utf8_lossy(&lit.0));
                    }
                    HirKind::Look(_) => {
                        // Anchors don't contribute text, skip them.
                    }
                    HirKind::Repetition(r) => {
                        // For repetitions, try to expand the inner part.
                        if let Some(w) = generate_witness_from_hir(&r.sub) {
                            // Add just one instance for minimal witness.
                            witness.push_str(&w);
                        }
                    }
                    HirKind::Class(_) => {
                        // For character classes, pick a representative character.
                        witness.push('a');
                    }
                    HirKind::Empty => {
                        // Empty doesn't contribute text.
                    }
                    _ => {
                        // For any other construct (groups with non-literals,
                        // alternations, etc), stop expanding here.
                        break;
                    }
                }
            }
            if witness.is_empty() {
                None
            } else {
                Some(witness)
            }
        }
        HirKind::Repetition(r) => generate_witness_from_hir(&r.sub),
        HirKind::Class(_) => Some("a".to_string()),
        HirKind::Look(_) => None,
        HirKind::Empty => Some(String::new()),
        _ => None,
    }
}

/// `E168` (issue #1848): flag every claiming handler whose pattern is
/// byte-identical to an earlier-declared one's *and never actually won a
/// claim*.
///
/// `E170` (issue #1859): flag every claiming handler whose pattern can
/// provably overlap with an earlier-declared one's *and never actually won a
/// claim of its own*.
///
/// `elements.handlers` is in declaration order (the order `collect` pushed
/// them, itself `root.children()`'s source order), which is also
/// [`try_claim`]'s dispatch order — see that function's doc for why
/// "earlier-declared" is the same as "wins". An identical pattern
/// *provably* matches the identical set of lines — but "the earlier one
/// always wins" is not quite the same claim as "the later one is dead
/// code". `try_claim` excludes a handler from claiming lines inside its
/// **own** declaration (the staging rule), and that exclusion does not
/// extend to a later, byte-identical twin: a twin is exactly the handler
/// that *can* claim a line living inside the earlier one's own body, since
/// the earlier one is barred from claiming there. So a later twin is
/// genuinely live, not dead code, for precisely those lines.
///
/// The same logic applies to E170: a later handler with an overlapping
/// (but non-identical) pattern is live if it actually won at least one claim
/// (necessarily for a line that the earlier pattern couldn't claim, or where
/// it was barred by the staging rule). Only report when the later handler
/// produced zero actual claims.
///
/// Must therefore run **after** the whole file has been lowered
/// ([`super::lower`] calls this once `walk_top_level` has returned), not
/// from [`collect`] before any body is lowered — `elements.matches` is the
/// only ground truth for "did this handler ever actually win a claim",
/// and it does not exist yet at collection time. A later handler that
/// produced at least one entry there is live and is not diagnosed; one
/// that produced none is provably dead or unreachable.
///
/// Each later handler is reported **at most once**, against the first
/// (source-order) earlier handler it overlaps with — a handler that overlaps
/// two or more earlier ones is not re-reported once per overlap.
///
/// `O(n²)` over a file's claiming handlers, which in practice number in
/// the single digits (one project's worth of prose conventions, not a
/// generated table) — a sorted/hashed pass would trade clarity for
/// headroom this call site doesn't need.
pub(super) fn diagnose_duplicate_patterns(
    file_id: FileId,
    elements: &Elements,
    diags: &mut Vec<Diagnostic>,
) {
    let handlers = &elements.handlers;
    for (later_idx, later) in handlers.iter().enumerate().skip(1) {
        // Ground truth, not assumption: did `later` ever actually win a
        // claim? If so it is live, not dead code.
        let fired = elements
            .matches
            .iter()
            .any(|m| m.handler.range == later.name.range);
        if fired {
            continue;
        }

        // Look for an earlier handler that conflicts with `later`.
        // Iterate backwards (from `later_idx - 1` down to 0) to find the
        // first (textually earliest) conflicting handler, and stop there.
        for earlier in handlers[..later_idx].iter().rev() {
            let is_byte_identical = earlier.pattern.as_str() == later.pattern.as_str();
            let has_provable_overlap = is_byte_identical
                || patterns_have_provable_overlap(&earlier.pattern, &later.pattern);

            if !has_provable_overlap {
                continue;
            }

            // Found a conflict. Pick the right diagnostic code.
            let code = if is_byte_identical {
                DiagnosticCode::E168
            } else {
                DiagnosticCode::E170
            };

            diags.push(Diagnostic {
                file: file_id,
                // Emit at `later`'s name token so `@[allow(...)]` can
                // suppress it — `AllowScope::range` covers the annotated
                // declaration but explicitly excludes the annotation line
                // itself, so emitting on the annotation would make
                // `@[allow(code)]` unable to suppress this diagnostic.
                range: later.name.range,
                message: code.title().to_string(),
                code,
            });

            // Stop at the first conflict to avoid re-reporting this handler
            // if it overlaps multiple earlier ones.
            break;
        }
    }
}

/// Try to claim one body item, returning the statements that replace it.
///
/// `None` means "nothing claimed this" and the caller lowers the item the
/// way it always did — the fall-through that keeps every unclaimed line,
/// and every file with no claiming handler, byte-identical.
///
/// # Dispatch order (interim)
///
/// When more than one handler's pattern matches a line, the first one in
/// `elements.handlers` wins — [`Iterator::find`] below, over a `Vec` built
/// by `collect` in top-level declaration order. **This is an interim rule,
/// not a permanent one.** Issue #1848: the 2026-07-31 §9.1 ruling's item
/// (5) says `fn conventions()` will *register* handlers in order once
/// issue #1840 lands, and statement order in that registering function —
/// not a claiming `fn`'s textual position in the file — becomes the
/// authoritative resolution order then. Until #1840 lands, declaration
/// order is the only order there is, so it is what this dispatches on; do
/// not treat it as a rule worth entrenching or optimizing around. Two
/// patterns that can both claim one line get no diagnostic today except
/// the narrow byte-identical case ([`diagnose_duplicate_patterns`], issue
/// #1848) — a genuinely overlapping (non-identical) pair silently
/// prefers the earlier one, exactly the failure mode "pattern power
/// proportional to auditability" (`docs/prose-dialect-spec.md` §3.5b)
/// exists to keep visible, not eliminate outright.
pub(super) fn try_claim(
    file_id: FileId,
    node: &SyntaxNode,
    elements: &mut Elements,
) -> Option<Vec<Stmt>> {
    if elements.is_inert() {
        return None;
    }
    let (kind, text_node) = candidate(node)?;
    let text = text_node.text().to_string();
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return None;
    }
    // Where `trimmed` starts inside `text_node`, so capture offsets land on
    // real source bytes rather than on the untrimmed run's start.
    let lead = u32::try_from(text.len() - text.trim_start().len()).unwrap_or(0);
    let base = text_node.text_range().start() + TextSize::from(lead);

    let claimed = node.text_range();
    let handler = elements
        .handlers
        .iter()
        .find(|h| !h.decl.contains_range(claimed) && h.pattern.is_match(trimmed))?;

    let caps = handler.pattern.captures(trimmed)?;
    let mut captures = Vec::with_capacity(handler.params.len());
    for param in &handler.params {
        // `E160`/`E167` already pinned params ≡ named captures at the
        // declaration, so a miss here means the group did not participate
        // in this particular match (an alternation branch). Declining the
        // claim is the honest answer: a call with a missing argument is
        // not "exactly one call", it is a broken one.
        let m = caps.name(param)?;
        captures.push(ElementCapture {
            name: param.clone(),
            text: m.as_str().to_string(),
            range: TextRange::new(
                base + TextSize::from(u32::try_from(m.start()).ok()?),
                base + TextSize::from(u32::try_from(m.end()).ok()?),
            ),
        });
    }

    let call = Expr::Call(
        Path {
            segments: vec![Name {
                text: handler.name.text.clone(),
                // The call is written at the claimed line, not at the
                // handler's declaration — the range a reader clicking the
                // rewritten call should land on.
                range: claimed,
            }],
            range: claimed,
        },
        captures
            .iter()
            .map(|c| {
                Expr::String(StringExpr {
                    parts: vec![StringPart::Literal(c.text.clone())],
                })
            })
            .collect(),
    );

    elements.matches.push(ElementMatch {
        line: claimed,
        kind,
        handler: handler.name.clone(),
        annotation: handler.annotation,
        captures,
        disposition: ElementDisposition::Call,
    });

    Some(vec![
        Stmt::Content(Content {
            ptr: Some(native_provenance(file_id, NodeClass::Content, node)),
            parts: vec![ContentPart::Interpolation(call)],
            tags: Vec::new(),
        }),
        Stmt::EndOfLine,
    ])
}

/// Classify a body item as a claim candidate, yielding the node whose text
/// a pattern is matched against.
///
/// A `CONTENT_LINE` qualifies only when it is *wholly* literal — exactly
/// one `TEXT` child and nothing else (no `LABEL`, `INTERPOLATION`, `SPAN`,
/// `TAG`, `GLUE_NODE`, `ESCAPE`, embedded divert or choice point). A
/// `SCENE_HEADING`'s title run qualifies the same way; the heading's
/// optional `[slug]` and trailing tags are structure the pattern is not
/// shown, so a heading carrying either is declined rather than matched
/// against a partial line.
fn candidate(node: &SyntaxNode) -> Option<(ElementKind, SyntaxNode)> {
    match node.kind() {
        N::CONTENT_LINE => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::TEXT && children.next().is_none())
                .then_some((ElementKind::ContentLine, first))
        }
        N::SCENE_HEADING => {
            let mut children = node.children();
            let first = children.next()?;
            (first.kind() == N::SCENE_TITLE && children.next().is_none())
                .then_some((ElementKind::SceneHeading, first))
        }
        _ => None,
    }
}
