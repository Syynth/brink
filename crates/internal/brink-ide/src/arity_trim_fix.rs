//! `Safe` fixer for `E031` (ordinary function-call argument-count mismatch)
//! and `E176` (divert-with-args argument-count mismatch): trim the
//! **over-supplied** call/divert site's excess arguments — issue #3428,
//! milestone 8 of the auto-fix epic (#3374, `docs/autofix-spec.md` §9's
//! first-wave `Safe` list: "E031/E176 over-supplied args → trim").
//!
//! # Runtime binding order — READ BEFORE TOUCHING THIS FILE
//!
//! `E031`/`E176` are both `Warning`-tier (`brink_ir::hir::diagnostics`):
//! the mismatched program still compiles, and the codegen for both call
//! shapes (`brink_analyzer::resolve::check_arity`/`check_divert_arity`,
//! firing on a resolution to `External`/`Knot`/`Stitch`/`Label`) is the
//! *classic* `Opcode::Call`/`Opcode::CallExternal` path — **not** the T1c
//! function-value path (`#fn(...)`/`call(...)`/`bind(...)`,
//! [`crate::creation_site_fix`]/[`crate::value_call_fix`]'s own territory).
//!
//! The two paths bind an over-supplied call's arguments in **opposite
//! directions**, and this is not a naming quibble — it changes which
//! argument a "trim the excess" fixer must delete to stay `Safe`:
//!
//! - The T1c path (`Opcode::MakeClosure`/`Opcode::CallValue`) pops exactly
//!   the *wire-carried* argument count and binds it in **declared order**
//!   (first supplied arg -> first param) — so `creation_site_fix`'s
//!   `TrimFnLiteralArgsFixer` correctly drops the **trailing** excess and
//!   keeps the leading prefix.
//! - The classic path this module fixes does not carry the *supplied*
//!   count into the callee at all. `Opcode::Call`'s callee has its own
//!   param-binding prologue (`declare_temp` per declared param), which
//!   pops **exactly its own declared count** off the shared value stack —
//!   a plain LIFO pop, so it reads whichever values the caller pushed
//!   *last*. `Opcode::CallExternal(fn_id, arg_count)` is explicit about
//!   the same thing: `arg_count` here is `info.params.len()` (the
//!   **declared** count, not `args.len()`, see
//!   `brink_ir::lir::lower::expr::lower_call`'s `SymbolKind::External`
//!   arm) — it pops that many values and reverses them
//!   (`vm.rs`'s `Opcode::CallExternal` handler: "Args were pushed
//!   left-to-right, popped right-to-left").
//!
//!   Either way, since codegen pushes every supplied argument in source
//!   order (`lower_call_args` never truncates to `params.len()`), an
//!   over-supplied call's **leading** arguments are the ones sitting
//!   deepest under the top-of-stack the callee's own prologue actually
//!   consumes — they are pushed, fully evaluated, and then never popped
//!   by anything. The **trailing** `expected` arguments are what the
//!   callee actually binds to its declared params.
//!
//! This was proven empirically, not just read off the bytecode (a
//! plausible-looking trace read is not the same as a repro): compiling
//! and playing `-> accuse("Hastings", "Poirot")` against `flow
//! accuse(who) { I accuse {who}! }` prints **"I accuse Poirot!"** — the
//! *trailing* argument wins, on both the ink and the native surface, for
//! both the ordinary-call and the divert-with-args shape.
//!
//! So the mechanically `Safe` trim here is the opposite of
//! [`crate::creation_site_fix`]'s: delete the **leading**
//! `got - expected` arguments (they were pushed, evaluated for any
//! side effect, and then silently discarded by the runtime — exactly
//! `docs/autofix-spec.md` §9's "the discarded args were already being
//! ignored", just naming the *other* end of the list than a first read of
//! "over-supplied args" suggests) and keep the trailing `expected` — the
//! ones the callee actually receives. Safety therefore hinges on the
//! **leading** (dropped) arguments being free of a nested call (or, on
//! ink, an `++`/`--` increment — the only other expression-position
//! mutation either frontend's grammar admits; native has neither a
//! postfix operator nor any way for a bare `=` assignment, which is
//! statement-only there, to reach inside an argument's expression subtree
//! — see [`native_is_pure`]'s doc), not the trailing ones. It also hinges
//! on the call's own return value being popped in isolation and on the
//! resolved target declaring no `ref` param — both BLOCKING review
//! findings, see [`ink_call_is_isolated`]/[`native_call_is_isolated`] and
//! [`expected_param_count`]'s doc.
//!
//! # Scope
//!
//! Reachable on both frontends (E176's own doc: "identical shape on the
//! native surface"), so this module parses with whichever frontend
//! [`brink_db::ProjectDb::is_native`] names, mirroring
//! [`crate::import_fix`]'s dialect branch. Only the shapes with an
//! unambiguous CST anchor are covered — an ordinary call
//! (`brink_syntax::ast::FunctionCall` / `brink_syntax_native::ast::CallExpr`)
//! for `E031`, and a divert/tunnel-call/thread-start
//! (`DivertTargetWithArgs`/`ThreadStart` on ink,
//! `DivertTarget`/`Splice` on native) for `E176`.
//!
//! **Correction (review finding, was wrong in an earlier revision of this
//! doc):** a tunnel *redirect* (`->-> target(args)`) was believed
//! unreachable here because `brink_ir::symbols::project::Projector::
//! walk_return` re-derives its HIR-level `arg_count` from a separate
//! `onwards_args` field rather than from a `DivertTargetWithArgs`'s own
//! `ArgList` — true at the HIR level, but the CST underneath it still
//! parses `->-> target(args)` as a real `DivertTargetWithArgs` (nested in
//! a `TunnelOnwardsNode`, `brink_ir::hir::lower::divert`'s
//! `lower_divert`), with its `ArgList` intact and its `.path()` range
//! matching the diagnostic's anchor exactly like a plain divert's. So
//! `ink_divert_args`'s plain descendants search finds it after all — and
//! the codegen for it (`container.rs`'s `StmtKind::Return` arm) pushes
//! `onwards_args` in source order exactly like an ordinary divert's args,
//! then `Opcode::TunnelReturn`, which never pops them itself (`vm.rs`) —
//! they are left for the target's own param-binding prologue to consume,
//! same LIFO trailing-wins convention as everywhere else in this module.
//! Confirmed empirically: `-> a -> / === a === / ->-> b(5, 3) / === b(x)
//! === / {x} / -> END` and the same program trimmed to `->-> b(3)` both
//! print `3` — see `e176_ink_tunnel_onwards_redirect_drops_the_leading_excess_argument`
//! below. This shape is fixed, not left alone.
//!
//! Under-supply (`got < expected`) has no mechanical rewrite (there is no
//! value to synthesize) and is left alone, same as `creation_site_fix`'s
//! `TrimFnLiteralArgsFixer` for its own over-binding-only scope.

