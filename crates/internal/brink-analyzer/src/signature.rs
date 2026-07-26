//! `signature(def)` — per-declaration signature summary.
//!
//! Phase-0 **stub** of the `signature(DefId)` query (scripting-substrate
//! spec §4, layer 2): it carries only what is already derivable from the
//! declaration itself — name, kind, declared params, the initializer-inferred
//! type (the same inference as [`crate::external_check::infer_value_meta`]),
//! and the `#@local` flow-private bit. No checking, no body analysis. The
//! future type checker reads this as its firewall unit.

use std::sync::Arc;

use brink_format::DefinitionId;
use brink_ir::hir::{Expr, HirFile};
use brink_ir::{FileId, HostManifest, ParamInfo, SymbolIndex, SymbolKind};

use crate::annotations::resolve as resolve_annotation;
use crate::external_check::{InferredType, infer_literal_type};
use crate::infer::Ty;
use crate::resolve::lookup_by_name;

/// Per-declaration signature summary (phase-0 stub).
///
/// Everything here is declaration-derived: a body edit that doesn't touch
/// the declaration line(s) must not change a `Sig`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sig {
    /// Canonical/qualified name, as indexed (e.g. `knot.stitch`).
    pub name: String,
    /// Declaration kind.
    pub kind: SymbolKind,
    /// Declared parameters (knots, stitches, externals).
    pub params: Vec<ParamInfo>,
    /// Type inferred from the initializer literal (VAR/CONST only), if the
    /// initializer is one. TM-2 (docs/typed-mode-spec.md §3): when the
    /// `VAR`/`CONST` also carries a `: type` annotation and that annotation
    /// resolves to a representable [`InferredType`], the annotation
    /// *replaces* this value — annotation wins over inference (the
    /// firewall rule) — so every existing consumer of `value_type`
    /// (notably `infer::collect_globals`) picks up the annotated type
    /// automatically, with no seam change.
    pub value_type: Option<InferredType>,
    /// A VAR/CONST's declaration-derived type at **full [`Ty`] fidelity**
    /// (issue #1540) — `None` for every other symbol kind, and for a
    /// VAR/CONST whose declaration determines no type at all.
    ///
    /// This is the field every *typed* consumer reads (`collect_globals` in
    /// both this crate and `brink-db`'s narrowed mirror); `value_type` above
    /// stays exactly as narrow as it was, because it is the wire-adjacent
    /// domain `infer_value_meta`/hover share and widening it would leak
    /// `Array`/`Map`/`Struct`/`Fn`/`Option`/`Range` into that schema.
    ///
    /// Populated in TM-2 firewall order — annotation first, then the
    /// initializer:
    /// - an explicit `: type` annotation on the VAR/CONST, resolved by
    ///   [`crate::annotations::resolve`] with **no downcast** (so
    ///   `array<int>`, `map<string, int>`, a declared `STRUCT` name,
    ///   `fn(T…): R` and `handle<K>` all survive — this is the
    ///   `ty_to_inferred_type` gap issue #1540 closes). `option<T>` and
    ///   `range` have **no annotation grammar at all**
    ///   ([`crate::annotations::resolve`] has no arm for either), so a
    ///   `Ty::Option`/`Ty::Range` value can never actually reach this
    ///   field yet;
    /// - else the initializer literal, at the same fidelity
    ///   ([`literal_ty`]): `#[…]` → `Ty::Array`, `#{…}` → `Ty::Map`,
    ///   `Name#{…}` → `Ty::Struct`, plus every scalar/`list<L>` form
    ///   `infer_literal_type` already covered;
    /// - else a bare `#fn(target, args…)` initializer (T1c follow-up, issue
    ///   #712, docs/t1c-spec.md §4), in which case the type is the bound
    ///   prefix consumed from `target`'s *own* declaration-derived signature
    ///   (`param_annotations`/`return_annotation` — a second, single-level
    ///   `signature()` call, never the target's body). An unannotated target
    ///   param/return reads as `Ty::Unknown` in the row, the same
    ///   conservative fallback declaration-derived typing always uses.
    pub value_ty: Option<Ty>,
    /// Marked flow-private via a `#@local` directive (knots, stitches, VARs).
    pub is_local: bool,
    /// TM-2 (docs/typed-mode-spec.md §3): declared param type annotations,
    /// positional (parallel to `params`) — `None` per-slot for an
    /// unannotated param, or one whose annotation doesn't resolve to a `Ty`
    /// (`void`, `fn(...)`, an unrecognized name — `crate::check_annotations`
    /// reports those separately). Knots/stitches only; empty for other
    /// symbol kinds.
    pub param_annotations: Vec<Option<Ty>>,
    /// TM-2: the function-header/stitch-header return type annotation
    /// (`): type ===` on a knot, `: type` on a stitch — #1509 widened the
    /// grammar to stitches), resolved. `None` when absent, `void`, or
    /// unresolved. Knots and stitches only.
    pub return_annotation: Option<Ty>,
}

