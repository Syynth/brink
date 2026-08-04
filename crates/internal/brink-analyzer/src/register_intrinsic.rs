//! The *legality* half of issue #1840 Q5 (`docs/decision-log.md` 2026-08-02
//! "`register` is a comptime-only intrinsic; calling it elsewhere is a
//! diagnostic"):
//!
//! > `register` is a T1b intrinsic, legal ONLY inside the conventions
//! > module's `fn conventions()`, where the comptime evaluator intercepts
//! > it during `begin_function_eval`. No opcode, no runtime registry cell,
//! > no bytecode... Calling `register` anywhere else is a compile error.
//!
//! `resolve::is_t1b_stdlib_name` already keeps `register` from raising
//! `E025` anywhere it's called (the same unconditional-by-name, unresolved-
//! call-only posture every other T1b intrinsic gets) — this module is the
//! **separate, narrower** pass that confines where a `register` call is
//! actually *legal*, exactly mirroring how `E169`'s
//! [`crate::conventions_module_diagnostics`] narrows `@[element(claims =
//! "…")]` declarations down to the one confined module on top of a
//! permissive general resolution. `E175` is this pass's diagnostic.
//!
//! # Why this can't reuse `E169`'s "unset `conventions`" silence
//!
//! `conventions_module_diagnostics` stays silent when no conventions module
//! is configured at all — nothing is being confined *to* yet, so a project
//! that hasn't opted in stays exactly as permissive as it always was. That
//! reasoning does not carry over here: `register` is a **comptime-only
//! intrinsic** — a language-level restriction, not a project-configuration-
//! dependent one — so a call to it is illegal precisely when there is no
//! possible legal placement, which includes the "no module configured"
//! case. The caller (`brink-db`) is responsible for computing
//! `is_conventions_module: bool` as `false` for *every* file when no
//! conventions module resolves, rather than skipping this pass entirely
//! (see `register_intrinsic_diagnostics_query`'s own doc).
//!
//! # Scope of this slice
//!
//! This pass answers "is this call legally placed", nothing more. It does
//! not comptime-evaluate `fn conventions()` (issue #1840's remaining
//! slice, not yet built — see `docs/decision-log.md`'s "`brink-compiler`
//! takes `brink-runtime` as a real dependency" entry for the architecture
//! that will), and it does not participate in the ordered-identity-list
//! join `brink_analyzer::conventions_registry` performs. A legally-placed
//! `register` call today compiles cleanly (its interim lowering,
//! `brink_ir::lir::lower::expr::lower_t1b_stdlib_call`'s `"register"` arm,
//! just evaluates and discards its argument) but does not yet feed any
//! registry — that is exactly what the comptime evaluator slice will
//! change, without needing to touch this confinement check again.
//!
//! # `register`'s effect row (issue #1840, registration slice)
//!
//! `docs/decision-log.md`'s 2026-08-01 Q4 ruling settles what `register`
//! *means* in the effect system — a write to a **named registry cell**,
//! the same "ordinary write" shape §10 of `docs/effects-spec.md` already
//! gives every RNG draw — specifically to correct an earlier framing
//! (`@[effects(pure)] fn conventions()`) that failed its own `E103` fence,
//! and to reject the alternative of a bespoke row-exempt intrinsic. This is
//! now wired: `super::infer::intrinsics::intrinsic_effects`'s
//! `conventions_write` bit makes every `register(...)` call write
//! `brink_format::DefinitionId::CONVENTIONS_REGISTRY_CELL`, so
//! `@[effects(pure)] fn conventions() { register(x) }` — the ruled
//! example's original spelling — now genuinely fails `E103` naming
//! `conventions_registry`; `@[effects(writes(conventions_registry))]` is
//! the corrected spelling that passes. This pass (legality/confinement,
//! `E175`) is a separate, unaffected check either way — see
//! `docs/effects-spec.md` §10/§14.5 item 3 and `docs/diagnostics/E175.md`
//! for the full history.

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot, ResolutionMap};
use rowan::TextRange;

use crate::determinism::LookupSet;

/// The well-known function name the conventions module registers handlers
/// from (`docs/decision-log.md`'s 2026-07-31 "Conventions are annotated
/// handlers" ruling, item 5) — a top-level, `is_function` knot only; a
/// stitch or a nested nonstarter can't carry this name's meaning.
const CONVENTIONS_FN_NAME: &str = "conventions";

/// The intrinsic's own name, as it appears in call position.
const REGISTER_INTRINSIC_NAME: &str = "register";

