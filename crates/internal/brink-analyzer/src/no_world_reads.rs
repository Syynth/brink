//! `E182`: the `@[convention]` no-world-reads fence (issue #2179, ruled
//! 2026-08-06, `docs/decision-log.md` "No-world-reads fence: analyzer
//! effect-row check; unclassified externals are diagnosed").
//!
//! # The rule
//!
//! A `@[convention]` handler may call pure functions and
//! [`ExternalKind::Effect`]/[`ExternalKind::Presentation`] externals
//! ("commands"), but may **never** read world state. The rule exists
//! because classification must be a pure function of the text: if it
//! depended on game state, the editor could never display it, the
//! projection could never be cached, and explain-match would depend on a
//! save file (the issue's own framing).
//!
//! # Mechanism
//!
//! The issue's own decline comment (#2179, investigated while building
//! #2178/PR #2193) traced the mechanism the original backport comment
//! named — `bevy-brink::capability::compute_container_access` — and found
//! it structurally unusable here: wrong crate/dependency direction (it
//! lives in `bevy-brink`, this check runs in `brink-analyzer` at
//! HIR-lowering time), no source spans on the `EffectRowEntry` it
//! consumes, and it needs a live, host-registered ECS `CapabilityRegistry`
//! that does not exist at compile time. That decline is adopted as the
//! evidentiary record; this module does not resurrect that route.
//!
//! The 2026-08-06 ruling's mechanism instead: compute the handler's
//! **transitive call closure**, classify every `EXTERNAL` the closure
//! reaches via [`ExternalKind`], and diagnose a reachable
//! [`ExternalKind::Query`] (or unclassified [`ExternalKind::Plain`]) at
//! the real call site.
//!
//! This module reuses the exact same call-graph substrate
//! [`crate::infer::effects_project`]'s per-def [`crate::EffectRow`]s are
//! built from — the same `collect_defs` walk `call_edges`/`effects_project`
//! use — rather than inventing a second call-graph traversal. Unlike
//! [`crate::infer::def_body`] (which re-runs `collect_defs` on every call,
//! the right tradeoff for its own one-def-at-a-time salsa query shape),
//! this module calls `collect_defs` exactly **once per [`check`] call** and
//! indexes the result by [`DefinitionId`] — `check`'s own BFS visits many
//! defs per handler and many handlers per project, so paying the
//! whole-project walk on every visited node would be quadratic in the
//! project's call-graph size. Same for the per-file resolution index
//! (`crate::infer::index_resolutions_by_file`): built once, not rebuilt by
//! filtering the whole project's [`ResolutionMap`] on every node. What this
//! module does NOT reuse is the
//! aggregated [`crate::EffectRow`]/[`crate::EffectAtoms`] shape itself:
//! both are **sets** (`calls: BTreeSet<String>` — external binding
//! *names*, already transitively closed, but with no site attached), and
//! the decline comment's own finding is that nothing in that shape carries
//! a span — exactly what this issue's own scope item ("a diagnostic that
//! points at the offending call") needs. So this walks bodies directly
//! (via [`brink_ir::hir::visit::walk_block`], the same structural walker
//! every other span-bearing HIR pass in this crate uses — see
//! [`crate::fn_values`] for the precedent) to recover the real call-site
//! range, breadth-first over the reachable definitions, guarded by a
//! visited set (CLAUDE.md's "guard against unbounded growth" — a
//! project's call graph is finite, but an unguarded walk over a cyclic one
//! is not).
//!
//! # `Plain` policy
//!
//! An `EXTERNAL` with neither an inline `@kind` doc tag nor a registered
//! manifest entry has no [`crate::SymbolMeta`] entry at all
//! ([`crate::external_check::analyze_externals`]'s own `continue` when
//! both `inline` and `reg` are `None`) — so it classifies as
//! [`ExternalKind::Plain`] (the enum's own `#[default]`) by simple
//! absence, with no special-casing needed here. Per the ruling: "an
//! unclassified external called from a convention handler is diagnosed
//! too — unprovable is not passable." This promotes [`ExternalKind`] from
//! advisory to load-bearing (the ruling's own words) — every construction
//! site across this crate that used to default silently to `Plain` now
//! has a real consumer that cares which externals stay unclassified.
//!
//! # What is NOT covered (explicitly out of this issue's scope)
//!
//! - A call **through a function value** (a param, a `VAR`/`CONST` cell, a
//!   `#fn`-created local) never appears as a literal `Expr::Call` naming
//!   the external, so this walk cannot see it — the same opaque-call gap
//!   [`crate::EffectRow::opaque`] documents for effect inference generally
//!   (spec §6.2/§6.3). Closing it is a widening of this same mechanism,
//!   not a new one, and is left for a follow-up if it proves reachable in
//!   practice.
//! - **A divert/tunnel-call/thread-start *to a knot or stitch* that itself
//!   calls a `Query`/`Plain` external.** It is true, as originally noted
//!   here, that an `EXTERNAL` binding is never itself invoked as a divert
//!   target — [`Stmt::Divert`]/[`Stmt::TunnelCall`]/[`Stmt::ThreadStart`]
//!   never name an `EXTERNAL` directly. But that is not the whole gap: the
//!   BFS below only enqueues a callee discovered as a literal
//!   [`Expr::Call`] (via [`CallSiteCollector`]), so a handler that reaches a
//!   world-reading external *only* by diverting/tunnel-calling/threading
//!   into a knot or stitch that then calls it is invisible to this check
//!   today — the closure never follows that edge. Left as a known gap
//!   (same "reachable in practice?" bar as the opaque-call gap above), not
//!   silently assumed covered.
//!
//! # Reserved-root (`std::`) files are NOT exempt
//!
//! Unlike [`crate::conventions_confinement`]'s `E169` (which exempts a
//! reserved peer root — `std::…`,
//! [`brink_ir::symbols::is_reserved_root_module`] — because *that* check is
//! about which module a claiming handler must be declared in, and the
//! mounted std preset was never authored to live in a project's own
//! configured module), this check is about whether a handler's call
//! closure ever reads world state — a property of the handler's *body*,
//! not of which file it happens to be mounted from. A `std::` preset
//! handler that reaches a `Query`/`Plain` external is just as illegal as a
//! project-authored one, and correctly so: `std::conventions::screenplay`'s
//! own `heading` handler calls the `scene_entered` host extern (issue
//! #2092), which must be classified `Effect` (an inline `@kind effect` doc
//! tag on the `extern`) for exactly this reason — not exempted from the
//! fence, but proven legal under it.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Diagnostic, DiagnosticCode, Expr, ExternalKind, FileId, HirFile, ResolutionMap, SymbolIndex,
    SymbolKind,
};
use rowan::TextRange;