/// Downcast a resolved [`Ty`] to [`InferredType`] where the two universes
/// overlap (`Ty`'s scalar leaves plus nominal `list<L>`) — the annotation-
/// wins substitution for `Sig::value_type`, which predates TM-2 and only
/// has room for `InferredType`'s narrower domain (hover + the `@brink-lang/web`
/// program model read it; widening it would change that schema).
///
/// Every `Ty` outside that overlap — `array<T>`, `map<K, V>`, a nominal
/// `STRUCT`, `fn(T…): R`, `handle<K>`, `option<T>`, `range`, `weighted<T>`,
/// a tower kind — is **not** dropped: since issue #1540 it is carried at
/// full fidelity on [`Sig::value_ty`], which is what `infer::collect_globals`
/// (and `brink-db`'s narrowed mirror of it) reads to give a global its static
/// type. `param_annotations`/`return_annotation`/`crate::resolve_annotation`
/// remain the equivalent full-fidelity surfaces for knots/stitches and for
/// any bare `TypeExpr`.
fn ty_to_inferred_type(ty: &Ty) -> Option<InferredType> {
    match ty {
        Ty::Int => Some(InferredType::Int),
        Ty::Float => Some(InferredType::Float),
        Ty::Bool => Some(InferredType::Bool),
        Ty::String => Some(InferredType::String),
        Ty::Divert => Some(InferredType::Divert),
        Ty::List(name) => Some(InferredType::List(name.clone())),
        // `Conflicted` (#627): a genuine type conflict has no representable
        // `InferredType` any more than `Unknown` does — this stub is a
        // gradual/advisory consumer, so it reads both the same way.
        // Everything below has no `InferredType` variant to downcast to —
        // `Array`/`Map` (T1b), `Struct` (TM-4b), `Fn` (T1c), `Handle`
        // (T1d-2), `Option` (NS-A1), `Range` (NS-A5), `Weighted` (NS-A7),
        // `Tower` (NS-A8). None of them is a silent drop: since issue #1540
        // each is carried whole on `Sig::value_ty` (see this function's doc),
        // which is the field every typed consumer reads. Only the narrow,
        // wire-adjacent `Sig::value_type` stops here.
        Ty::Weighted(_)
        | Ty::Tower(_)
        | Ty::Array(_)
        | Ty::Map(_, _)
        | Ty::Struct(_)
        | Ty::Fn(..)
        | Ty::Handle(_)
        | Ty::Option(_)
        | Ty::Range { .. }
        | Ty::Unknown
        | Ty::Conflicted => None,
    }
}

/// TM-2 firewall rule, shared by `VAR` and `CONST`: if `annotation` resolves
/// to a representable [`InferredType`], it replaces `literal_type`; otherwise
/// the literal-inferred type stands.
fn value_type_with_annotation_override(
    literal_type: Option<InferredType>,
    annotation: Option<&brink_ir::TypeExpr>,
    names: &crate::annotations::TypeNames,
) -> Option<InferredType> {
    annotation
        .and_then(|ann| resolve_annotation(ann, names))
        .and_then(|ty| ty_to_inferred_type(&ty))
        .or(literal_type)
}

