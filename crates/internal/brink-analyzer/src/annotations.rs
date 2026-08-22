//! TM-2 inline type annotation resolution + `signature()` firewall wiring
//! (docs/typed-mode-spec.md §3).
//!
//! Two independent jobs, both consuming already-lowered HIR (never touching
//! `infer::body`'s `BodyCtx` — that rework is fenced off, #638):
//!
//! - [`resolve`]: turn a parsed [`brink_ir::TypeExpr`] into the checker's
//!   [`Ty`] universe, for `signature()` to carry as its firewall (annotation
//!   wins over inference — see `crate::signature::Sig`'s `param_annotations`/
//!   `return_annotation` fields, populated via this function).
//! - [`check`]: semantic diagnostics on the annotation *content* — unknown
//!   type names (`E061`). Runs only under the brink dialect
//!   (`finish_analysis` gates the call): under `strict-ink`,
//!   `dialect_gate` already rejects the annotation whole as extension
//!   syntax (`E051`), and content diagnostics on rejected syntax are noise
//!   (maintainer ruling 2026-07-13). `fn(T…): R` types, formerly reserved
//!   (`E062`, retired with T1c-1), are legal and resolve to [`Ty::Fn`].
//!
//! [`mismatches`] is the third job: the annotation-vs-body-inference
//! diagnostic (`E063`), composing `signature()`'s annotations with
//! `infer_project`'s already-computed body-derived types — a pure consumer
//! of both public seams, touching neither's internals (per the fence: "no
//! changes to the FG query decomposition beyond consuming its public seam").
//!
//! [`check_reserved_type_names`] is the fourth job (issue #1865): a
//! declaration-site diagnostic (`E188`) for a `STRUCT` whose own name
//! collides with one of [`resolve`]'s builtin-leaf/tower-kind names — the
//! reserved set [`resolve`] itself checks before ever consulting
//! `names.structs`.

use std::collections::BTreeSet;

use brink_format::DefinitionId;
use brink_ir::{
    BaseType, Diagnostic, DiagnosticCode, FileId, HirFile, HostManifest, Knot, Stitch, SymbolIndex,
    SymbolKind,
};

use crate::infer::{InferenceResult, Ty};
use crate::resolve::ImportScope;

/// Recognized bare nominal leaf names (typed-mode-spec §3): everything except
/// the generic heads (`List`/`Array`/`Map`/`Option`/`Weighted`/`Handle`) and
/// the reserved function-type keyword (`fn`), which are grammar/semantic
/// concerns of their own.
fn is_known_leaf(name: &str) -> bool {
    matches!(
        name,
        "int"
            | "float"
            | "bool"
            | "string"
            | "divert"
            | "void"
            // NS-A8 (`docs/tower-mini-spec.md`): the tower kinds are
            // global type names like `int` (stdlib-spec §2b).
            | "vec2"
            | "vec3"
            | "vec4"
            | "quat"
            | "mat2"
            | "mat3"
            | "mat4"
            // issue #1846, `docs/prose-dialect-spec.md` §3.5b's capture
            // contract: a first-class content value, fragment-capture
            // backed — the type of a `content`-typed parameter (e.g.
            // `fn radio(chan: string, text: content)`). A global leaf name
            // like every other scalar above, not a declared/nominal vocab
            // entry.
            | "content"
    )
}

/// The three name vocabularies a type annotation resolves nominal generics
/// against — `List<L>`/`STRUCT` names come from ink source (the project's
/// `SymbolIndex`), `Handle<K>` kinds come from the registered host manifest
/// (T1d-2, docs/t1d-spec.md §3: "Handle kinds live in the external manifest
/// — the existing host semantic-type vocabulary the analyzer already
/// polices — not in the format"). Bundled together because every existing
/// call site already threads `list_names`/`struct_names` as an inseparable
/// pair; `handles` joins that pair rather than becoming a fourth parameter
/// sprinkled through every signature in this crate.
#[derive(Debug, Clone, Default)]
pub struct TypeNames {
    pub lists: BTreeSet<String>,
    pub structs: BTreeSet<String>,
    pub handles: BTreeSet<String>,
}

impl TypeNames {
    /// Build the full bundle for a project: declared `LIST`/`STRUCT` names
    /// from the symbol index, declared handle kinds from the registered host
    /// manifest (`None` when no manifest is registered — an empty
    /// vocabulary, matching how an unregistered manifest degrades every
    /// other manifest-driven check).
    #[must_use]
    pub fn new(index: &SymbolIndex, manifest: Option<&HostManifest>) -> Self {
        Self {
            lists: declared_list_names(index),
            structs: declared_struct_names(index),
            handles: declared_handle_kinds(manifest),
        }
    }
}

/// Every declared handle-kind name in the registered host manifest (T1d-2):
/// a [`brink_ir::SemanticTypeDef`] whose `base` is [`BaseType::Handle`] — its
/// `name` field *is* the kind name `Handle<K>` annotations resolve `K`
/// against (`host_manifest.rs`'s `BaseType::Handle` doc). Empty when no
/// manifest is registered, same degrade-gracefully posture as
/// `external_check`'s semantic-type resolution (issue #339).
#[must_use]
pub fn declared_handle_kinds(manifest: Option<&HostManifest>) -> BTreeSet<String> {
    manifest
        .map(|m| {
            m.types
                .iter()
                .filter(|t| t.base == BaseType::Handle)
                .map(|t| t.name.clone())
                .collect()
        })
        .unwrap_or_default()
}

/// Resolve a parsed type annotation into the checker's `Ty` universe.
///
/// Returns `None` for `void` (no `Ty` — return-position-only, handled
/// separately by callers that care) and any name this function doesn't
/// recognize (an unknown leaf name, or a `List<L>` whose `L` isn't a
/// declared `LIST` — [`check`] is what reports these, not this function).
///
/// `fn(T…): R` (T1c, docs/t1c-spec.md §4 — the boundary-annotation form)
/// resolves to [`Ty::Fn`] when every param and the return resolve; a
/// `void` return inside a fn type is unsupported in this slice (the type
/// universe has no void — the whole annotation resolves `None`, i.e. is
/// treated as absent, same contract as any other unresolvable component).
///
/// `names.structs` (TM-4b, docs/typed-mode-spec.md §6): a bare `Named` type
/// whose name is a declared `STRUCT` resolves to `Ty::Struct` — "declared
/// struct names join the TM-2 annotation type grammar", the same join
/// `names.lists` gives `List<L>`. Checked after the fixed scalar-keyword set
/// so a struct can never shadow `int`/`float`/etc. (those names aren't
/// legal `STRUCT` identifiers by convention, but this ordering is the
/// unambiguous choice regardless).
///
/// `names.handles` (T1d-2, docs/t1d-spec.md §3): `Handle<K>` resolves to
/// `Ty::Handle(K)` when `K` names a declared handle kind — the manifest
/// mirror of `List<L>`'s ink-source-declared vocabulary.
///
/// `Option<T>`/`Weighted<T>` (issue #1552, `docs/decision-log.md`
/// 2026-07-27 "Type-name surface ruled"): the annotation mirror of
/// `Ty::Option`/`Ty::Weighted`, which previously had no spelling on this
/// surface at all — resolve pointwise on the single element, exactly like
/// `Array<T>`.
#[must_use]
pub fn resolve(te: &brink_ir::TypeExpr, names: &TypeNames) -> Option<Ty> {
    match te {
        brink_ir::TypeExpr::Named { name, .. } => match name.as_str() {
            "int" => Some(Ty::Int),
            "float" => Some(Ty::Float),
            "bool" => Some(Ty::Bool),
            "string" => Some(Ty::String),
            // issue #1846: the capture-contract's `content` leaf resolves
            // to `Ty::Content` — a distinct nominal leaf from `string` (see
            // that variant's doc for why it must never coerce to one).
            "content" => Some(Ty::Content),
            "divert" => Some(Ty::Divert),
            // NS-A8 tower kinds — checked before the struct lookup, so a
            // STRUCT can never shadow a tower type name (the same ordering
            // that keeps `int`/`float` unshadowable).
            _ if crate::infer::TowerTy::from_name(name).is_some() => {
                crate::infer::TowerTy::from_name(name).map(Ty::Tower)
            }
            _ if names.structs.contains(name) => Some(Ty::Struct(name.clone())),
            _ => None, // "void", or an unrecognized/unknown name
        },
        brink_ir::TypeExpr::Generic { name, args, .. } => match name.as_str() {
            "List" if args.len() == 1 => match &args[0] {
                brink_ir::TypeExpr::Named { name: l, .. } if names.lists.contains(l) => {
                    Some(Ty::List(l.clone()))
                }
                _ => None,
            },
            "Handle" if args.len() == 1 => match &args[0] {
                brink_ir::TypeExpr::Named { name: k, .. } if names.handles.contains(k) => {
                    Some(Ty::Handle(k.clone()))
                }
                _ => None,
            },
            "Array" if args.len() == 1 => resolve(&args[0], names).map(|t| Ty::Array(Box::new(t))),
            "Map" if args.len() == 2 => {
                let k = resolve(&args[0], names)?;
                let v = resolve(&args[1], names)?;
                Some(Ty::Map(Box::new(k), Box::new(v)))
            }
            "Option" if args.len() == 1 => {
                resolve(&args[0], names).map(|t| Ty::Option(Box::new(t)))
            }
            "Weighted" if args.len() == 1 => {
                resolve(&args[0], names).map(|t| Ty::Weighted(Box::new(t)))
            }
            _ => None,
        },
        brink_ir::TypeExpr::Fn { params, ret, .. } => {
            let params: Option<Vec<Ty>> = params.iter().map(|p| resolve(p, names)).collect();
            let ret = resolve(ret, names)?;
            // The effect row is the top element: a written `fn(T…): R`
            // annotation names no creation target, so it carries no evidence
            // about where the values reaching the slot were made (issue
            // #1680, `docs/effects-spec.md` §6.1a — creation sites are
            // syntactic `#fn` literals, never annotations). Conservative by
            // construction, and the reason assignability has to ignore rows:
            // an annotated param's top row would otherwise never equal the
            // join with a real argument's row (see `infer::assignable`).
            Some(Ty::Fn(
                params?,
                Box::new(ret),
                crate::infer::FnRow::unknown(),
            ))
        }
    }
}