use crate::external_check::SymbolMeta;
use crate::infer::{collect_defs, index_resolutions_by_file};

/// `(start, end)` key for range-indexed lookups (`TextRange` has no `Ord`) —
/// the same convention [`crate::fn_values`]/[`crate::await_purity`] use.
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// Diagnose every `@[convention]` handler declared in `hir` whose
/// transitive call closure reaches a world-reading (`Query`) or
/// unclassified (`Plain`) `EXTERNAL`.
///
/// `hir`/`file` are the file being checked — handlers only ever live in
/// the project's one configured conventions module
/// ([`crate::conventions_confinement`]'s `E169`), so this only does real
/// work for that file; every other file's `hir.claim_handlers` is empty
/// and this returns immediately. `project_files` is the whole project's
/// HIR (a handler's callees, transitively, may live in other files) and
/// `resolutions` the whole project's resolution map — both the same
/// project-wide inputs [`crate::infer::effects_project`] itself takes.
/// `symbol_meta` is the merged externals table
/// ([`crate::external_check::analyze_externals`]'s output, already
/// computed earlier in [`crate::whole_project_diagnostics`]) this reads
/// each external's [`ExternalKind`] from.
#[must_use]
pub fn check(
    file: FileId,
    hir: &HirFile,
    project_files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
    symbol_meta: &BTreeMap<DefinitionId, SymbolMeta>,
) -> Vec<Diagnostic> {
    if hir.claim_handlers.is_empty() {
        return Vec::new();
    }

    // Declaration-range -> DefinitionId, this file only: a claiming
    // handler's own `fn` is always indexed as a `Knot` (native `fn` lowers
    // to `SymbolKind::Knot`, `detail == Some("function")` —
    // `SymbolInfo::is_function_definition`), keyed by its name's own
    // declaration range, which is exactly `ClaimHandlerDecl::name.range`.
    let handler_defs: BTreeMap<(u32, u32), DefinitionId> = index
        .symbols
        .values()
        .filter(|s| s.file == file && s.kind == SymbolKind::Knot)
        .map(|s| (range_key(s.range), s.id))
        .collect();

    // Hoisted once for the whole `check` call (this module's own doc,
    // "Mechanism") — the BFS below visits many defs per handler and many
    // handlers per project; rebuilding either map per visited node would
    // pay a whole-project walk on every step instead of once total.
    let defs_by_id: BTreeMap<DefinitionId, _> = collect_defs(project_files, index)
        .into_iter()
        .map(|d| (d.id, d))
        .collect();
    let resolutions_by_file = index_resolutions_by_file(resolutions);

    let mut out = Vec::new();
    // Dedup across handlers sharing a helper: two handlers reaching the
    // same illegal call through a common callee must not double-report it.
    let mut diagnosed: BTreeSet<(FileId, (u32, u32))> = BTreeSet::new();
    // Shared across every handler in this `check` call, not reset per
    // handler: a def already fully explored (via an earlier handler's
    // closure) needs no second walk — any violation inside it is already
    // in `diagnosed`, keyed on the real call site, not on which handler
    // found it first, so re-walking a shared callee per handler would only
    // waste work, never change the output.
    let mut visited: BTreeSet<DefinitionId> = BTreeSet::new();
    let mut queue: VecDeque<DefinitionId> = VecDeque::new();

    for handler in &hir.claim_handlers {
        let Some(&handler_def) = handler_defs.get(&range_key(handler.name.range)) else {
            // Should not happen (the annotation validated during collection
            // implies the `fn` is indexed) — silently skip rather than
            // panic on a coordination gap between two passes.
            continue;
        };

        if !visited.insert(handler_def) {
            // Already explored (as some other handler's transitive
            // callee); its closure's violations, if any, are already
            // reported.
            continue;
        }
        queue.push_back(handler_def);

        while let Some(def) = queue.pop_front() {
            let Some(d) = defs_by_id.get(&def) else {
                continue;
            };
            let def_file = d.file;
            let Some(by_range) = resolutions_by_file.get(&def_file) else {
                continue;
            };

            let mut collector = CallSiteCollector {
                by_range,
                sites: Vec::new(),
            };
            visit::walk_block(d.body, &mut collector);

            for (call_range, target) in collector.sites {
                match index.symbols.get(&target).map(|s| s.kind) {
                    Some(SymbolKind::External) => {
                        let kind = symbol_meta.get(&target).map(|m| m.kind).unwrap_or_default();
                        if !matches!(kind, ExternalKind::Query | ExternalKind::Plain) {
                            continue;
                        }
                        if !diagnosed.insert((def_file, range_key(call_range))) {
                            continue;
                        }
                        let ext_name = index.symbols.get(&target).map_or("?", |s| s.name.as_str());
                        let (what, fix) = match kind {
                            ExternalKind::Query => (
                                "a `Query`-kind external (a world read)",
                                "call sites that must read world state belong outside \
                                 `@[convention]` handlers",
                            ),
                            _ => (
                                "an unclassified (`Plain`-kind) external — unprovable is not \
                                 passable",
                                "classify it with an inline `@kind` doc tag or a registered \
                                 host manifest entry",
                            ),
                        };
                        out.push(Diagnostic {
                            file: def_file,
                            range: call_range,
                            code: DiagnosticCode::E182,
                            message: format!(
                                "{}: `{}`'s call to `{ext_name}` reaches {what} — \
                                 `@[convention]` handlers may call pure functions and \
                                 commands, but must never read world state ({fix})",
                                DiagnosticCode::E182.title(),
                                handler.name.text,
                            ),
                        });
                    }
                    Some(SymbolKind::Knot | SymbolKind::Stitch) if visited.insert(target) => {
                        queue.push_back(target);
                    }
                    _ => {}
                }
            }
        }
    }

    out
}