/// Diagnose every `register(...)` call in `hir` that is not legally placed:
/// inside the top-level `fn conventions()` of the project's *configured*
/// conventions module.
///
/// `is_conventions_module` is caller-fed, matching
/// [`crate::conventions_module_diagnostics`]'s own shape exactly — this
/// crate stays project-identity-free (resolving `brink.toml`'s `[project]
/// conventions` against real module identity is `brink-db`'s job). Unlike that
/// function, `is_conventions_module: false` here still means every
/// `register` call in `hir` is diagnosed — see the module doc for why the
/// two checks' "unconfigured project" postures differ.
///
/// **Shadowing**: `resolve::is_t1b_stdlib_name` only recognizes `register`
/// as the intrinsic on a resolution *failure* — a real declaration of the
/// same name (a `fn`, `EXTERNAL`, `VAR`/`CONST`, `LIST`, or a local
/// temp/param) always wins resolution first, exactly like every other T1b
/// name, so calls to it lower as ordinary calls and never reach the
/// intrinsic lowering at all (`lir::lower::expr::lower_expr`'s
/// resolution-map branch runs before its `lower_t1b_stdlib_call`
/// fallback). This pass honors that the same way `dialect_gate::check`
/// honors it for the rest of the T1b stdlib names (`dialect_gate.rs`'s own
/// `is_t1b_stdlib_call_name` check): `resolved` is the project's already-
/// computed resolution result (`brink-db` threads in this file's own
/// [`resolve_file`](crate::resolve::resolve_file) output, the same
/// project-wide-index-backed resolver every other pass reads) — a call
/// whose own range appears in `resolved` resolved to a *real* symbol
/// (same-file **or** cross-file, and locals), so it is never the intrinsic
/// and is filtered out per-call before the confinement check ever sees it.
/// This is deliberately **not** a whole-file suppression: only the
/// individual shadowed call sites are exempt, so a file that both shadows
/// `register` for one call and makes an illegal intrinsic call elsewhere
/// still gets `E175` for the latter.
#[must_use]
pub fn register_intrinsic_diagnostics(
    file_id: FileId,
    hir: &HirFile,
    is_conventions_module: bool,
    resolved: &ResolutionMap,
) -> Vec<Diagnostic> {
    let resolved_ranges: LookupSet<TextRange> = resolved.iter().map(|r| r.range).collect();

    let mut walker = RegisterCallWalker {
        in_conventions_fn: false,
        calls: Vec::new(),
    };
    visit::visit(hir, &mut walker);

    walker
        .calls
        .into_iter()
        .filter(|call| !resolved_ranges.contains(&call.range))
        .filter(|call| !(is_conventions_module && call.in_conventions_fn))
        .map(|call| Diagnostic {
            file: file_id,
            range: call.range,
            code: DiagnosticCode::E175,
            message: DiagnosticCode::E175.title().to_owned(),
        })
        .collect()
}

/// One `register(...)` call site found by [`RegisterCallWalker`].
struct RegisterCall {
    /// The callee's own source range (the `register` token) — matches
    /// `resolve::unresolved_diag`'s anchor for an ordinary unresolved call,
    /// so this diagnostic lands in the same place an `E025` would have.
    range: TextRange,
    /// Whether this call sits inside the top-level `fn conventions()`
    /// currently being walked (file-identity-agnostic — the caller narrows
    /// that with `is_conventions_module`).
    in_conventions_fn: bool,
}

/// Walks the whole HIR tree once, recording every bare `register(...)` call
/// site alongside whether it sits inside a top-level, `is_function` knot
/// literally named `conventions`.
struct RegisterCallWalker {
    in_conventions_fn: bool,
    calls: Vec<RegisterCall>,
}

impl HirVisitor for RegisterCallWalker {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_knot(&mut self, knot: &Knot) {
        self.in_conventions_fn = knot.is_function && knot.name.text == CONVENTIONS_FN_NAME;
    }