use brink_db::ProjectDb;
use brink_ir::{Diagnostic, DiagnosticCode, FileId};
use rowan::{TextRange, TextSize};

use crate::fix::{Applicability, Fix, FixCx, Fixer};
use crate::rename::FileEdit;

/// The `E031` over-supply fixer for an ordinary call (`f(args…)`).
pub struct CallArityTrimFixer;

impl Fixer for CallArityTrimFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E031
    }

    fn max_applicability(&self) -> Applicability {
        Applicability::Safe
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        trim_fix(cx.db, d, DiagnosticCode::E031, false)
    }
}

/// The `E176` over-supply fixer for a divert-with-args site (`-> knot(args)`,
/// a tunnel call, or a thread-start).
pub struct DivertArityTrimFixer;

impl Fixer for DivertArityTrimFixer {
    fn code(&self) -> DiagnosticCode {
        DiagnosticCode::E176
    }

    fn max_applicability(&self) -> Applicability {
        Applicability::Safe
    }

    fn fixes(&self, cx: &FixCx<'_>, d: &Diagnostic) -> Vec<Fix> {
        trim_fix(cx.db, d, DiagnosticCode::E176, true)
    }
}

/// Shared entry point for both fixers: resolve the target's declared
/// parameter count, locate the call/divert site on whichever frontend
/// `d.file` uses, and build the leading-argument trim if it is safe.
fn trim_fix(db: &ProjectDb, d: &Diagnostic, code: DiagnosticCode, is_divert: bool) -> Vec<Fix> {
    let Some(expected) = expected_param_count(db, d) else {
        return Vec::new();
    };
    let Some(source) = db.source(d.file) else {
        return Vec::new();
    };
    let native = db.is_native(d.file);
    let shape = match (is_divert, native) {
        (false, false) => ink_call_args(source, d.range),
        (false, true) => native_call_args(source, d.range),
        (true, false) => ink_divert_args(source, d.range),
        (true, true) => native_divert_args(source, d.range),
    };
    let Some(shape) = shape else {
        return Vec::new();
    };
    let name = source
        .get(usize::from(d.range.start())..usize::from(d.range.end()))
        .unwrap_or_default();
    build_trim_fix(d.file, code, name, expected, &shape)
}

