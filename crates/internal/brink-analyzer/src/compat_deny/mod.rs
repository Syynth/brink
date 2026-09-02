//! The **compat-deny** diagnostic tier (issue #3373, RULED 2026-09-01):
//! "inklecate rejects this; brink can run it; you must opt in."
//!
//! `docs/compiler-spec.md` "Compat-deny diagnostics" owns the tier's
//! mechanics and admission invariant — a code may join only when brink
//! produces a *working* program with the code downgraded (`[lints]` to
//! `warn` or all the way to `allow`). Each member gets its own submodule
//! here, named after what it checks, so the tier can grow without every
//! member's logic piling into one file.
//!
//! Every member is `DiagnosticCode::severity() == Error` (inklecate's own
//! rejection, kept as the default) yet `DiagnosticCode::is_overridable() ==
//! true` — the one deliberate exception to "a hard error can never be
//! downgraded by `[lints]`" (issue #1160). See
//! `brink_ir::hir::DiagnosticCode::is_compat_deny` for the predicate that
//! marks membership, and `brink_analyzer::strict::effective_severity` for
//! where that predicate changes the resolution order.

pub mod knot_temp_from_stitch;