/// The exact name set `resolve`'s `TypeExpr::Named` arm resolves *before* it
/// ever consults `names.structs` (issue #1865) — every name here always wins
/// a bare type-annotation resolution, no matter what `names.structs`
/// declares. Kept as its own function (rather than inlined at
/// [`check_reserved_type_names`]'s one call site) so its doc can carry the
/// full "what's deliberately NOT included and why" accounting in one place.
///
/// Mirrors `resolve`'s own literal arms exactly — `int`/`float`/`bool`/
/// `string`/`content`/`divert` — plus the NS-A8 tower-kind catch-all
/// (`crate::infer::TowerTy::from_name`). Deliberately excludes:
///
/// - **`void`**: unlike the leaves above, `resolve`'s `Named` arm has no
///   explicit `"void"` case at all — an unmatched name falls straight to
///   the struct-lookup arm, so a `STRUCT` named `void` resolves to
///   `Ty::Struct("void")` exactly like any other declared name (pinned by
///   `struct_named_void_is_not_shadowed_and_resolves_fine`, this file's own
///   test module) — there is no collision to warn about.
/// - **The generic heads** (`List`/`Array`/`Map`/`Option`/`Weighted`/
///   `Handle`): these are special-cased only inside
///   `TypeExpr::Generic`'s own `name` dispatch (`Array<T>`) — a *bare*
///   `Named` reference to a struct sharing one of those names (`f: Array`,
///   no `<...>`) still falls through to the ordinary
///   `names.structs.contains(name)` arm and resolves correctly (pinned by
///   `struct_named_array_stays_reachable_and_is_not_flagged`) — again, no
///   real collision.
/// - **Declared `LIST` names / registered `Handle<K>` kinds**: `names.lists`/
///   `names.handles` are only ever consulted inside `List<L>`/`Handle<K>`'s
///   own generic-argument position, never against a bare `Named`
///   annotation — a different namespace from `names.structs`, so a struct
///   sharing a `LIST`/handle-kind name has nothing to collide with here.
fn is_reserved_before_struct_lookup(name: &str) -> bool {
    matches!(
        name,
        "int" | "float" | "bool" | "string" | "content" | "divert"
    ) || crate::infer::TowerTy::from_name(name).is_some()
}

/// Issue #1865: `resolve`'s `TypeExpr::Named` arm checks the builtin-leaf/
/// tower-kind name set BEFORE it ever consults `names.structs` (see that
/// function's own doc — "the same ordering that keeps int/float
/// unshadowable"). That ordering stays exactly as it is; this check does
/// not re-order resolution. What it adds is the missing diagnostic: a
/// declared `STRUCT` whose own name collides with one of those reserved
/// names is silently unreachable through a bare type annotation — every
/// `content`-typed annotation, say, resolves to the builtin `Ty::Content`,
/// never to the struct — and before this check existed, nothing said so in
/// either direction (no diagnostic at the struct declaration, none at any
/// annotation site either).
///
/// Fires once per declared `STRUCT` whose name is in
/// [`is_reserved_before_struct_lookup`]'s set — see that function's own doc
/// for the full "what's deliberately not covered and why" accounting
/// (generic heads, `void`, `LIST`/`Handle` names all stay clean, and are
/// pinned as such by this file's own tests).
///
/// Warning-tier (`DiagnosticCode::E188`'s own doc): the declaration is not
/// rejected — it still compiles, and a construction literal
/// (`content#{...}`) still reaches the struct correctly, since
/// `resolve::resolve_struct_ref`/`resolve_type_ref` never consult this
/// same builtin/tower precedence at all. Only the annotation spelling is
/// shadowed.
#[must_use]
pub fn check_reserved_type_names(files: &[(FileId, &HirFile)]) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        for s in &hir.structs {
            let name = s.name.text.as_str();
            if is_reserved_before_struct_lookup(name) {
                out.push(Diagnostic {
                    file,
                    range: s.name.range,
                    message: format!(
                        "STRUCT `{name}` has the same name as a reserved builtin/tower type — \
                         a `{name}`-typed annotation will always resolve to the builtin, never \
                         to this struct (construction literals, `{name}#{{...}}`, are \
                         unaffected)"
                    ),
                    code: DiagnosticCode::E188,
                });
            }
        }
    }
    out
}

/// Every declared `LIST` name in the project — `List<L>` is nominal per the
/// declaring `LIST` (spec §2/§3), so validating/resolving it needs project-
/// wide knowledge, same as every other cross-file lookup in this crate.
pub(crate) fn declared_list_names(index: &SymbolIndex) -> BTreeSet<String> {
    index
        .symbols
        .values()
        .filter(|s| s.kind == SymbolKind::List)
        .map(|s| s.name.clone())
        .collect()
}

/// Every declared `STRUCT` name in the project (TM-4b, docs/typed-mode-spec.md
/// §6) — mirrors [`declared_list_names`] exactly for the same reason: a
/// struct name is nominal, and joining the annotation grammar needs
/// project-wide knowledge.
pub(crate) fn declared_struct_names(index: &SymbolIndex) -> BTreeSet<String> {
    index
        .symbols
        .values()
        .filter(|s| s.kind == SymbolKind::Struct)
        .map(|s| s.name.clone())
        .collect()
}