/// `d.range`'s resolved target's own declared parameter count — never
/// derived from the diagnostic's message text (`crate::import_fix`'s own
/// module doc: "never by parsing the diagnostic message"). `d.range` is
/// exactly the `ResolvedRef::range` `resolve_function`/`resolve_divert`
/// pushed alongside the diagnostic (issue #1561's range contract), so a
/// plain equality lookup on the same file's resolution map finds it.
///
/// Returns `None` — no fix — when any declared param is `ref` (BLOCKING
/// finding): `lower_call_args` decides ref-ness **positionally against the
/// declared params**
/// (`crates/internal/brink-ir/src/lir/lower/expr.rs`: `let is_ref =
/// params.get(i).is_some_and(|p| p.is_ref);`), while the runtime binds the
/// call's arguments by **trailing** position (this module's own "Runtime
/// binding order" doc). Trimming the leading excess re-indexes every
/// remaining argument against that positional decision, so a `ref` param
/// can silently end up bound to a plain value (or vice versa) — flipping
/// write-back rather than preserving it. Repro: `VAR hp = 10` +
/// `=== function heal(ref h, amt) === / ~ h = h + amt / ~ return h` +
/// `~ temp r = heal(hp, hp, 5)` — before the fix arg0 (`hp`, a pointer)
/// binds nothing (the callee binds only its trailing 2 args), so `{hp}`
/// stays `10`; the offered trim (`heal(hp, 5)`) would make arg0 bind `h`
/// for real, changing `{hp}` to `15`. Not observably equivalent — withhold
/// the fix entirely rather than risk it.
fn expected_param_count(db: &ProjectDb, d: &Diagnostic) -> Option<usize> {
    let (resolutions, _) = db.resolve(d.file)?;
    let target = resolutions.iter().find(|r| r.range == d.range)?.target;
    let index = db.symbol_index();
    let symbol = index.symbols.get(&target)?;
    if symbol.params.iter().any(|p| p.is_ref) {
        return None;
    }
    Some(symbol.params.len())
}

/// One call/divert-with-args site as observed from either frontend's CST:
/// enough to build the trim edit and check its safety without the shared
/// logic needing to know which dialect the syntax came from.
struct ArgsShape {
    /// Where the argument content begins — the deletion's start whether we
    /// drop some leading arguments or (`expected == 0`) all of them.
    zero_start: TextSize,
    /// Byte offset of the site's closing `)` — the deletion's end when
    /// `expected == 0` (there is no "first kept argument" to anchor on).
    close_paren: usize,
    /// Each supplied argument's own `(range, is_pure)`, in source order.
    /// "Pure" means free of a nested call (or, on ink, an `++`/`--`
    /// increment) anywhere in its subtree — see [`ink_is_pure`]/
    /// [`native_is_pure`] and this module's doc for why it is the
    /// *leading* arguments (the ones a `Safe` trim here deletes) that must
    /// be pure.
    args: Vec<(TextRange, bool)>,
}

/// Build the `Fix` for an over-supplied site, or nothing when the site is
/// not over-supplied, the leading (dropped) arguments are not provably
/// pure, or there is nothing left to delete.
fn build_trim_fix(
    file: FileId,
    code: DiagnosticCode,
    name: &str,
    expected: usize,
    shape: &ArgsShape,
) -> Vec<Fix> {
    let got = shape.args.len();
    if got <= expected {
        // Exact match (nothing to fix) or under-supply (no mechanical
        // rewrite — there is no value to synthesize for a missing arg).
        return Vec::new();
    }
    let drop_count = got - expected;
    // Safety (this module's doc, "Runtime binding order"): the callee
    // actually receives the *trailing* `expected` arguments, so the
    // *leading* `drop_count` ones are what a Safe trim deletes — and they
    // must be side-effect-free for that deletion to be observably
    // equivalent.
    if shape.args[..drop_count].iter().any(|(_, pure)| !pure) {
        return Vec::new();
    }
    let start = shape.zero_start;
    let end = if expected == 0 {
        TextSize::from(u32::try_from(shape.close_paren).unwrap_or(u32::MAX))
    } else {
        shape.args[drop_count].0.start()
    };
    if end <= start {
        return Vec::new();
    }
    vec![Fix {
        code,
        title: format!(
            "Remove leading extra argument(s) — `{name}` binds the trailing \
             {expected} argument(s) supplied here"
        ),
        applicability: Applicability::Safe,
        edits: vec![FileEdit {
            file,
            range: TextRange::new(start, end),
            new_text: String::new(),
        }],
        caret: None,
    }]
}

// ── ink frontend ─────────────────────────────────────────────────────────

fn ink_is_pure(node: &brink_syntax::SyntaxNode) -> bool {
    !node.descendants().any(|n| {
        matches!(
            n.kind(),
            brink_syntax::SyntaxKind::FUNCTION_CALL
                | brink_syntax::SyntaxKind::CALL_EXPR
                | brink_syntax::SyntaxKind::POSTFIX_EXPR
        )
    })
}