/// A VAR/CONST initializer literal's type at full [`Ty`] fidelity (issue
/// #1540) — the collection-aware sibling of
/// [`infer_literal_type`](crate::external_check::infer_literal_type), which
/// stops at [`InferredType`]'s scalar/`list<L>` domain.
///
/// Policy parity with `infer::body`'s own per-body literal arms is
/// deliberate and load-bearing: this declaration-derived stub and the real
/// HM inference must never disagree about what `#[1, 2, 3]` is, or a
/// global and a `temp` holding the same literal would type differently.
/// So the element/key/value joins go through the same
/// [`unify_all`](crate::infer::unify_all) the body arms use (an empty or
/// mixed literal lands on `Ty::Unknown`/`Ty::Conflicted` identically), and
/// a construction literal's type is its written shape name, exactly as
/// `Expr::StructLiteral`'s arm has it — construction *validity* is
/// `crate::structs`' job, not this function's.
///
/// Nested elements recurse through this same function rather than through
/// body inference: a declaration default is constant-folded, so the only
/// element forms that can carry a type here are literals (a call/index/
/// field access in that position is already `E077`).
///
/// Deliberately **not** handled: `Expr::Range`. A range literal never
/// constant-folds into a declaration default at all (`brink-ir`'s
/// `lir::lower::decls::is_const_foldable_decl_default` returns `false` for
/// it), so typing one here would mint `NonEmptyRange` evidence — or its
/// absence — for a declaration that cannot compile. Moot in practice: a
/// `range` *annotation* has no grammar either
/// ([`crate::annotations::resolve`] has no `"range"` arm), so
/// `declared_value_ty`'s annotation branch can't produce `Ty::Range` any
/// more than this literal branch can.
fn literal_ty(expr: &Expr, index: &SymbolIndex) -> Option<Ty> {
    match expr {
        Expr::ArrayLiteral(a) => Some(Ty::Array(Box::new(crate::infer::unify_all(
            a.elements
                .iter()
                .map(|e| literal_ty(e, index).unwrap_or(Ty::Unknown)),
        )))),
        Expr::MapLiteral(m) => {
            let keys = m
                .entries
                .iter()
                .map(|(k, _)| literal_ty(k, index).unwrap_or(Ty::Unknown));
            let vals: Vec<Ty> = m
                .entries
                .iter()
                .map(|(_, v)| literal_ty(v, index).unwrap_or(Ty::Unknown))
                .collect();
            Some(Ty::Map(
                Box::new(crate::infer::unify_all(keys)),
                Box::new(crate::infer::unify_all(vals)),
            ))
        }
        Expr::StructLiteral(sl) => Some(Ty::Struct(sl.shape.text.clone())),
        _ => infer_literal_type(expr, index).map(Ty::from),
    }
}

/// A VAR/CONST's declaration-derived type at full [`Ty`] fidelity — the
/// value behind [`Sig::value_ty`] (issue #1540). Resolution order is the
/// TM-2 firewall's: an explicit annotation wins outright, then the
/// initializer literal, then a `#fn(…)` initializer.
fn declared_value_ty(
    value: &Expr,
    annotation: Option<&brink_ir::TypeExpr>,
    index: &SymbolIndex,
    files: &[(FileId, &HirFile)],
    names: &crate::annotations::TypeNames,
    manifest: Option<&HostManifest>,
) -> Option<Ty> {
    if let Some(ty) = annotation.and_then(|ann| resolve_annotation(ann, names)) {
        return Some(ty);
    }
    literal_ty(value, index).or_else(|| declared_fn_type(value, index, files, manifest))
}