/// Semantic diagnostics on annotation content: unknown type names (`E061`).
/// Brink-dialect-only (see module doc). `manifest`: the registered host
/// manifest, if any — T1d-2's `Handle<K>` vocabulary source (`None` degrades
/// to an empty handle-kind set, same posture as every other manifest-driven
/// check).
///
/// Issue #2272: a bare `Named` annotation's struct-name check is
/// **referrer-scoped**, mirroring `resolve::resolve_type_ref`'s own
/// `ImportScope`/`Candidacy` semantics (this file's `check_one`'s own doc
/// has the detail) — `names.structs` itself stays the project-flat set
/// [`TypeNames::new`] always built (unchanged consumer: [`resolve`]'s
/// `Ty::Struct` resolution and `structs::declared_shapes`'s field-type
/// resolution both still want "declared anywhere", not "visible from
/// here" — see [`check_one`]'s doc for the full accounting of which
/// consumer gets which view).
///
/// `scope`: the referrer's own **declared-module** [`ImportScope`], caller-
/// supplied rather than derived here from `hir.module`/`hir.imports` in
/// isolation. An earlier version of this fix built its own per-file scope
/// exactly that way (mirroring `structs::check_assignments`'s own
/// construction) and called it a `brink-analyzer::annotations`-only change
/// — that claim did NOT survive contact with the full gate: `hir.module`
/// only ever carries an explicit `#@module(...)` directive, but a **native**
/// file's real declared module is *path*-derived (`analyze_with_modules`'s
/// own doc: "a native file's `hir.module` carries a deliberately empty
/// `name`... and would otherwise scope the file to the module named `\"\"`"
/// — never `None`, so the naive derivation silently excluded a native
/// file's own siblings, including the mounted stdlib's own internal
/// same-module references, misfiring `E061` in `std/conventions/
/// screenplay.brink` itself). The correct scope is only ever resolvable
/// with the project's real [`ModuleMap`] (or `brink-db`'s
/// `module_map_query`) in hand — see [`per_file_diagnostics`]'s own `scope`
/// doc — so this DOES have a caller-side signature-plumbing footprint after
/// all (`per_file_diagnostics`, `finish_analysis`, `analyze_with_modules`,
/// and `brink-db`'s `per_file_diagnostics_query`), even though it never
/// touches LIR lowering.
///
/// Takes exactly **one** file, not a slice (review finding on this PR): a
/// single `scope` applied across a whole `files: &[(FileId, &HirFile)]`
/// slice would silently apply file A's import scope to file B's
/// annotations the moment a caller ever passed more than one entry — this
/// crate already keys a scope per file correctly elsewhere
/// (`structs::check_assignments`'s `BTreeMap<FileId, ImportScope>`). Every
/// real and test caller here has only ever passed a single-file slice, so
/// this narrows the signature to what's actually true rather than widening
/// it to a map with only ever one entry.
#[must_use]
pub fn check(
    file: FileId,
    hir: &HirFile,
    index: &SymbolIndex,
    manifest: Option<&HostManifest>,
    scope: &ImportScope,
) -> Vec<Diagnostic> {
    let names = TypeNames::new(index, manifest);
    let mut out = Vec::new();
    for v in &hir.variables {
        if let Some(te) = &v.annotation {
            check_one(te, &names, index, scope, file, &mut out);
        }
    }
    for c in &hir.constants {
        if let Some(te) = &c.annotation {
            check_one(te, &names, index, scope, file, &mut out);
        }
    }
    for knot in &hir.knots {
        check_knot(knot, file, &names, index, scope, &mut out);
    }
    out
}

fn check_knot(
    knot: &Knot,
    file: FileId,
    names: &TypeNames,
    index: &SymbolIndex,
    scope: &ImportScope,
    out: &mut Vec<Diagnostic>,
) {
    for p in &knot.params {
        if let Some(te) = &p.annotation {
            check_one(te, names, index, scope, file, out);
        }
    }
    if let Some(rt) = &knot.return_type {
        check_one(rt, names, index, scope, file, out);
    }
    for stitch in &knot.stitches {
        check_stitch(stitch, file, names, index, scope, out);
    }
}

fn check_stitch(
    stitch: &Stitch,
    file: FileId,
    names: &TypeNames,
    index: &SymbolIndex,
    scope: &ImportScope,
    out: &mut Vec<Diagnostic>,
) {
    for p in &stitch.params {
        if let Some(te) = &p.annotation {
            check_one(te, names, index, scope, file, out);
        }
    }
    if let Some(rt) = &stitch.return_type {
        check_one(rt, names, index, scope, file, out);
    }
}

/// Every distinct declared module (issue #2272) carrying a `SymbolKind::
/// Struct` named `name` — consulted only to make an out-of-scope E061 name
/// *which* module a referrer would need to import from, rather than the
/// referrer needing to guess. `None` when no declared `STRUCT` anywhere in
/// the project carries this name at all (the ordinary "unrecognized name"
/// case — [`check_one`] falls back to the generic message then) or when
/// every same-named candidate lives in the undeclared/legacy stem-module
/// (`module: None`) — which `resolve::classify` treats as unconditionally
/// bare-visible, so a scope miss with such a candidate present cannot
/// actually happen; this stays defensive rather than assuming that.
fn declared_struct_modules_hint(index: &SymbolIndex, name: &str) -> Option<String> {
    let ids = index.by_name.get(name)?;
    let modules: BTreeSet<String> = ids
        .iter()
        .filter_map(|id| index.symbols.get(id))
        .filter(|info| info.kind == SymbolKind::Struct)
        .filter_map(|info| info.module.clone())
        .collect();
    if modules.is_empty() {
        None
    } else {
        Some(modules.into_iter().collect::<Vec<_>>().join(", "))
    }
}