/// Whether `fc`'s return value is popped in **isolation** — nothing else
/// shares the same evaluation's stack region beneath it (BLOCKING finding:
/// "the call is the entire RHS of a `~` assignment").
///
/// Only the direct RHS of a `~ temp` decl or a `~` assignment qualifies.
/// A call nested inside a larger expression — `~ temp r = 1 +
/// greet("Al", "Bob")` — is **not** isolated: `Infix` pushes `1` first,
/// then evaluates `greet(...)` (which leaves the leaked leading arg `"Al"`
/// sitting *beneath* the call's own return value, per this module's
/// "Runtime binding order" doc), so the stack right before `+` fires reads
/// `[1, "Al", "Hi Bob"]` — `Add` pops the top two (`"Hi Bob"` and `"Al"`),
/// not `1` and `"Hi Bob"`, and `1` is left stranded. Trimming the leading
/// arg removes that corruption entirely, so the trimmed program computes
/// `1 + "Hi Bob"` instead of the diagnosed program's actual `"Al" + "Hi
/// Bob"` — a different result, not an equivalent one. Confirmed empirically
/// (not just reasoned about): see
/// `e031_no_fix_when_the_call_is_not_the_entire_rhs` below.
fn ink_call_is_isolated(fc: &brink_syntax::ast::FunctionCall) -> bool {
    use brink_syntax::ast::{Assignment, AstNode as _, TempDecl};
    let Some(parent) = fc.syntax().parent() else {
        return false;
    };
    match parent.kind() {
        brink_syntax::SyntaxKind::TEMP_DECL => TempDecl::cast(parent)
            .and_then(|t| t.value())
            .is_some_and(|v| v.syntax().text_range() == fc.syntax().text_range()),
        brink_syntax::SyntaxKind::ASSIGNMENT => Assignment::cast(parent)
            .and_then(|a| a.value())
            .is_some_and(|v| v.syntax().text_range() == fc.syntax().text_range()),
        _ => false,
    }
}

fn ink_call_args(source: &str, target_range: TextRange) -> Option<ArgsShape> {
    use brink_syntax::ast::{AstNode as _, FunctionCall};

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let root = tree.syntax().clone();

    let fc = root
        .descendants()
        .filter_map(FunctionCall::cast)
        .find(|fc| fc.identifier().map(|i| i.syntax().text_range()) == Some(target_range))?;
    if !ink_call_is_isolated(&fc) {
        return None;
    }
    let arg_list = fc.arg_list()?;
    let close_paren = crate::text::closing_paren_offset(fc.syntax())?;
    let args = arg_list
        .args()
        .map(|a| (a.syntax().text_range(), ink_is_pure(a.syntax())))
        .collect();
    Some(ArgsShape {
        zero_start: arg_list.syntax().text_range().start(),
        close_paren,
        args,
    })
}

fn ink_divert_args(source: &str, target_range: TextRange) -> Option<ArgsShape> {
    use brink_syntax::ast::{AstNode as _, DivertTargetWithArgs, ThreadStart};

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let root = tree.syntax().clone();

    let found = root
        .descendants()
        .filter_map(DivertTargetWithArgs::cast)
        .find_map(|dtwa| {
            (dtwa.path().map(|p| p.syntax().text_range()) == Some(target_range))
                .then(|| dtwa.arg_list())
                .flatten()
                .map(|al| (dtwa.syntax().clone(), al))
        })
        .or_else(|| {
            root.descendants()
                .filter_map(ThreadStart::cast)
                .find_map(|ts| {
                    (ts.target().map(|p| p.syntax().text_range()) == Some(target_range))
                        .then(|| ts.arg_list())
                        .flatten()
                        .map(|al| (ts.syntax().clone(), al))
                })
        })?;
    let (container, arg_list) = found;
    let close_paren = crate::text::closing_paren_offset(&container)?;
    let args = arg_list
        .args()
        .map(|a| (a.syntax().text_range(), ink_is_pure(a.syntax())))
        .collect();
    Some(ArgsShape {
        zero_start: arg_list.syntax().text_range().start(),
        close_paren,
        args,
    })
}

// ── native frontend ──────────────────────────────────────────────────────

/// Only `CALL_EXPR` — native has no expression-position mutation to guard
/// against. `ASSIGN_STMT` was checked here until a review finding: native's
/// `arg_list` parses each argument with `expression(p)`
/// (`parser/expr.rs::arg_list`), and `ASSIGN_STMT` is only ever produced by
/// the statement dispatcher (`parser/stmt.rs::logic_line`/`assign_stmt`),
/// which nothing in the expression grammar calls into — no block-expression
/// production exists for an argument to smuggle one in through. So
/// `ASSIGN_STMT` can never appear in an argument's subtree, and checking
/// for it was an untested, unreachable defensive branch (native has no
/// `POSTFIX_EXPR` either — no `++`/`--`; ink's own predicate below guards
/// that instead of an assignment shape, for the same "expression-position
/// mutation" reason).
fn native_is_pure(node: &brink_syntax_native::SyntaxNode) -> bool {
    !node
        .descendants()
        .any(|n| n.kind() == brink_syntax_native::SyntaxKind::CALL_EXPR)
}