/// A VAR/CONST's declaration-derived `#fn(target, args…)` initializer type
/// (T1c follow-up, issue #712) — `None` when the initializer isn't a
/// `#fn(...)` literal. Feeds [`Sig::value_ty`]'s last fallback, after the
/// annotation and the initializer-literal branches in
/// [`declared_value_ty`] have both already come up empty — an `fn(...)`
/// *annotation* is resolved and returned there directly (that call site's
/// own `if let Some(ty) = annotation.and_then(...)` is strictly earlier in
/// the firewall order and already covers it), so this function only ever
/// needs to look at the initializer.
fn declared_fn_type(
    value: &Expr,
    index: &SymbolIndex,
    files: &[(FileId, &HirFile)],
    manifest: Option<&HostManifest>,
) -> Option<Ty> {
    let Expr::FnLiteral(fl) = value else {
        return None;
    };
    // `#fn` targets are always a single, statically-named function knot —
    // never a stitch (T1c-1's own note: "every stitch target rejects until
    // stitch-functions exist"), never a local (a global initializer has no
    // enclosing body to scope a temp/param against). This is exactly
    // `resolve::resolve_function`'s own first-tried bucket for a bare
    // name, so it agrees with the real resolver for every target that
    // isn't already an E079 error; an E079 target (wrong kind, or no
    // "function" detail) falls through to `None` — the pre-existing
    // Unknown-escape fallback, not a new failure mode.
    let target_name = fl
        .target
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");
    // Signature typing of a global `#fn` initializer has no per-file import
    // context (no enclosing body), so it looks the target up flatly (M-2d
    // import scoping — issue #790 — is inert here: a single-candidate knot
    // hits `lookup_by_name`'s fast path and resolves identically; only the
    // new multi-declared-module homonym case, not reachable from a typed
    // `#fn` target today, would differ).
    let target_def = lookup_by_name(
        index,
        &crate::resolve::ImportScope::default(),
        &target_name,
        &[SymbolKind::Knot],
    )?;
    let target_info = index.symbols.get(&target_def)?;
    if target_info.detail.as_deref() != Some("function") {
        return None;
    }
    // `signature()` is only ever handed a *narrowed* `files` slice by
    // `brink-db`'s per-def `signature_query` (FG-2, single declaring file
    // only — never a whole-project scan, by design). A cross-file `#fn`
    // target's HIR simply isn't in that slice; calling `signature()`
    // anyway would silently return a Sig with empty `param_annotations`
    // (indistinguishable from "declares zero params"), fabricating a wrong
    // arity instead of the honest "can't determine" `None`. The
    // whole-project caller (`infer::collect_globals`, which always passes
    // every file) never hits this guard.
    if !files.iter().any(|&(id, _)| id == target_info.file) {
        return None;
    }
    // Single-level recursion into the *target's* declaration-derived
    // signature only (never its body) — `#fn` targets are always knots, so
    // this never re-enters the `Variable`/`Constant` arms that call
    // `declared_fn_type` in the first place.
    let target_sig = signature(target_def, index, files, manifest)?;
    let remaining: Vec<Ty> = target_sig
        .param_annotations
        .iter()
        .skip(fl.args.len())
        .map(|a| a.clone().unwrap_or(Ty::Unknown))
        .collect();
    let ret = target_sig.return_annotation.clone().unwrap_or(Ty::Unknown);
    Some(Ty::Fn(remaining, Box::new(ret)))
}

