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
//! **leading** (dropped) arguments being free of any call or assignment,
//! not the trailing ones.
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
//! `DivertTarget`/`Splice` on native) for `E176`. A tunnel-*redirect*
//! (`->-> target(args)`, whose args live on the `Return` statement rather
//! than on a divert-target node — see
//! `brink_ir::symbols::project::Projector::walk_return`) is not
//! structurally reachable from a plain descendants search for either of
//! those node types and is left unfixed (narrower applicability, not a
//! downgrade — `fixes` simply returns nothing for it).
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
fn expected_param_count(db: &ProjectDb, d: &Diagnostic) -> Option<usize> {
    let (resolutions, _) = db.resolve(d.file)?;
    let target = resolutions.iter().find(|r| r.range == d.range)?.target;
    let index = db.symbol_index();
    Some(index.symbols.get(&target)?.params.len())
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
    /// "Pure" means free of a nested call or assignment anywhere in its
    /// subtree — see this module's doc for why it is the *leading*
    /// arguments (the ones a `Safe` trim here deletes) that must be pure.
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

fn ink_call_args(source: &str, target_range: TextRange) -> Option<ArgsShape> {
    use brink_syntax::ast::{AstNode as _, FunctionCall};

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let root = tree.syntax().clone();

    let fc = root
        .descendants()
        .filter_map(FunctionCall::cast)
        .find(|fc| fc.identifier().map(|i| i.syntax().text_range()) == Some(target_range))?;
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

fn native_is_pure(node: &brink_syntax_native::SyntaxNode) -> bool {
    !node.descendants().any(|n| {
        matches!(
            n.kind(),
            brink_syntax_native::SyntaxKind::CALL_EXPR
                | brink_syntax_native::SyntaxKind::ASSIGN_STMT
        )
    })
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

fn native_call_args(source: &str, target_range: TextRange) -> Option<ArgsShape> {
    use brink_syntax_native::ast::{AstNode as _, CallExpr};

    let parsed = brink_syntax_native::parse(source);
    let tree = parsed.tree();
    let root = tree.syntax().clone();

    let call = root
        .descendants()
        .filter_map(CallExpr::cast)
        .find(|c| c.callee().map(|p| p.syntax().text_range()) == Some(target_range))?;
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
}