/// The `(`/`)` boundary of a native `ArgList` — unlike ink's `ArgList`
/// (the parens live on the *parent* node, `FunctionCall`/
/// `DivertTargetWithArgs`), native's own `parser::expr::arg_list` wraps
/// `L_PAREN`/`R_PAREN` as direct tokens of the `ARG_LIST` node itself
/// (`p.expect(L_PAREN)` right after `p.start_node(ARG_LIST)`,
/// `p.expect(R_PAREN)` right before `p.finish_node()`). Returns `(end of
/// the opening "(", byte offset of the start of the closing ")")` — the
/// two anchors [`ArgsShape`] needs. `None` when the parser never
/// consumed one of the two (an unterminated arg list under error
/// recovery): assuming either paren's position would silently mislocate
/// the trim.
fn native_arg_list_bounds(
    arg_list: &brink_syntax_native::ast::ArgList,
) -> Option<(TextSize, usize)> {
    use brink_syntax_native::ast::AstNode as _;
    let tokens: Vec<_> = arg_list
        .syntax()
        .children_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .collect();
    let open_end = tokens
        .iter()
        .find(|t| t.kind() == brink_syntax_native::SyntaxKind::L_PAREN)
        .map(|t| t.text_range().end())?;
    let close_start = tokens
        .iter()
        .rfind(|t| t.kind() == brink_syntax_native::SyntaxKind::R_PAREN)
        .map(|t| usize::from(t.text_range().start()))?;
    Some((open_end, close_start))
}

/// The native counterpart of [`ink_call_is_isolated`] — same reasoning,
/// against `LetStmt`/`AssignStmt`'s own `value()` (a bare `SyntaxNode`,
/// unlike ink's `Expr`-typed accessors — native's own convention, see
/// `LetStmt::value`/`AssignStmt::value`'s doc comments).
fn native_call_is_isolated(call: &brink_syntax_native::ast::CallExpr) -> bool {
    use brink_syntax_native::ast::{AssignStmt, AstNode as _, LetStmt};
    let Some(parent) = call.syntax().parent() else {
        return false;
    };
    match parent.kind() {
        brink_syntax_native::SyntaxKind::LET_STMT => LetStmt::cast(parent)
            .and_then(|l| l.value())
            .is_some_and(|v| v.text_range() == call.syntax().text_range()),
        brink_syntax_native::SyntaxKind::ASSIGN_STMT => AssignStmt::cast(parent)
            .and_then(|a| a.value())
            .is_some_and(|v| v.text_range() == call.syntax().text_range()),
        _ => false,
    }
}

fn native_call_args(source: &str, target_range: TextRange) -> Option<ArgsShape> {
    use brink_syntax_native::ast::{AstNode as _, CallExpr};

    let parsed = brink_syntax_native::parse(source);
    let tree = parsed.tree();
    let root = tree.syntax().clone();

    let call = root
        .descendants()
        .filter_map(CallExpr::cast)
        .find(|c| c.callee().map(|p| p.syntax().text_range()) == Some(target_range))?;
    if !native_call_is_isolated(&call) {
        return None;
    }
    let arg_list = call.arg_list()?;
    let (zero_start, close_paren) = native_arg_list_bounds(&arg_list)?;
    let args = arg_list
        .syntax()
        .children()
        .map(|n| (n.text_range(), native_is_pure(&n)))
        .collect();
    Some(ArgsShape {
        zero_start,
        close_paren,
        args,
    })
}