/// Collects every `Expr::Call`'s callee range + resolved target within one
/// walked block/body — the span-bearing fact neither [`crate::EffectRow`]
/// nor [`crate::EffectAtoms`] carries (this module's own doc, "Mechanism").
struct CallSiteCollector<'a> {
    by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    sites: Vec<(TextRange, DefinitionId)>,
}

impl HirVisitor for CallSiteCollector<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::Call(path, _args) = expr
            && let Some(&target) = self.by_range.get(&range_key(path.range))
        {
            self.sites.push((path.range, target));
        }
    }
}

#[cfg(test)]
mod tests {
    use brink_ir::hir::lower_native;
    use brink_ir::{DiagnosticCode, FileId, HostManifest};

    use crate::{AnalysisOptions, AnalysisResult, analyze_with_options};

    /// Lower `src` as a single-file native project and run it through the
    /// full [`analyze_with_options`] pipeline — the same monolithic path
    /// `query_equivalence.rs` pins against the salsa-composed one, so a
    /// test passing here is evidence the real `whole_project_diagnostics`
    /// wiring fires, not just this module's own `check` in isolation.
    fn analyze(src: &str, host_manifest: Option<HostManifest>) -> AnalysisResult {
        let parsed = brink_syntax_native::parse(src);
        assert!(parsed.errors().is_empty(), "{:?}", parsed.errors());
        let (hir, manifest, diags) = lower_native::lower(FileId(0), &parsed.tree());
        assert!(diags.is_empty(), "{diags:?}");
        let opts = AnalysisOptions {
            host_manifest,
            ..AnalysisOptions::default()
        };
        analyze_with_options(&[(FileId(0), &hir, &manifest)], &opts)
    }