    fn exit_knot(&mut self, _knot: &Knot) {
        self.in_conventions_fn = false;
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::Call(path, _args) = expr
            && path.segments.len() == 1
            && path.segments[0].text == REGISTER_INTRINSIC_NAME
        {
            self.calls.push(RegisterCall {
                range: path.range,
                in_conventions_fn: self.in_conventions_fn,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::{DefinitionId, DefinitionTag};
    use brink_ir::hir::lower_native;
    use brink_ir::{ResolvedRef, Stmt};

    fn build_native(src: &str) -> HirFile {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, _manifest, _diags) = lower_native::lower(FileId(0), &parsed.tree());
        hir
    }

    fn no_resolutions() -> ResolutionMap {
        ResolutionMap::new()
    }

    /// Finds the sole `register(...)` call inside a knot named `knot_name`,
    /// unwrapping the `~ { … }` [`brink_ir::LogicBlock`] wrapper a native
    /// `fn` body's logic statements always lower into (`knot.body.stmts`'s
    /// one [`Stmt::LogicBlock`], whose own `stmts` are [`brink_ir::BlockStmt`]
    /// — a different type from the outer `Stmt`).
    fn find_register_call<'a>(hir: &'a HirFile, knot_name: &str) -> &'a Expr {
        let knot = hir
            .knots
            .iter()
            .find(|k| k.name.text == knot_name)
            .unwrap_or_else(|| unreachable!("knot {knot_name:?} must exist in the built HIR"));
        let Some(Stmt::LogicBlock(lb)) = knot.body.stmts.first() else {
            unreachable!("a native `fn` body's statements lower into one LogicBlock");
        };
        let Some(brink_ir::BlockStmt::ExprStmt(expr)) = lb.stmts.first() else {
            unreachable!("the logic block's one statement is the register(...) call");
        };
        expr
    }

    #[test]
    fn a_legal_call_inside_conventions_fn_is_silent() {
        let hir = build_native(
            "fn scene(place: string) {\n  return place;\n}\n\
             fn conventions() {\n  register(scene);\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true, &no_resolutions());
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn a_call_from_a_different_function_is_e175_even_in_the_conventions_module() {
        let hir = build_native(
            "fn scene(place: string) {\n  return place;\n}\n\
             fn setup() {\n  register(scene);\n}\n\
             fn conventions() {\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true, &no_resolutions());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E175);
    }

    #[test]
    fn a_call_from_the_right_function_in_the_wrong_file_is_e175() {
        let hir = build_native(
            "fn scene(place: string) {\n  return place;\n}\n\
             fn conventions() {\n  register(scene);\n}\n\
             flow main() {\n  hi\n}\n",
        );
        // Same source, but this file is NOT the configured conventions
        // module — `is_conventions_module: false` — so even though the
        // call sits inside a function named `conventions`, it's illegal.
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, false, &no_resolutions());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E175);
    }

    #[test]
    fn every_call_is_e175_when_no_conventions_module_is_configured() {
        // Mirrors the "unconfigured project" case the module doc explains:
        // `is_conventions_module` is `false` for every file when nothing is
        // configured, so this call — which would otherwise be legal — is
        // still diagnosed. There is no possible legal placement.
        let hir = build_native(
            "fn scene(place: string) {\n  return place;\n}\n\
             fn conventions() {\n  register(scene);\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, false, &no_resolutions());
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn multiple_illegal_calls_each_get_their_own_diagnostic() {
        let hir = build_native(
            "fn a() {\n  register(x);\n}\n\
             fn b() {\n  register(y);\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true, &no_resolutions());
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E175));
    }

    /// A same-file `register`-named user function shadows the intrinsic —
    /// `resolve_file` resolves the call to that declaration and this pass
    /// reads the resolution result, exactly like `dialect_gate::check`'s own
    /// `resolved_stdlib_name_call_is_never_flagged_in_either_dialect` test
    /// simulates it for the rest of the T1b names. Unlike the old whole-file
    /// `file_declares_register` heuristic, this is a **per-call** filter: it
    /// is proven precise (not merely coarse-safe) below by
    /// `a_shadowed_call_does_not_suppress_an_unrelated_illegal_call_in_the_
    /// same_file`.
    #[test]
    fn a_resolved_call_to_a_real_declaration_is_never_e175() {
        let hir = build_native(
            "fn register(x: string) {\n  return x;\n}\n\
             fn setup() {\n  register(\"x\");\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let Expr::Call(path, _) = find_register_call(&hir, "setup") else {
            unreachable!("setup's one statement is always the register(\"x\") call");
        };
        let resolved = vec![ResolvedRef {
            file: FileId(0),
            range: path.range,
            target: DefinitionId::new(DefinitionTag::Address, 1),
        }];
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true, &resolved);
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// The precision half of the fix (review finding on the old
    /// `file_declares_register`, which suppressed the *entire file* once any
    /// one call shadowed): a call that resolved to a real symbol is exempt,
    /// but an unrelated illegal intrinsic call in the very same file still
    /// raises `E175` — proving the filter is per-call-site, not per-file.
    #[test]
    fn a_shadowed_call_does_not_suppress_an_unrelated_illegal_call_in_the_same_file() {
        let hir = build_native(
            "fn register(x: string) {\n  return x;\n}\n\
             fn setup() {\n  register(\"x\");\n}\n\
             fn other() {\n  register(1);\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let Expr::Call(shadowed_path, _) = find_register_call(&hir, "setup") else {
            unreachable!("setup's one statement is always the register(\"x\") call");
        };
        // Only `setup`'s call resolved (to the real `fn register`); `other`'s
        // `register(1)` call never resolved (nothing but the intrinsic list
        // matches an int argument to `fn register(x: string)`), so it's
        // still illegal.
        let resolved = vec![ResolvedRef {
            file: FileId(0),
            range: shadowed_path.range,
            target: DefinitionId::new(DefinitionTag::Address, 1),
        }];
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true, &resolved);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E175);
    }

    #[test]
    fn no_register_calls_at_all_is_always_silent() {
        let hir = build_native("flow main() {\n  hi\n}\n");
        assert!(
            register_intrinsic_diagnostics(FileId(0), &hir, true, &no_resolutions()).is_empty()
        );
        assert!(
            register_intrinsic_diagnostics(FileId(0), &hir, false, &no_resolutions()).is_empty()
        );
    }
}