/// Compute the signature stub for one definition.
///
/// Reads the declaration only: the indexed `SymbolInfo` plus the declaring
/// file's HIR for the initializer expression and the `#@local` bit. Returns
/// `None` for an unknown definition id.
#[must_use]
#[expect(
    clippy::too_many_lines,
    reason = "each per-kind annotation-resolution branch below (Variable/Constant/ \
              Knot/Stitch) threads the same TypeNames bundle (lists/structs/handles, \
              TM-4b + T1d-2) through its own param/return/value-type resolution — \
              a wide but flat shape, not a new structural concern worth splitting out"
)]
pub fn signature(
    def: DefinitionId,
    index: &SymbolIndex,
    files: &[(FileId, &HirFile)],
    manifest: Option<&HostManifest>,
) -> Option<Arc<Sig>> {
    let info = index.symbols.get(&def)?;
    let hir = files
        .iter()
        .find(|&&(id, _)| id == info.file)
        .map(|&(_, hir)| hir);

    let mut value_type = None;
    let mut value_ty = None;
    let mut is_local = false;
    let mut param_annotations = Vec::new();
    let mut return_annotation = None;
    if let Some(hir) = hir {
        // `list<L>`/declared-struct annotations resolve nominally against
        // every declared `LIST`/`STRUCT` in the project (spec §2/§3, TM-4b
        // §6) — computed lazily, only when this def actually has a
        // knot/stitch/var to annotate below. T1d-2b (issue #774): the
        // registered `HostManifest` now reaches `signature()` too (threaded
        // by `brink-db`'s per-def `signature_query`, the same coarse
        // project-wide dependency shape `per_file_diagnostics_query` already
        // reads `host_manifest` at), so `handle<K>` annotations resolve
        // against the manifest's declared kind vocabulary here exactly like
        // `list<L>`/`STRUCT` resolve against the project's declared names —
        // `None` (no manifest registered) still degrades to an empty
        // handle-kind set, same "unresolved -> silent" contract every other
        // unrecognized name already gets.
        let names = || crate::annotations::TypeNames::new(index, manifest);
        match info.kind {
            SymbolKind::Variable => {
                if let Some(v) = hir.variables.iter().find(|v| v.name.text == info.name) {
                    is_local = v.is_local;
                    // TM-2: annotation wins over the literal-inferred type.
                    value_type = value_type_with_annotation_override(
                        infer_literal_type(&v.value, index),
                        v.annotation.as_ref(),
                        &names(),
                    );
                    // Issue #1540: the full-fidelity type every typed
                    // consumer reads — `Ty::Array`/`Map`/`Struct`/`Fn`/
                    // `Option`/`Range` have no `InferredType` home, so they
                    // ride here. See `Sig::value_ty`.
                    value_ty = declared_value_ty(
                        &v.value,
                        v.annotation.as_ref(),
                        index,
                        files,
                        &names(),
                        manifest,
                    );
                }
            }
            SymbolKind::Constant => {
                if let Some(c) = hir.constants.iter().find(|c| c.name.text == info.name) {
                    // TM-2: annotation wins over the literal-inferred type
                    // (same firewall rule as VAR — see the `Variable` arm).
                    value_type = value_type_with_annotation_override(
                        infer_literal_type(&c.value, index),
                        c.annotation.as_ref(),
                        &names(),
                    );
                    // Issue #1540 — see the `Variable` arm.
                    value_ty = declared_value_ty(
                        &c.value,
                        c.annotation.as_ref(),
                        index,
                        files,
                        &names(),
                        manifest,
                    );
                }
            }
            SymbolKind::Knot => {
                if let Some(k) = hir.knots.iter().find(|k| k.name.text == info.name) {
                    is_local = k.is_local;
                    let names = names();
                    param_annotations = k
                        .params
                        .iter()
                        .map(|p| {
                            p.annotation
                                .as_ref()
                                .and_then(|a| resolve_annotation(a, &names))
                        })
                        .collect();
                    return_annotation = k
                        .return_type
                        .as_ref()
                        .and_then(|rt| resolve_annotation(rt, &names));
                }
            }
            SymbolKind::Stitch => {
                // Indexed stitch names are qualified (`knot.stitch`); HIR
                // nests stitches under their knot by bare name. A top-level
                // stitch is promoted to knot status during HIR lowering.
                if let Some((knot_name, stitch_name)) = info.name.split_once('.') {
                    if let Some(s) = hir
                        .knots
                        .iter()
                        .find(|k| k.name.text == knot_name)
                        .and_then(|k| k.stitches.iter().find(|s| s.name.text == stitch_name))
                    {
                        is_local = s.is_local;
                        let names = names();
                        param_annotations = s
                            .params
                            .iter()
                            .map(|p| {
                                p.annotation
                                    .as_ref()
                                    .and_then(|a| resolve_annotation(a, &names))
                            })
                            .collect();
                        return_annotation = s
                            .return_type
                            .as_ref()
                            .and_then(|rt| resolve_annotation(rt, &names));
                    }
                } else if let Some(k) = hir.knots.iter().find(|k| k.name.text == info.name) {
                    is_local = k.is_local;
                    let names = names();
                    param_annotations = k
                        .params
                        .iter()
                        .map(|p| {
                            p.annotation
                                .as_ref()
                                .and_then(|a| resolve_annotation(a, &names))
                        })
                        .collect();
                    return_annotation = k
                        .return_type
                        .as_ref()
                        .and_then(|rt| resolve_annotation(rt, &names));
                }
            }
            _ => {}
        }
    }

    Some(Arc::new(Sig {
        name: info.name.clone(),
        kind: info.kind,
        params: info.params.clone(),
        value_type,
        value_ty,
        is_local,
        param_annotations,
        return_annotation,
    }))
}
