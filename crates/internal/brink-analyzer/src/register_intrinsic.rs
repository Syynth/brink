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
//! # Why this can't reuse `E169`'s "unset `elements`" silence
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

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Knot};
use rowan::TextRange;

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
/// elements` against real module identity is `brink-db`'s job). Unlike that
/// function, `is_conventions_module: false` here still means every
/// `register` call in `hir` is diagnosed — see the module doc for why the
/// two checks' "unconfigured project" postures differ.
///
/// **Shadowing**: `resolve::is_t1b_stdlib_name` only recognizes `register`
/// as the intrinsic on a resolution *failure* — a real top-level
/// declaration of the same name (a `fn`, `EXTERNAL`, `VAR`/`CONST`, or
/// `LIST`) always wins resolution first, exactly like every other T1b
/// name, so calls to it lower as ordinary calls and never reach the
/// intrinsic lowering at all (`lir::lower::expr::lower_expr`'s
/// resolution-map branch runs before its `lower_t1b_stdlib_call`
/// fallback). This pass honors that for a **same-file** shadow — if `hir`
/// itself declares anything named `register`, every bare `register(...)`
/// call in `hir` resolves to that declaration, so this pass stays silent
/// for the whole file rather than raising a false `E175` against calls
/// that were never the intrinsic to begin with. A *cross-file* shadow
/// (declared in a different file, reached however this project's
/// resolution normally reaches other files' symbols) is not checked here
/// — this crate has no project-wide symbol index to consult (see the
/// module doc) — and is a known, narrow gap: a project that shadows
/// `register` from another file could see a spurious `E175` here. This
/// never affects *codegen*: lowering itself is fully resolution-aware and
/// always compiles a real shadow correctly regardless of this pass.
#[must_use]
pub fn register_intrinsic_diagnostics(
    file_id: FileId,
    hir: &HirFile,
    is_conventions_module: bool,
) -> Vec<Diagnostic> {
    if file_declares_register(hir) {
        return Vec::new();
    }

    let mut walker = RegisterCallWalker {
        in_conventions_fn: false,
        calls: Vec::new(),
    };
    visit::visit(hir, &mut walker);

    walker
        .calls
        .into_iter()
        .filter(|call| !(is_conventions_module && call.in_conventions_fn))
        .map(|call| Diagnostic {
            file: file_id,
            range: call.range,
            code: DiagnosticCode::E175,
            message: format!(
                "{}: `register` is a comptime-only intrinsic — legal only inside the \
                 project's configured conventions module's `fn conventions()`",
                DiagnosticCode::E175.title(),
            ),
        })
        .collect()
}

/// Whether `hir` itself declares any top-level symbol literally named
/// `register` — a `fn`/`flow` knot, an `EXTERNAL`, a `VAR`/`CONST`, or a
/// `LIST` — matching `resolve::resolve_function`'s own lookup chain (every
/// category it checks before falling back to the T1b intrinsic list). See
/// [`register_intrinsic_diagnostics`]'s doc for why a same-file shadow
/// must suppress this whole file's check.
fn file_declares_register(hir: &HirFile) -> bool {
    hir.knots
        .iter()
        .any(|k| k.name.text == REGISTER_INTRINSIC_NAME)
        || hir
            .externals
            .iter()
            .any(|e| e.name.text == REGISTER_INTRINSIC_NAME)
        || hir
            .variables
            .iter()
            .any(|v| v.name.text == REGISTER_INTRINSIC_NAME)
        || hir
            .constants
            .iter()
            .any(|c| c.name.text == REGISTER_INTRINSIC_NAME)
        || hir
            .lists
            .iter()
            .any(|l| l.name.text == REGISTER_INTRINSIC_NAME)
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
    use brink_ir::hir::lower_native;

    fn build_native(src: &str) -> HirFile {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, _manifest, _diags) = lower_native::lower(FileId(0), &parsed.tree());
        hir
    }

    #[test]
    fn a_legal_call_inside_conventions_fn_is_silent() {
        let hir = build_native(
            "fn scene(place: string) {\n  return place;\n}\n\
             fn conventions() {\n  register(scene);\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true);
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
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true);
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
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, false);
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
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, false);
        assert_eq!(diags.len(), 1, "{diags:?}");
    }

    #[test]
    fn multiple_illegal_calls_each_get_their_own_diagnostic() {
        let hir = build_native(
            "fn a() {\n  register(x);\n}\n\
             fn b() {\n  register(y);\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E175));
    }

    #[test]
    fn a_same_file_shadowing_register_fn_suppresses_the_whole_file() {
        // A real top-level `fn register(...)` always wins name resolution
        // over the intrinsic (same posture as every other T1b name), so
        // every bare `register(...)` call in this file resolves to it,
        // not to the comptime intrinsic — this pass must stay silent for
        // the whole file rather than raising a false `E175`.
        let hir = build_native(
            "fn register(x: string) {\n  return x;\n}\n\
             fn setup() {\n  register(\"x\");\n}\n\
             flow main() {\n  hi\n}\n",
        );
        let diags = register_intrinsic_diagnostics(FileId(0), &hir, true);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn no_register_calls_at_all_is_always_silent() {
        let hir = build_native("flow main() {\n  hi\n}\n");
        assert!(register_intrinsic_diagnostics(FileId(0), &hir, true).is_empty());
        assert!(register_intrinsic_diagnostics(FileId(0), &hir, false).is_empty());
    }
}