fn native_divert_args(source: &str, target_range: TextRange) -> Option<ArgsShape> {
    use brink_syntax_native::ast::{AstNode as _, DivertTarget, Splice};

    let parsed = brink_syntax_native::parse(source);
    let tree = parsed.tree();
    let root = tree.syntax().clone();

    let arg_list = root
        .descendants()
        .filter_map(DivertTarget::cast)
        .find_map(|dt| {
            (dt.path().map(|p| p.syntax().text_range()) == Some(target_range))
                .then(|| dt.call_args())
                .flatten()
        })
        .or_else(|| {
            root.descendants().filter_map(Splice::cast).find_map(|s| {
                (s.path().map(|p| p.syntax().text_range()) == Some(target_range))
                    .then(|| s.arg_list())
                    .flatten()
            })
        })?;
    let (zero_start, close_paren) = native_arg_list_bounds(&arg_list)?;
    let args = arg_list
        .syntax()
        .children()
        .map(|n| (n.text_range(), native_is_pure(&n)))
        .collect();
    Some(ArgsShape {
        zero_start,
        close_paren,
        args,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fix::fixes_at;
    use crate::session::IdeSession;

    fn session_with(dialect: brink_analyzer::Dialect, path: &str, src: &str) -> IdeSession {
        let mut session = IdeSession::new();
        session.set_language_dialect(dialect);
        session.update_source(path, src.to_string());
        session.update_and_analyze(path, src.to_string());
        session
    }

    fn applied(src: &str, fix: &Fix) -> String {
        let mut out = src.to_owned();
        let mut edits: Vec<&FileEdit> = fix.edits.iter().collect();
        edits.sort_by_key(|e| std::cmp::Reverse(e.range.start()));
        for e in edits {
            out.replace_range(
                usize::from(e.range.start())..usize::from(e.range.end()),
                &e.new_text,
            );
        }
        out
    }

    // ── E031: ordinary call, ink ──────────────────────────────────────

    const GREET: &str = "=== greet(name) ===\n~ return \"Hi \" + name\n\n";

    #[test]
    fn e031_ink_drops_the_leading_excess_arguments() {
        // `greet` declares one param; the call over-supplies two. The
        // runtime binds the *trailing* supplied arg (this module's doc),
        // so the Safe trim keeps `"Bob"` and removes the leading `"Al"`.
        let src =
            format!("{GREET}=== main ===\n~ temp r = greet(\"Al\", \"Bob\")\n{{r}}\n-> DONE\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("greet(\"Al\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E031);
        assert_eq!(fixes[0].applicability, Applicability::Safe);
        assert_eq!(
            applied(&src, &fixes[0]),
            format!("{GREET}=== main ===\n~ temp r = greet(\"Bob\")\n{{r}}\n-> DONE\n")
        );
    }

    #[test]
    fn e031_no_fix_when_a_dropped_argument_has_a_call() {
        // The leading (dropped) argument here is `sideEffect()` — a call,
        // never provably pure — so no fix is offered rather than silently
        // discarding it (this module's doc: "narrow, never downgrade").
        let side_effecting = format!(
            "{GREET}=== function sideEffect() ===\n~ return \"z\"\n\n=== main ===\n~ temp r = greet(sideEffect(), \"Bob\")\n{{r}}\n-> DONE\n"
        );
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &side_effecting);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(
            side_effecting
                .find("greet(sideEffect()")
                .expect("cursor site"),
        )
        .expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    #[test]
    fn e031_reanalysis_clears_the_diagnostic() {
        let src =
            format!("{GREET}=== main ===\n~ temp r = greet(\"Al\", \"Bob\")\n{{r}}\n-> DONE\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("greet(\"Al\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(&src, &fixes[0]);

        let after = session_with(brink_analyzer::Dialect::Brink, "test.ink", &patched);
        let file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E031),
            "{diags:?}"
        );
    }

    #[test]
    fn e031_no_offer_when_arity_matches() {
        let src = format!("{GREET}=== main ===\n~ temp r = greet(\"Al\")\n-> DONE\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("greet(\"Al\")").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    // ── E176: divert-with-args, ink ────────────────────────────────────

    const ACCUSE: &str = "=== accuse(who) ===\nI accuse {who}!\n-> DONE\n\n";

    #[test]
    fn e176_ink_drops_the_leading_excess_argument() {
        let src = format!("{ACCUSE}=== main ===\n-> accuse(\"Hastings\", \"Poirot\")\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off =
            u32::try_from(src.find("accuse(\"Hastings\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E176);
        assert_eq!(fixes[0].applicability, Applicability::Safe);
        assert_eq!(
            applied(&src, &fixes[0]),
            format!("{ACCUSE}=== main ===\n-> accuse(\"Poirot\")\n")
        );
    }

    #[test]
    fn e176_no_fix_when_a_dropped_argument_has_a_call() {
        let src = format!(
            "{ACCUSE}=== function sideEffect() ===\n~ return \"z\"\n\n=== main ===\n-> accuse(sideEffect(), \"Poirot\")\n"
        );
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off =
            u32::try_from(src.find("accuse(sideEffect()").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    #[test]
    fn e176_reanalysis_clears_the_diagnostic() {
        let src = format!("{ACCUSE}=== main ===\n-> accuse(\"Hastings\", \"Poirot\")\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off =
            u32::try_from(src.find("accuse(\"Hastings\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(&src, &fixes[0]);

        let after = session_with(brink_analyzer::Dialect::Brink, "test.ink", &patched);
        let file = after.file_id("test.ink").expect("file id");
        let diags = after.db().diagnostics(file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E176),
            "{diags:?}"
        );
    }

    #[test]
    fn e176_no_offer_when_arity_matches() {
        let src = format!("{ACCUSE}=== main ===\n-> accuse(\"Hastings\")\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off =
            u32::try_from(src.find("accuse(\"Hastings\")").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    // ── native surface ───────────────────────────────────────────────

    #[test]
    fn e031_native_drops_the_leading_excess_arguments() {
        let src = "fn greet(name) >{\n  return \"Hi \" + name\n}\n\nflow main() {\n  ~ let r = greet(\"Al\", \"Bob\")\n  {r}\n  -> END\n}\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.brink", src);
        let file = session.file_id("test.brink").expect("file id");
        let off = u32::try_from(src.find("greet(\"Al\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E031);
        assert_eq!(fixes[0].applicability, Applicability::Safe);
        assert_eq!(
            applied(src, &fixes[0]),
            "fn greet(name) >{\n  return \"Hi \" + name\n}\n\nflow main() {\n  ~ let r = greet(\"Bob\")\n  {r}\n  -> END\n}\n"
        );
    }

    #[test]
    fn e176_native_drops_the_leading_excess_argument() {
        let src = "flow accuse(who) {\n  I accuse {who}!\n}\n\nflow main() {\n  -> accuse(\"Hastings\", \"Poirot\")\n}\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.brink", src);
        let file = session.file_id("test.brink").expect("file id");
        let off =
            u32::try_from(src.find("accuse(\"Hastings\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E176);
        assert_eq!(fixes[0].applicability, Applicability::Safe);
        assert_eq!(
            applied(src, &fixes[0]),
            "flow accuse(who) {\n  I accuse {who}!\n}\n\nflow main() {\n  -> accuse(\"Poirot\")\n}\n"
        );
    }

    // ── review findings: soundness narrowing ────────────────────────────

    #[test]
    fn e031_no_fix_when_the_call_is_not_the_entire_rhs() {
        // BLOCKING finding: `1 + greet(...)` is not isolated. `greet`
        // leaks its leading arg `"Al"` onto the shared stack beneath its
        // own return value; `+`'s pop then reads that leaked value as its
        // other operand instead of `1` (before any fix: `Add` pops
        // `"Hi Bob"` and `"Al"`, and `1` is left stranded). Trimming the
        // leading arg would remove that leak entirely and compute
        // `1 + "Hi Bob"` instead — a different result from the diagnosed
        // program's actual one, so no fix may be offered here.
        let src =
            format!("{GREET}=== main ===\n~ temp r = 1 + greet(\"Al\", \"Bob\")\n{{r}}\n-> DONE\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("greet(\"Al\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    #[test]
    fn e031_native_no_fix_when_the_call_is_not_the_entire_rhs() {
        // Native counterpart — proves `native_call_is_isolated` guards the
        // same shape (`~ let r = 1 + greet(...)`), not just ink's.
        let src = "fn greet(name) >{\n  return \"Hi \" + name\n}\n\nflow main() {\n  ~ let r = 1 + greet(\"Al\", \"Bob\")\n  {r}\n  -> END\n}\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.brink", src);
        let file = session.file_id("test.brink").expect("file id");
        let off = u32::try_from(src.find("greet(\"Al\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    #[test]
    fn e031_no_fix_when_the_target_declares_a_ref_param() {
        // BLOCKING finding: `heal`'s `ref h` binds by *declared* position
        // (`lower_call_args`) while the runtime binds the *value* by
        // *trailing* position — trimming the leading args re-indexes which
        // supplied argument lands on `h`, flipping write-back. Before any
        // fix, arg0 (`hp`, a pointer) binds nothing since the callee only
        // binds its trailing 2 args, so `{hp}` stays `10`; the withheld
        // trim (`heal(hp, 5)`) would make arg0 bind `h` for real and
        // `{hp}` become `15`. No fix may be offered here.
        let src = "VAR hp = 10\n=== function heal(ref h, amt) ===\n~ h = h + amt\n~ return h\n\n=== main ===\n~ temp r = heal(hp, hp, 5)\n{hp}\n-> DONE\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("heal(hp, hp").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        assert!(fixes_at(&cx, file, off).is_empty());
    }

    // ── review finding: `expected == 0` arm coverage ─────────────────────

    const SHOUT: &str = "=== function shout() ===\n~ return \"Yo!\"\n\n";

    #[test]
    fn e031_ink_zero_params_drops_every_argument() {
        // `build_trim_fix`'s `expected == 0` arm: a zero-declared-param
        // target over-supplied with one argument trims to a bare `()`,
        // and the result must still parse and clear the diagnostic.
        let src = format!("{SHOUT}=== main ===\n~ temp r = shout(\"extra\")\n{{r}}\n-> DONE\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("shout(\"extra\")").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(&src, &fixes[0]);
        assert_eq!(
            patched,
            format!("{SHOUT}=== main ===\n~ temp r = shout()\n{{r}}\n-> DONE\n")
        );

        let after = session_with(brink_analyzer::Dialect::Brink, "test.ink", &patched);
        let after_file = after.file_id("test.ink").expect("file id");
        let parse = brink_syntax::parse(&patched);
        assert!(
            parse.errors().is_empty(),
            "trimmed source must still parse cleanly: {:?}",
            parse.errors()
        );
        let diags = after.db().diagnostics(after_file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E031),
            "{diags:?}"
        );
    }

    #[test]
    fn e031_native_zero_params_drops_every_argument() {
        let src = "fn shout() >{\n  return \"Yo!\"\n}\n\nflow main() {\n  ~ let r = shout(\"extra\")\n  {r}\n  -> END\n}\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.brink", src);
        let file = session.file_id("test.brink").expect("file id");
        let off = u32::try_from(src.find("shout(\"extra\")").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(fixes.len(), 1);
        let patched = applied(src, &fixes[0]);
        assert_eq!(
            patched,
            "fn shout() >{\n  return \"Yo!\"\n}\n\nflow main() {\n  ~ let r = shout()\n  {r}\n  -> END\n}\n"
        );

        let after = session_with(brink_analyzer::Dialect::Brink, "test.brink", &patched);
        let after_file = after.file_id("test.brink").expect("file id");
        let parse = brink_syntax_native::parse(&patched);
        assert!(
            parse.errors().is_empty(),
            "trimmed source must still parse cleanly: {:?}",
            parse.errors()
        );
        let diags = after.db().diagnostics(after_file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E031),
            "{diags:?}"
        );
    }

    #[test]
    fn e176_ink_zero_params_drops_every_argument() {
        // A weave `Label` (`- (lbl)`) declares no params at all — the
        // `E176` shape of the same `expected == 0` arm.
        let src = "=== target ===\n= stitch\n- (lbl)\nHello\n-> DONE\n\n=== main ===\n-> target.stitch.lbl(\"extra\")\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("target.stitch.lbl").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        let patched = applied(src, &fixes[0]);
        assert_eq!(
            patched,
            "=== target ===\n= stitch\n- (lbl)\nHello\n-> DONE\n\n=== main ===\n-> target.stitch.lbl()\n"
        );

        let after = session_with(brink_analyzer::Dialect::Brink, "test.ink", &patched);
        let after_file = after.file_id("test.ink").expect("file id");
        let parse = brink_syntax::parse(&patched);
        assert!(
            parse.errors().is_empty(),
            "trimmed source must still parse cleanly: {:?}",
            parse.errors()
        );
        let diags = after.db().diagnostics(after_file).expect("diagnostics");
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E176),
            "{diags:?}"
        );
    }

    // ── review finding: sibling shapes ───────────────────────────────────

    #[test]
    fn e176_ink_thread_start_drops_the_leading_excess_argument() {
        // `ink_divert_args` handles `ThreadStart` (`<- knot(args)`) as well
        // as a plain divert — previously untested.
        let src = format!("{ACCUSE}=== main ===\n<- accuse(\"Hastings\", \"Poirot\")\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off =
            u32::try_from(src.find("accuse(\"Hastings\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E176);
        assert_eq!(
            applied(&src, &fixes[0]),
            format!("{ACCUSE}=== main ===\n<- accuse(\"Poirot\")\n")
        );
    }

    #[test]
    fn e176_native_splice_drops_the_leading_excess_argument() {
        // `native_divert_args` handles `Splice` (native's `<- knot(args)`
        // inside a `{? … }` choice point) — previously untested.
        let src = "flow options(a, b) {\n  {a} {b}\n}\n\nflow main() {\n  {?\n    <- options(\"gold\", 2, 3)\n  }\n}\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.brink", src);
        let file = session.file_id("test.brink").expect("file id");
        let off = u32::try_from(src.find("options(\"gold\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E176);
        assert_eq!(
            applied(src, &fixes[0]),
            "flow options(a, b) {\n  {a} {b}\n}\n\nflow main() {\n  {?\n    <- options(2, 3)\n  }\n}\n"
        );
    }

    #[test]
    fn e176_ink_tunnel_call_drops_the_leading_excess_argument() {
        // The tunnel-call spelling (`-> knot(args) ->`) reuses the same
        // `DivertTargetWithArgs` node a plain divert does — previously
        // untested as its own shape.
        let src =
            format!("{ACCUSE}=== main ===\n-> accuse(\"Hastings\", \"Poirot\") ->\n-> DONE\n");
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", &src);
        let file = session.file_id("test.ink").expect("file id");
        let off =
            u32::try_from(src.find("accuse(\"Hastings\"").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E176);
        assert_eq!(
            applied(&src, &fixes[0]),
            format!("{ACCUSE}=== main ===\n-> accuse(\"Poirot\") ->\n-> DONE\n")
        );
    }

    #[test]
    fn e176_ink_tunnel_onwards_redirect_drops_the_leading_excess_argument() {
        // Correction (review finding): a tunnel *redirect* (`->->
        // target(args)`) is reachable after all — see this module's
        // "Correction" doc above for why the earlier "left unfixed" claim
        // was wrong, and `brink play` on `before`/`after` (both print `3`)
        // for the empirical proof behind it.
        let src = "-> a ->\n=== a ===\n->-> b(5, 3)\n=== b(x) ===\n{x}\n-> END\n";
        let session = session_with(brink_analyzer::Dialect::Brink, "test.ink", src);
        let file = session.file_id("test.ink").expect("file id");
        let off = u32::try_from(src.find("b(5, 3)").expect("cursor site")).expect("fits");
        let cx = FixCx::new(session.db());
        let fixes = fixes_at(&cx, file, off);
        assert_eq!(
            fixes.len(),
            1,
            "{:?}",
            fixes.iter().map(|f| &f.title).collect::<Vec<_>>()
        );
        assert_eq!(fixes[0].code, DiagnosticCode::E176);
        assert_eq!(
            applied(src, &fixes[0]),
            "-> a ->\n=== a ===\n->-> b(3)\n=== b(x) ===\n{x}\n-> END\n"
        );
    }
}