/// Check one type expression (and recursively, its generic args / fn
/// params+return) for unknown names / reserved fn-types.
///
/// The `Named` arm's struct-name check is referrer-scoped (issue #2272):
/// `index`/`scope` route through [`crate::resolve::lookup_by_name`] — the
/// exact `ImportScope`/`Candidacy` machinery `resolve::resolve_type_ref`
/// already applies to the underlying `RefKind::Type` resolution — rather
/// than consulting `names.structs` (project-flat, no referrer-scoping or
/// std-exclusion) the way this arm used to. Before this fix, an unimported
/// std-only struct name (e.g. `~ temp c: Cue` with `Cue` unimported) read
/// as "recognized" here even though `resolve_type_ref` silently missed it
/// by design — raising no diagnostic anywhere (issue #2272's own
/// "compounding gap", left open by PR #2271). `names.lists`/`names.handles`
/// (the `List<L>`/`Handle<K>` arms below) are deliberately **not** touched
/// by this fix — `resolve_type_ref` only ever resolves `SymbolKind::Struct`
/// candidates, so there is no referrer-scoping precedent for those two
/// vocabularies to mirror; scoping them is out of this issue's fence.
fn check_one(
    te: &brink_ir::TypeExpr,
    names: &TypeNames,
    index: &SymbolIndex,
    scope: &ImportScope,
    file: FileId,
    out: &mut Vec<Diagnostic>,
) {
    match te {
        brink_ir::TypeExpr::Named { name, range } => {
            // TM-4b (docs/typed-mode-spec.md §6): "declared struct names
            // join the TM-2 annotation type grammar... E061 no longer fires
            // for a declared name" — narrowed by #2272: "declared" now
            // means declared AND reachable from this referrer, not merely
            // declared somewhere in the project.
            if !is_known_leaf(name)
                && crate::resolve::lookup_by_name(index, scope, name, &[SymbolKind::Struct])
                    .is_none()
            {
                let message = match declared_struct_modules_hint(index, name) {
                    // Dialect-blind wording (review finding — mirrors
                    // `modules::check`'s own E025 precedent, see that call
                    // site's comment): this arm runs identically for both
                    // `.ink` (`STRUCT`) and native `.brink` (`struct`)
                    // source, so the message must not spell out either
                    // keyword's casing. Nor does it say "import it" —
                    // `lookup_by_name`'s `!multiple` fast path (see its own
                    // doc) returns any sole ordinary candidate regardless of
                    // scope, so the only candidates that can ever reach this
                    // arm with a non-empty module hint are reserved-root
                    // (std) ones — and a real `use std::…` import does not
                    // exist yet (`std/conventions/screenplay.brink`'s own
                    // header; `resolve::lookup_by_name_direct`'s std gate):
                    // it needs #1582's `pub` marker and #2167's
                    // closure-scoped confinement, neither built. Say what is
                    // actually true today instead of advice no author can
                    // follow.
                    Some(modules) => format!(
                        "`{name}` names a declared struct in `{modules}`, but it isn't \
                         reachable from this file yet (see #1582, #2167) — check the spelling, \
                         or declare/use it from a module this file can see"
                    ),
                    None => format!(
                        "`{name}` is not a recognized type — expected int, float, bool, \
                         string, content, divert, void, a tower kind \
                         (vec2/vec3/vec4/quat/mat2/mat3/mat4), List<L>, Array<T>, Map<K, V>, \
                         Option<T>, Weighted<T>, Handle<K>, or a declared STRUCT name"
                    ),
                };
                out.push(Diagnostic {
                    file,
                    range: *range,
                    message,
                    code: DiagnosticCode::E061,
                });
            }
        }
        brink_ir::TypeExpr::Generic { name, args, range } => match name.as_str() {
            "List" => {
                let bad = match args.as_slice() {
                    [brink_ir::TypeExpr::Named { name: l, .. }] => !names.lists.contains(l),
                    _ => true,
                };
                if bad {
                    out.push(Diagnostic {
                        file,
                        range: *range,
                        message: format!(
                            "`List<{}>` doesn't name a declared LIST",
                            args.first().map_or(String::new(), display_short)
                        ),
                        code: DiagnosticCode::E061,
                    });
                }
            }
            // T1d-2 (docs/t1d-spec.md §3): `Handle<K>` is a legal type form
            // whose kind vocabulary lives in the registered host manifest,
            // not ink source — the `List<L>` pattern above, mirrored against
            // `names.handles` instead of `names.lists`.
            "Handle" => {
                let bad = match args.as_slice() {
                    [brink_ir::TypeExpr::Named { name: k, .. }] => !names.handles.contains(k),
                    _ => true,
                };
                if bad {
                    out.push(Diagnostic {
                        file,
                        range: *range,
                        message: format!(
                            "`Handle<{}>` doesn't name a declared handle kind in the host \
                             manifest",
                            args.first().map_or(String::new(), display_short)
                        ),
                        code: DiagnosticCode::E061,
                    });
                }
            }
            // Option<T>/Weighted<T> (issue #1552): newly annotatable,
            // content-checked recursively exactly like Array<T>/Map<K, V> —
            // there's no separate declared vocabulary to validate the
            // element against, only its own well-formedness.
            "Array" | "Map" | "Option" | "Weighted" => {
                for a in args {
                    check_one(a, names, index, scope, file, out);
                }
            }
            _ => {
                out.push(Diagnostic {
                    file,
                    range: *range,
                    message: format!("`{name}<...>` is not a recognized generic type"),
                    code: DiagnosticCode::E061,
                });
            }
        },
        // T1c: `fn(T…): R` is a legal type form (docs/t1c-spec.md §4 —
        // "boundary annotations gain the fn(T…): R form"); E062 is retired.
        // Component names are still content-checked recursively (E061).
        brink_ir::TypeExpr::Fn { params, ret, .. } => {
            for p in params {
                check_one(p, names, index, scope, file, out);
            }
            check_one(ret, names, index, scope, file, out);
        }
    }
}

fn display_short(te: &brink_ir::TypeExpr) -> String {
    match te {
        brink_ir::TypeExpr::Named { name, .. } => name.clone(),
        brink_ir::TypeExpr::Generic { name, .. } => format!("{name}<...>"),
        brink_ir::TypeExpr::Fn { .. } => "fn(...)".to_owned(),
    }
}

// ─── Signature-firewall mismatch (E063) ──────────────────────────────

/// Compare each def's annotated param/return types (`Sig`, declaration-only)
/// against the same def's body-inferred types (`InferenceResult`, from
/// `infer_project`/the composed `call_edges`→`solve_scc` path) and report a
/// disagreement. Advisory-only (`E063` is a warning) — severity policy for
/// strict mode is TM-3's call, not this one's.
///
/// A pure consumer of two already-public seams: never touches
/// `infer::body`'s internals, never re-solves anything.
/// `manifest`: the registered host manifest (T1d-2), so an annotated
/// `Handle<K>` param/return can resolve against its declared handle kinds
/// instead of always reading as an unresolved annotation — `None` degrades
/// to an empty handle-kind set (gradual/advisory: an unresolved `Handle<K>`
/// merely opts the slot out of `E063`, never a hard failure).
#[must_use]
pub fn mismatches(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    inference: &InferenceResult,
    manifest: Option<&HostManifest>,
) -> Vec<Diagnostic> {
    let names = TypeNames::new(index, manifest);
    let mut out = Vec::new();
    for &(file, hir) in files {
        for knot in &hir.knots {
            check_def_mismatch(knot, file, index, &names, inference, &mut out);
            for stitch in &knot.stitches {
                check_stitch_mismatch(
                    stitch,
                    &knot.name.text,
                    file,
                    index,
                    &names,
                    inference,
                    &mut out,
                );
            }
        }
    }
    out
}

pub(crate) fn def_id_for(
    index: &SymbolIndex,
    file: FileId,
    kind: SymbolKind,
    name: &str,
) -> Option<DefinitionId> {
    index
        .by_name
        .get(name)?
        .iter()
        .find(|id| {
            index
                .symbols
                .get(id)
                .is_some_and(|info| info.file == file && info.kind == kind)
        })
        .copied()
}

fn check_def_mismatch(
    knot: &Knot,
    file: FileId,
    index: &SymbolIndex,
    names: &TypeNames,
    inference: &InferenceResult,
    out: &mut Vec<Diagnostic>,
) {
    let Some(id) = def_id_for(index, file, knot.symbol_kind(), &knot.name.text) else {
        return;
    };
    let Some(inferred) = inference.signatures.get(&id) else {
        return;
    };
    for (i, p) in knot.params.iter().enumerate() {
        let Some(ann) = &p.annotation else { continue };
        let Some(ann_ty) = resolve(ann, names) else {
            continue;
        };
        let Some(body_ty) = inferred.params.get(i) else {
            continue;
        };
        report_if_mismatched(ann, &ann_ty, body_ty, file, out);
    }
    if let Some(rt) = &knot.return_type
        && let Some(ann_ty) = resolve(rt, names)
    {
        report_if_mismatched(rt, &ann_ty, &inferred.return_ty, file, out);
    }
}

fn check_stitch_mismatch(
    stitch: &Stitch,
    knot_name: &str,
    file: FileId,
    index: &SymbolIndex,
    names: &TypeNames,
    inference: &InferenceResult,
    out: &mut Vec<Diagnostic>,
) {
    let qualified = format!("{knot_name}.{}", stitch.name.text);
    let Some(id) = def_id_for(index, file, SymbolKind::Stitch, &qualified) else {
        return;
    };
    let Some(inferred) = inference.signatures.get(&id) else {
        return;
    };
    for (i, p) in stitch.params.iter().enumerate() {
        let Some(ann) = &p.annotation else { continue };
        let Some(ann_ty) = resolve(ann, names) else {
            continue;
        };
        let Some(body_ty) = inferred.params.get(i) else {
            continue;
        };
        report_if_mismatched(ann, &ann_ty, body_ty, file, out);
    }
    if let Some(rt) = &stitch.return_type
        && let Some(ann_ty) = resolve(rt, names)
    {
        report_if_mismatched(rt, &ann_ty, &inferred.return_ty, file, out);
    }
}