    fn e182(result: &AnalysisResult) -> Vec<&brink_ir::Diagnostic> {
        result
            .diagnostics
            .iter()
            .filter(|d| d.code == DiagnosticCode::E182)
            .collect()
    }

    #[test]
    fn handler_calling_a_pure_fn_is_legal() {
        let src = "@[convention(claims = \"^X$\", order = 10)]\n\
                   fn handler() {\n  return helper();\n}\n\
                   fn helper() {\n  return 1;\n}\n\
                   flow main() {\n  hi\n}\n";
        let result = analyze(src, None);
        assert!(e182(&result).is_empty(), "{:?}", result.diagnostics);
    }

    #[test]
    fn handler_calling_an_effect_external_is_legal() {
        let src = "/// @kind effect\n\
                   extern play_sound(name)\n\n\
                   @[convention(claims = \"^X$\", order = 10)]\n\
                   fn handler() {\n  play_sound(\"ding\");\n  return \"ok\";\n}\n\
                   flow main() {\n  hi\n}\n";
        let result = analyze(src, None);
        assert!(e182(&result).is_empty(), "{:?}", result.diagnostics);
    }

    #[test]
    fn handler_calling_a_query_external_directly_is_diagnosed() {
        let src = "/// @kind query\n\
                   extern get_health()\n\n\
                   @[convention(claims = \"^X$\", order = 10)]\n\
                   fn handler() {\n  return get_health();\n}\n\
                   flow main() {\n  hi\n}\n";
        let result = analyze(src, None);
        let found = e182(&result);
        assert_eq!(found.len(), 1, "{:?}", result.diagnostics);
        assert!(found[0].message.contains("get_health"), "{found:?}");
        assert!(found[0].message.contains("handler"), "{found:?}");
    }

    #[test]
    fn handler_calling_an_unclassified_external_directly_is_diagnosed() {
        // No doc comment, no manifest entry — `ExternalKind::Plain` by
        // simple absence from `symbol_meta` (this module's own doc,
        // "`Plain` policy").
        let src = "extern mystery()\n\n\
                   @[convention(claims = \"^X$\", order = 10)]\n\
                   fn handler() {\n  return mystery();\n}\n\
                   flow main() {\n  hi\n}\n";
        let result = analyze(src, None);
        let found = e182(&result);
        assert_eq!(found.len(), 1, "{:?}", result.diagnostics);
        assert!(found[0].message.contains("mystery"), "{found:?}");
    }

    #[test]
    fn transitive_query_through_a_helper_fn_is_diagnosed_at_the_real_call_site() {
        // The closure is the point: `handler` never calls `get_health`
        // directly, only through `helper` — a direct-call-only check would
        // miss this (issue #2179's own required test).
        let src = "/// @kind query\n\
                   extern get_health()\n\n\
                   @[convention(claims = \"^X$\", order = 10)]\n\
                   fn handler() {\n  return helper();\n}\n\
                   fn helper() {\n  return get_health();\n}\n\
                   flow main() {\n  hi\n}\n";
        let result = analyze(src, None);
        let found = e182(&result);
        assert_eq!(found.len(), 1, "{:?}", result.diagnostics);
        // Anchored at the real call site inside `helper`'s own body (the
        // second `get_health()` occurrence — the first is the `extern`
        // declaration itself), not at `handler`'s call to `helper`.
        let call_start =
            u32::try_from(src.find("return get_health()").expect("call site") + "return ".len())
                .expect("offset fits u32");
        assert_eq!(found[0].range.start(), call_start.into(), "{found:?}");
    }
}