/// `body_ty` disagrees with `ann_ty` when the body implies something
/// concrete that isn't the annotation itself and isn't absorbed by it
/// (`Unknown` never disagrees — an unused/unconstrained slot is silent, not
/// a mismatch; `unify(ann, body) == ann` covers the one legal directional
/// coercion, `int` annotated but body only ever compares against `int`
/// literals promoted to `float` nowhere, etc.). `Conflicted` (#627) reads
/// the same as `Unknown` here too: E063 is gradual/advisory (never wired
/// into `finish_analysis`), and reporting a *conflicted* slot specifically
/// is strict mode's TM-3 (#619) job, not this diagnostic's — see
/// [`Ty::is_unresolved`].
fn report_if_mismatched(
    te: &brink_ir::TypeExpr,
    ann_ty: &Ty,
    body_ty: &Ty,
    file: FileId,
    out: &mut Vec<Diagnostic>,
) {
    if body_ty.is_unresolved() {
        return;
    }
    // Row-insensitive (issue #1680): an annotation's `Ty::Fn` carries the
    // top effect row while a body-derived one carries its real creation
    // targets, so a structural `unify(ann, body) == ann` would call every
    // correctly-annotated fn-typed slot a mismatch. `assignable` erases
    // rows on both sides — see `infer::assignable`.
    if crate::infer::assignable(ann_ty, body_ty) {
        return;
    }
    out.push(Diagnostic {
        file,
        range: te.range(),
        message: format!(
            "annotated type `{}` disagrees with the type inferred from usage (`{}`)",
            ann_ty.display(),
            body_ty.display()
        ),
        code: DiagnosticCode::E063,
    });
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use super::*;
    use brink_ir::ResolutionMap;
    use brink_ir::hir::lower;

    fn build(src: &str) -> (HirFile, SymbolIndex) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        (hir, (*index).clone())
    }

    /// Build a [`TypeNames`] bundle for a `resolve()` test call — pre-T1d-2
    /// tests only ever needed `lists`/`structs`; `handles` stays empty
    /// (no manifest in these fixtures).
    fn tn(lists: &BTreeSet<String>, structs: &BTreeSet<String>) -> TypeNames {
        TypeNames {
            lists: lists.clone(),
            structs: structs.clone(),
            handles: BTreeSet::new(),
        }
    }

    /// Like [`build`], but also computes real resolutions — needed by the
    /// `mismatches()` tests: `infer_project` resolves body references (e.g.
    /// `hp` inside a knot body back to its own param) via the resolution
    /// map, same as `infer::tests::build`'s helper does.
    fn build_with_resolutions(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        (hir, (*index).clone(), (*resolutions).clone())
    }

    // ── resolve() ───────────────────────────────────────────────────

    #[test]
    fn resolve_recognizes_scalar_leaves() {
        let (hir, _index) = build("VAR a: int = 1\nVAR b: float = 1.0\nVAR c: bool = true\n");
        let a = hir.variables[0].annotation.as_ref().expect("a annotation");
        let b = hir.variables[1].annotation.as_ref().expect("b annotation");
        let c = hir.variables[2].annotation.as_ref().expect("c annotation");
        let empty = BTreeSet::new();
        assert_eq!(resolve(a, &tn(&empty, &empty)), Some(Ty::Int));
        assert_eq!(resolve(b, &tn(&empty, &empty)), Some(Ty::Float));
        assert_eq!(resolve(c, &tn(&empty, &empty)), Some(Ty::Bool));
    }

    /// Issue #1846, `docs/prose-dialect-spec.md` §3.5b's capture contract:
    /// `content` resolves to `Ty::Content`, a fixed global leaf like the
    /// scalars above — it needs no declared vocabulary the way `List<L>`/
    /// `Handle<K>` do.
    #[test]
    fn resolve_recognizes_content_leaf() {
        let (hir, _index) = build("VAR v: content = 0\n");
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        let empty = BTreeSet::new();
        assert_eq!(resolve(te, &tn(&empty, &empty)), Some(Ty::Content));
    }

    #[test]
    fn resolve_array_and_map_generics() {
        let (hir, _index) = build("VAR a: Array<int> = 0\nVAR m: Map<string, int> = 0\n");
        let a = hir.variables[0].annotation.as_ref().expect("a");
        let m = hir.variables[1].annotation.as_ref().expect("m");
        let empty = BTreeSet::new();
        assert_eq!(
            resolve(a, &tn(&empty, &empty)),
            Some(Ty::Array(Box::new(Ty::Int)))
        );
        assert_eq!(
            resolve(m, &tn(&empty, &empty)),
            Some(Ty::Map(Box::new(Ty::String), Box::new(Ty::Int)))
        );
    }

    #[test]
    fn resolve_list_generic_needs_declared_list_name() {
        let (hir, _index) = build("VAR w: List<Weathers> = 0\n");
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        let empty = BTreeSet::new();
        assert_eq!(
            resolve(te, &tn(&empty, &empty)),
            None,
            "Weathers isn't declared here"
        );
        let declared: BTreeSet<String> = ["Weathers".to_string()].into_iter().collect();
        assert_eq!(
            resolve(te, &tn(&declared, &empty)),
            Some(Ty::List("Weathers".to_string()))
        );
    }

    #[test]
    fn resolve_void_and_unknown_are_none() {
        let (hir, _index) = build("VAR v: void = 0\nVAR u: Frobnicator = 0\n");
        let empty = BTreeSet::new();
        for v in &hir.variables {
            let te = v.annotation.as_ref().expect("annotation");
            assert_eq!(resolve(te, &tn(&empty, &empty)), None, "{v:?}");
        }
    }

    // ── T1c fn(T…): R (docs/t1c-spec.md §4) ─────────────────────────

    #[test]
    fn resolve_fn_type_form() {
        let (hir, _index) = build("VAR cb: fn(int, string): bool = 0\nVAR z: fn(): int = 0\n");
        let empty = BTreeSet::new();
        let cb = hir.variables[0].annotation.as_ref().expect("cb");
        let z = hir.variables[1].annotation.as_ref().expect("z");
        assert_eq!(
            resolve(cb, &tn(&empty, &empty)),
            Some(Ty::Fn(
                vec![Ty::Int, Ty::String],
                Box::new(Ty::Bool),
                crate::infer::FnRow::unknown()
            ))
        );
        assert_eq!(
            resolve(z, &tn(&empty, &empty)),
            Some(Ty::Fn(
                Vec::new(),
                Box::new(Ty::Int),
                crate::infer::FnRow::unknown()
            ))
        );
    }

    #[test]
    fn resolve_nested_fn_type_forms() {
        // fn types compose with the generic heads in both directions.
        let (hir, _index) =
            build("VAR a: Array<fn(int): int> = 0\nVAR b: fn(Array<int>): fn(int): bool = 0\n");
        let empty = BTreeSet::new();
        let a = hir.variables[0].annotation.as_ref().expect("a");
        let b = hir.variables[1].annotation.as_ref().expect("b");
        assert_eq!(
            resolve(a, &tn(&empty, &empty)),
            Some(Ty::Array(Box::new(Ty::Fn(
                vec![Ty::Int],
                Box::new(Ty::Int),
                crate::infer::FnRow::unknown()
            ))))
        );
        assert_eq!(
            resolve(b, &tn(&empty, &empty)),
            Some(Ty::Fn(
                vec![Ty::Array(Box::new(Ty::Int))],
                Box::new(Ty::Fn(
                    vec![Ty::Int],
                    Box::new(Ty::Bool),
                    crate::infer::FnRow::unknown()
                )),
                crate::infer::FnRow::unknown()
            ))
        );
    }

    #[test]
    fn resolve_fn_type_with_void_return_is_none_in_this_slice() {
        // The checker's type universe has no void — a fn type whose return
        // is void resolves as absent (documented T1c-1 limitation).
        let (hir, _index) = build("VAR cb: fn(int): void = 0\n");
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        let empty = BTreeSet::new();
        assert_eq!(resolve(te, &tn(&empty, &empty)), None);
    }

    #[test]
    fn resolve_recognizes_declared_struct_name() {
        // TM-4b: "declared struct names join the TM-2 annotation type
        // grammar" — a bare `Named` type whose name is a declared `STRUCT`
        // resolves to `Ty::Struct`, same join `list_names` gives `List<L>`.
        let (hir, _index) = build("STRUCT Point = #{x: float}\nVAR p: Point = 0\n");
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        let empty = BTreeSet::new();
        assert_eq!(
            resolve(te, &tn(&empty, &empty)),
            None,
            "Point isn't in struct_names here"
        );
        let declared: BTreeSet<String> = ["Point".to_string()].into_iter().collect();
        assert_eq!(
            resolve(te, &tn(&empty, &declared)),
            Some(Ty::Struct("Point".to_string()))
        );
    }

    // ── #1552 Option<T>/Weighted<T> annotatable ──────────────────────

    #[test]
    fn resolve_option_and_weighted_generics() {
        let (hir, _index) = build("VAR o: Option<int> = 0\nVAR w: Weighted<string> = 0\n");
        let o = hir.variables[0].annotation.as_ref().expect("o");
        let w = hir.variables[1].annotation.as_ref().expect("w");
        let empty = BTreeSet::new();
        assert_eq!(
            resolve(o, &tn(&empty, &empty)),
            Some(Ty::Option(Box::new(Ty::Int)))
        );
        assert_eq!(
            resolve(w, &tn(&empty, &empty)),
            Some(Ty::Weighted(Box::new(Ty::String)))
        );
    }

    #[test]
    fn check_accepts_option_and_weighted_annotations() {
        let (hir, index) = build("VAR o: Option<int> = 0\nVAR w: Weighted<float> = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_flags_unknown_name_inside_option_element() {
        // Option<T>/Weighted<T> content-check recursively, exactly like
        // Array<T>/Map<K, V> — an unrecognized element name still flags.
        let (hir, index) = build("VAR o: Option<Bogus> = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    // ── #1552 old lowercase spellings are a breaking rename ──────────

    #[test]
    fn old_lowercase_generic_heads_no_longer_resolve() {
        // The pre-#1552 spelling was `array<T>`/`map<K, V>`/`list<L>` —
        // lowercase heads. The rename to `Array<T>`/`Map<K, V>`/`List<L>` is
        // breaking by design (docs/decision-log.md 2026-07-27 "Type-name
        // surface ruled"): the old lowercase head is now just an
        // unrecognized generic name, exactly like any typo. Built via
        // `format!` rather than a literal so a future casing sweep over this
        // file's own source text can't accidentally launder the fixture.
        let lower = |s: &str| s.to_lowercase();
        let source = format!(
            "LIST Weathers = sunny, rainy\n\
             VAR a: {}<int> = 0\n\
             VAR m: {}<string, int> = 0\n\
             VAR w: {}<Weathers> = 0\n",
            lower("Array"),
            lower("Map"),
            lower("List"),
        );
        let (hir, index) = build(&source);
        let empty = BTreeSet::new();
        let declared: BTreeSet<String> = ["Weathers".to_string()].into_iter().collect();
        for v in &hir.variables {
            let te = v.annotation.as_ref().expect("annotation");
            assert_eq!(
                resolve(te, &tn(&declared, &empty)),
                None,
                "{v:?} should no longer resolve under the old lowercase spelling"
            );
        }
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(diags.len(), 3, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E061));
    }

    // ── T1d-2 Handle<K> (docs/t1d-spec.md §3) ────────────────────────

    /// A `HostManifest` declaring one handle kind, `AudioInstance`.
    fn audio_instance_manifest() -> HostManifest {
        HostManifest {
            markup: Vec::new(),
            types: vec![brink_ir::SemanticTypeDef {
                name: "AudioInstance".to_string(),
                base: BaseType::Handle,
                constraint: None,
                values: None,
                widget: None,
            }],
            ..Default::default()
        }
    }

    #[test]
    fn declared_handle_kinds_reads_only_handle_based_semantic_types() {
        let manifest = HostManifest {
            markup: Vec::new(),
            types: vec![
                brink_ir::SemanticTypeDef {
                    name: "AudioInstance".to_string(),
                    base: BaseType::Handle,
                    constraint: None,
                    values: None,
                    widget: None,
                },
                brink_ir::SemanticTypeDef {
                    name: "switch_id".to_string(),
                    base: BaseType::Int,
                    constraint: None,
                    values: None,
                    widget: None,
                },
            ],
            ..Default::default()
        };
        let kinds = declared_handle_kinds(Some(&manifest));
        assert_eq!(kinds, ["AudioInstance".to_string()].into_iter().collect());
        assert!(declared_handle_kinds(None).is_empty());
    }

    #[test]
    fn resolve_handle_generic_needs_declared_manifest_kind() {
        let (hir, index) = build("VAR h: Handle<AudioInstance> = 0\n");
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        assert_eq!(
            resolve(te, &TypeNames::new(&index, None)),
            None,
            "AudioInstance isn't declared without a manifest"
        );
        let manifest = audio_instance_manifest();
        assert_eq!(
            resolve(te, &TypeNames::new(&index, Some(&manifest))),
            Some(Ty::Handle("AudioInstance".to_string()))
        );
    }

    #[test]
    fn check_flags_undeclared_handle_kind() {
        let (hir, index) = build("VAR h: Handle<Nope> = 0\n");
        let diags = check(
            FileId(0),
            &hir,
            &index,
            Some(&audio_instance_manifest()),
            &ImportScope::default(),
        );
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    #[test]
    fn check_accepts_declared_handle_kind() {
        let (hir, index) = build("VAR h: Handle<AudioInstance> = 0\n");
        let diags = check(
            FileId(0),
            &hir,
            &index,
            Some(&audio_instance_manifest()),
            &ImportScope::default(),
        );
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_flags_handle_kind_with_no_manifest_registered() {
        // Mirrors `check_flags_undeclared_list_name`: a `Handle<K>` with no
        // manifest registered at all has no vocabulary to resolve against —
        // an empty handle-kind set, same degrade-gracefully posture as
        // every other manifest-driven check.
        let (hir, index) = build("VAR h: Handle<AudioInstance> = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    // ── check() ─────────────────────────────────────────────────────

    #[test]
    fn check_flags_unknown_type_name() {
        let (hir, index) = build("VAR p: Frobnicator = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    #[test]
    fn check_accepts_fn_type_since_t1c() {
        // T1c-1 (#699): E062 retired — `fn(T…): R` is a legal type form.
        let (hir, index) = build("VAR cb: fn(int, int): bool = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_still_flags_unknown_names_inside_a_fn_type() {
        let (hir, index) = build("VAR cb: fn(Bogus): bool = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    #[test]
    fn check_accepts_known_scalar_and_generic_types() {
        let (hir, index) =
            build("VAR a: int = 1\nVAR b: Array<float> = 0\nVAR c: Map<string, bool> = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_accepts_void_return_type() {
        let (hir, index) = build("=== function noop(): void ===\n~ return\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Issue #1846: before this landed, the ruled `fn radio(chan: string,
    /// text: content)` example (`docs/prose-dialect-spec.md` §3.5b) could
    /// not compile — `content` tripped `E061` like any other unrecognized
    /// name. Same signature, exercised through the shared ink-grammar
    /// `function` form this module's tests already use.
    #[test]
    fn check_accepts_content_param_annotation() {
        let (hir, index) =
            build("=== function radio(chan: string, text: content) ===\n~ return text\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_accepts_declared_list_name() {
        let (hir, index) = build("LIST Weathers = sunny, rainy\nVAR w: List<Weathers> = sunny\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// TM-4b: "E061 no longer fires for a declared [struct] name".
    #[test]
    fn check_accepts_declared_struct_name() {
        let (hir, index) = build("STRUCT Point = #{x: float}\nVAR p: Point = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── #2272: E061 referrer-scoping (split out of #2249/PR #2271) ───

    /// Inject a `SymbolKind::Struct` named `name`, declared under `module`,
    /// into `index` — mirrors `resolve.rs`'s own hand-built std-mount
    /// fixtures (e.g. `resolve_type_ref_excludes_a_std_only_struct_with_no_
    /// project_homonym_or_import`), reused here so `annotations::check`'s
    /// own referrer-scoping gets the exact same std-mount shape proven at
    /// the `resolve_type_ref` layer, rather than a second, possibly
    /// diverging fixture shape.
    fn inject_struct(index: &mut SymbolIndex, name: &str, module: &str, tag_seed: u64) {
        let id = DefinitionId::new(brink_format::DefinitionTag::StructDef, tag_seed);
        index.symbols.insert(
            id,
            brink_ir::SymbolInfo {
                kind: SymbolKind::Struct,
                file: FileId(9),
                range: rowan::TextRange::default(),
                id,
                name: name.to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some(module.to_string()),
                visibility: brink_ir::Visibility::Public,
            },
        );
        index.by_name.entry(name.to_string()).or_default().push(id);
    }

    /// RED characterization (issue #2272's own "compounding gap"): before
    /// this fix, a std-only struct name unreachable through this file's
    /// `ImportScope` still read as "recognized" by E061's project-flat
    /// `names.structs` — even though `resolve::resolve_type_ref` (PR #2271)
    /// already silently excludes exactly this candidate from the
    /// `RefKind::Type` resolution feeding lowering. Net effect before this
    /// fix: `~ temp`/`VAR`-shaped reference to an unimported std-only
    /// struct name raised NO diagnostic anywhere. This test pins the GREEN
    /// side — E061 now fires, naming the module to import from.
    #[test]
    fn check_flags_unimported_std_only_struct_name_referrer_scoped() {
        let (hir, mut index) = build("VAR c: Cue = 0\n");
        inject_struct(&mut index, "Cue", "std::conventions::screenplay", 0xC0F);

        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(
            diags.len(),
            1,
            "an unimported std-only struct name must now raise E061 — before this fix it \
             raised nothing anywhere: {diags:?}"
        );
        assert_eq!(diags[0].code, DiagnosticCode::E061);
        assert!(
            diags[0].message.contains("std::conventions::screenplay"),
            "the message should hint the module a referrer would import from: {:?}",
            diags[0].message
        );
    }

    /// Forward-looking guard, NOT evidence the escape hatch works today
    /// (review finding): this test synthesizes TWO states the real
    /// compilation pipeline cannot produce yet, both required for `check`
    /// to stay clean here. (1) `inject_struct` marks the std struct
    /// `Visibility::Public` directly in a hand-built `SymbolIndex` —
    /// `lookup_by_name_direct`'s own doc (`resolve.rs`, issue #2197) states
    /// plainly that "nothing under `std` can be marked public yet" in a
    /// real mount. (2) the hand-built `ImportScope` below claims a
    /// qualified import of `std::conventions::screenplay` — no real `use
    /// std::…`/`IMPORT` syntax can populate `qualified_modules`/
    /// `bare_imports` with a std module today (needs #1582's `pub` marker
    /// and #2167's closure-scoped confinement, neither built — see the
    /// E061 message's own comment on this). Given both synthetic
    /// preconditions, `classify` legitimately answers `Imported` and
    /// `check` legitimately stays clean — so the test **does** exercise
    /// real `classify`/`lookup_by_name` machinery correctly, but only
    /// because it starts from a state no author can reach today. Read it as
    /// a guard for the day #1582/#2167 land, not as proof the escape hatch
    /// is usable now.
    #[test]
    fn check_accepts_std_only_struct_name_once_imported() {
        let (hir, mut index) = build("VAR c: Cue = 0\n");
        inject_struct(&mut index, "Cue", "std::conventions::screenplay", 0xC10);

        // `check()` now takes the referrer's `ImportScope` as a caller-
        // supplied parameter (issue #2272's own gate finding — see
        // `check`'s doc for why deriving it locally from `hir.module` was
        // wrong) rather than deriving one internally, so a real "imported"
        // scope is exercised end-to-end here: hand-construct the
        // qualified-import scope shape (mirroring `resolve.rs`'s own
        // fixtures) and confirm `check` itself, not just the underlying
        // `lookup_by_name` primitive, stays clean.
        let scope = ImportScope {
            file_module: None,
            qualified_modules: ["std::conventions::screenplay".to_string()]
                .into_iter()
                .collect(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        let diags = check(FileId(0), &hir, &index, None, &scope);
        assert!(
            diags.is_empty(),
            "an imported std struct must stay clean through the real check() call: {diags:?}"
        );
    }

    /// Should-NOT-fire: a locally-declared struct in the *same* (undeclared/
    /// legacy) stem-module — `module: None`, unconditionally bare-visible
    /// per `resolve::classify` — still resolves clean. Regression guard:
    /// referrer-scoping must not narrow the single-file/no-modules world
    /// this crate's entire pre-#2249 test suite lives in (see
    /// `check_accepts_declared_struct_name` just above, unmoved).
    #[test]
    fn check_still_accepts_locally_declared_struct_with_no_modules_in_play() {
        let (hir, index) = build("STRUCT Cue = #{x: int}\nVAR c: Cue = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(
            diags.is_empty(),
            "an ordinary, single-file, no-`#@module` struct declaration must stay clean: \
             {diags:?}"
        );
    }

    /// Should-NOT-fire: every builtin/tower leaf name from
    /// [`is_known_leaf`] never routes through the referrer-scoped lookup at
    /// all — `check_one`'s `is_known_leaf(name)` short-circuit runs first,
    /// unchanged by this fix. Regression guard for the "leaves are checked
    /// before the struct lookup" ordering this fix must not disturb.
    #[test]
    fn check_still_accepts_every_builtin_leaf_with_a_std_mount_present() {
        let (hir, mut index) = build(
            "VAR a: int = 0\nVAR b: float = 0\nVAR c: bool = true\nVAR d: string = \"x\"\n\
             VAR e: content = 0\nVAR f: divert = 0\nVAR g: vec3 = 0\n",
        );
        // A std mount is present in the index but declares nothing named
        // like any of these leaves — proves the leaf short-circuit, not an
        // absence of candidates, is what keeps them clean.
        inject_struct(
            &mut index,
            "Unrelated",
            "std::conventions::screenplay",
            0xC11,
        );
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn check_flags_undeclared_struct_name_still() {
        // A name that isn't a known scalar, generic head, or declared
        // struct still flags E061 — TM-4b only widens the accepted set.
        let (hir, index) = build("VAR w: NotAStruct = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    #[test]
    fn check_flags_undeclared_list_name() {
        let (hir, index) = build("VAR w: List<Nope> = 0\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E061);
    }

    #[test]
    fn check_flags_param_and_return_type_annotations() {
        let (hir, index) = build("=== function heal(hp: Bogus): AlsoBogus ===\n~ return hp\n");
        let diags = check(FileId(0), &hir, &index, None, &ImportScope::default());
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E061));
    }

    // ── mismatches() ────────────────────────────────────────────────

    /// Issue #1680: an annotated `fn(T…): R` return type carries the
    /// *unknown* effect row while the body's `#fn(target)` return carries a
    /// concrete one. `report_if_mismatched` compares them through
    /// `infer::assignable`, which erases rows on both sides — the two are
    /// the same type and must not be reported.
    ///
    /// The unknown row is the join's top element and therefore absorbing,
    /// so this direction was already safe under the old structural test;
    /// the assertion pins it against a future change to where an annotation
    /// gets its row from, and keeps all four assignability sites on one
    /// predicate.
    #[test]
    fn a_fn_typed_annotation_does_not_disagree_with_its_body_derived_row() {
        let (hir, index, res) = build_with_resolutions(
            "=== function bump(n: int): int ===\n~ return n + 1\n\
             === function pick(): fn(int): int ===\n~ return #fn(bump)\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference, None);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E063),
            "{diags:?}"
        );
    }

    #[test]
    fn mismatches_flags_annotation_disagreeing_with_body_inference() {
        // `hp` is annotated `string` but the body only ever compares it
        // against an int literal — body inference derives `int`.
        let (hir, index, res) =
            build_with_resolutions("=== heal(hp: string) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E063);
    }

    #[test]
    fn mismatches_is_silent_when_annotation_and_inference_agree() {
        let (hir, index, res) =
            build_with_resolutions("=== heal(hp: int) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mismatches_is_silent_when_body_never_constrains_the_param() {
        // Annotated `int`, body never uses `hp` at all — body infers
        // `Unknown`, which never disagrees (spec: "unresolved -> Unknown,
        // which is LEGAL").
        let (hir, index, res) = build_with_resolutions("=== heal(hp: int) ===\nHello.\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mismatches_is_silent_for_the_legal_int_to_float_coercion() {
        // Annotated `float`, body only ever compares against an int literal
        // — `unify(Float, Int) == Float`, the one legal directional
        // coercion (spec §4) — not a disagreement.
        let (hir, index, res) =
            build_with_resolutions("=== heal(hp: float) ===\n{hp > 1:\n  ok\n}\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mismatches_is_silent_when_body_is_conflicted() {
        // #627 ruling: `Conflicted` reads exactly like `Unknown` to this
        // gradual/advisory consumer — reporting a *conflicted* slot
        // specifically is strict mode's TM-3 (#619) job, not E063's. `hp`
        // is compared against both an int and a string literal (a genuine
        // conflict), annotated `int`; this must stay silent, unchanged from
        // the pre-#627 behavior where the same body inferred `Unknown`.
        let (hir, index, res) = build_with_resolutions(
            "=== heal(hp: int) ===\n{hp > 1:\n  ok\n}\n{hp == \"x\":\n  no\n}\n-> DONE\n",
        );
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        // Confirm the fixture actually exercises `Conflicted`, not some
        // other path, before asserting on `mismatches`' silence.
        let heal_id = index
            .by_name
            .get("heal")
            .and_then(|ids| ids.first())
            .copied()
            .expect("heal");
        let sig = inference
            .signatures
            .get(&heal_id)
            .expect("inferred signature for heal");
        assert_eq!(sig.params, vec![Ty::Conflicted], "fixture sanity check");

        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference, None);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn mismatches_flags_nested_stitch_return_type_disagreeing_with_body_inference() {
        // Regression for the #1509 review finding: `check_stitch_mismatch`
        // looked the stitch up under its bare name, but a nested stitch is
        // indexed under `"{knot}.{stitch}"` (brink-ir/src/symbols/project.rs)
        // — so `def_id_for` always missed and this check was dead code.
        // `fire` is annotated `: string` but its body only ever returns an
        // int literal.
        let (hir, index, res) =
            build_with_resolutions("=== camp ===\n= fire(): string\n~ return 1\n-> DONE\n");
        let inference =
            crate::infer_project(&[(FileId(0), &hir)], &index, &res, None, &BTreeMap::new());
        let diags = mismatches(&[(FileId(0), &hir)], &index, &inference, None);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E063);
    }

    // ── check_reserved_type_names() / E188 (issue #1865) ────────────────

    /// RED characterization: a `STRUCT` declared `content` compiles clean
    /// today (before this issue's fix, and unchanged after it — the fix
    /// diagnoses this, it does not re-order resolution) with every
    /// `content`-typed annotation silently meaning the builtin
    /// `Ty::Content`, never the struct. This is the observable wrong
    /// resolution issue #1865 reports.
    #[test]
    fn red_annotation_named_content_resolves_to_builtin_not_the_colliding_struct() {
        let (hir, index) = build("STRUCT content = #{x: int}\nVAR v: content = 0\n");
        let names = TypeNames::new(&index, None);
        assert!(
            names.structs.contains("content"),
            "fixture sanity check: the struct must actually be declared"
        );
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        assert_eq!(
            resolve(te, &names),
            Some(Ty::Content),
            "the builtin leaf wins — the struct is unreachable through this annotation"
        );
    }

    /// GREEN: the new declaration-site diagnostic fires for the fixture
    /// above, naming both the struct and the reserved name it collides
    /// with.
    #[test]
    fn struct_named_content_collides_with_builtin_leaf_is_e188() {
        let (hir, _index) = build("STRUCT content = #{x: int}\n");
        let diags = check_reserved_type_names(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E188);
        assert!(
            diags[0].message.contains("content"),
            "{:?}",
            diags[0].message
        );
    }

    /// The NS-A8 tower-kind sibling: `resolve`'s tower-kind arm runs before
    /// the struct lookup too (this file's own doc on `resolve`), so a
    /// `STRUCT vec3` is shadowed exactly like `content` is — same
    /// RED/GREEN pair, one test each.
    #[test]
    fn red_annotation_named_vec3_resolves_to_tower_kind_not_the_colliding_struct() {
        let (hir, index) =
            build("STRUCT vec3 = #{x: float, y: float, z: float}\nVAR v: vec3 = 0\n");
        let names = TypeNames::new(&index, None);
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        assert!(
            !matches!(resolve(te, &names), Some(Ty::Struct(_))),
            "the tower kind must win over the colliding struct, got {:?}",
            resolve(te, &names)
        );
    }

    #[test]
    fn struct_named_vec3_collides_with_tower_kind_is_e188() {
        let (hir, _index) = build("STRUCT vec3 = #{x: float, y: float, z: float}\n");
        let diags = check_reserved_type_names(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E188);
    }

    /// Should-NOT-fire: an ordinary struct name that collides with nothing
    /// stays clean.
    #[test]
    fn ordinary_struct_name_gets_no_e188() {
        let (hir, _index) = build("STRUCT Point = #{x: float, y: float}\n");
        let diags = check_reserved_type_names(&[(FileId(0), &hir)]);
        assert!(diags.is_empty(), "{diags:?}");
    }

    /// Should-NOT-fire, truthfully verified rather than assumed: `void` has
    /// no explicit arm in `resolve`'s `Named` match at all (unlike the
    /// scalar leaves) — an unmatched name falls straight through to the
    /// ordinary struct lookup, so a `STRUCT void` is never actually
    /// shadowed. Confirms both halves: no `E188`, and `resolve` genuinely
    /// does resolve the struct.
    #[test]
    fn struct_named_void_is_not_shadowed_and_resolves_fine() {
        let (hir, index) = build("STRUCT void = #{x: int}\nVAR v: void = 0\n");
        let diags = check_reserved_type_names(&[(FileId(0), &hir)]);
        assert!(
            diags.is_empty(),
            "`void` has no collision to report: {diags:?}"
        );

        let names = TypeNames::new(&index, None);
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        assert_eq!(
            resolve(te, &names),
            Some(Ty::Struct("void".to_string())),
            "a STRUCT named `void` must resolve fine — `resolve`'s Named arm has no \
             explicit void case, so it falls through to the struct lookup"
        );
    }

    /// Should-NOT-fire, truthfully verified: the generic heads
    /// (`List`/`Array`/`Map`/`Option`/`Weighted`/`Handle`) are only
    /// special-cased inside `TypeExpr::Generic`'s own dispatch — a *bare*
    /// `Named` reference to a struct sharing one of those names still
    /// resolves through the ordinary struct-lookup arm. Confirms both
    /// halves for `Array`, same shape as the `void` test above.
    #[test]
    fn struct_named_array_stays_reachable_and_is_not_flagged() {
        let (hir, index) = build("STRUCT Array = #{x: int}\nVAR v: Array = 0\n");
        let diags = check_reserved_type_names(&[(FileId(0), &hir)]);
        assert!(
            diags.is_empty(),
            "a bare `Array` annotation has no collision — Array is only special-cased \
             inside TypeExpr::Generic, never TypeExpr::Named: {diags:?}"
        );

        let names = TypeNames::new(&index, None);
        let te = hir.variables[0].annotation.as_ref().expect("annotation");
        assert_eq!(
            resolve(te, &names),
            Some(Ty::Struct("Array".to_string())),
            "a bare `Array` annotation must resolve to the struct, not silently fail"
        );
    }

    /// Multiple colliding structs in one file each get their own
    /// diagnostic, at their own declaration's range.
    #[test]
    fn multiple_colliding_structs_each_get_their_own_e188() {
        let (hir, _index) = build(
            "STRUCT content = #{x: int}\nSTRUCT bool = #{y: int}\nSTRUCT Point = #{z: int}\n",
        );
        let diags = check_reserved_type_names(&[(FileId(0), &hir)]);
        assert_eq!(diags.len(), 2, "{diags:?}");
        assert!(diags.iter().all(|d| d.code == DiagnosticCode::E188));
        let ranges: BTreeSet<(u32, u32)> = diags
            .iter()
            .map(|d| (d.range.start().into(), d.range.end().into()))
            .collect();
        assert_eq!(
            ranges.len(),
            2,
            "each collision must point at its own declaration"
        );
    }
}
