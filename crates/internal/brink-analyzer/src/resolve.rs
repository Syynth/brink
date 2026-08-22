use std::collections::{BTreeMap, BTreeSet};

use brink_format::DefinitionId;
use brink_ir::{
    Diagnostic, DiagnosticCode, FileId, Import, LocalSymbol, RefKind, ResolutionMap, ResolvedRef,
    Scope, SymbolIndex, SymbolInfo, SymbolKind, SymbolManifest, Visibility,
    is_reserved_root_module,
};

use crate::manifest::local_definition_id;

/// Per-file import coverage, shared verbatim by resolution ([`ImportScope`])
/// and the E025 import-required checker (`modules::import_covers`): the set
/// of modules imported **qualified** (`IMPORT mod`, which licenses
/// `module.name` access to any public export) and the `(module, name)` pairs
/// brought into scope by **bare** imports (`IMPORT { name } FROM mod`, which
/// is name-precise — it does *not* license every other export of `mod`).
///
/// A single source of truth for this distinction: an earlier version of
/// `ImportScope` collapsed every import to just its module name, so a bare
/// `IMPORT { other } FROM mod` was (wrongly) treated as importing *all* of
/// `mod`, silently disagreeing with `import_covers`'s name-precise gate.
///
/// # Dual-reading a bare item's trailing segment (issue #1592)
///
/// `use story::market::barter;` lowers to a bare import whose sole item is
/// `barter` "from" module `story::market` (`lower_native::import`'s
/// item-is-the-leaf reading) — but Rust's `use`, which charter §13.2
/// commits to lifting verbatim, dual-reads that trailing segment: `barter`
/// may be an *item* `story::market` exports, or it may itself name the
/// **module** `story::market::barter` (a real submodule, licensing
/// **module-qualified** access to its own public exports — "a trailing
/// segment that resolves to a module licenses that module, exactly as
/// Rust's `use` does", per the #1592 ruling, `docs/decision-log.md`
/// 2026-07-27). Same shape for every entry in a nested list (`use a::{b,
/// c};` dual-reads `b` and `c` independently, exactly as `use a::b;`
/// dual-reads `b`).
///
/// This is a **per-file** query (`resolve(FileId)`'s incremental contract:
/// "reads only the symbol index and this file's own manifest — never
/// another file's content") and has no way to know here whether
/// `module::name` is a real declared module elsewhere in the project — that
/// whole-project view exists only in `modules::check`. So both readings are
/// licensed **unconditionally**: the item pairing is inserted into `bare`
/// exactly as before, and `module::name` is *also* inserted into
/// `qualified` as a phantom qualified-module candidate. This is a pure
/// no-op unless some file's symbol genuinely carries that exact module
/// name — `classify` only ever matches a *candidate's own* `info.module`
/// against this set, so an unreal phantom module simply never matches
/// anything. **Precedence decision (issue #1592, "decide and document"):
/// both readings apply — there is no exclusion.** They populate disjoint,
/// non-conflicting sets (`bare` is name-precise; `qualified` is
/// module-wide), so a name that resolves as *both* an item of `module` and
/// a module in its own right gets both: the item is bare-importable under
/// its own name (via `bare`), and the submodule's public exports become
/// reachable **qualified** — `barter::haggle`, never bare `haggle` (via
/// `qualified`). This mirrors Rust's own per-namespace `use` semantics
/// (a module and a value can share a name without conflict) without this
/// codebase needing to model namespaces explicitly. `modules::check`'s
/// `E088` is the check that validates *this* file's readings against real
/// project-wide module/export data and diagnoses when a trailing segment
/// resolves to **neither**.
///
/// ⚠ **Corrected 2026-08-05 (issue #2287), widened 2026-08-22 (issue
/// #2298).** This doc previously claimed the submodule reading makes its
/// exports "bare-visible" / "referenceable by bare name" — that was an
/// over-read of the 2026-07-27 ruling and was itself the reported bug: a
/// `qualified` entry alone must never license a *bare* reference
/// (`is_qualified_import_only`, enforced via the shared
/// `lookup_bare_excluding_qualified_only` helper both `lookup_divert`'s
/// `Knot`/`Stitch`/`Label`/`Variable`+`Constant` steps and
/// `resolve_function`'s knot-as-tunnel-function step now go through —
/// #2287 shipped only the `Knot`-in-a-divert case; #2298 closed the
/// deliberate remainder). `qualified_modules.contains(module)` still
/// legitimately grants every *other* reference kind (calls resolving to an
/// `External`/`Variable`/`Constant`, struct literals, etc.)
/// `Candidacy::Imported` for their own qualified-syntax forms via the
/// ordinary flat `classify`/`lookup_by_name` path; nothing about that
/// changed.
#[must_use]
pub(crate) fn import_coverage_for_file(
    imports: &[Import],
) -> (BTreeSet<String>, BTreeSet<(&str, &str)>) {
    let mut qualified = BTreeSet::new();
    let mut bare = BTreeSet::new();
    for import in imports {
        if import.bare {
            for item in &import.items {
                bare.insert((import.module.as_str(), item.name.as_str()));
                // Dual-reading (issue #1592, doc above): `item.name` might
                // itself name a submodule of `import.module` rather than an
                // item of it. `::`-joining is native's real module-path
                // separator (`brink_db::modules::native_module_path`), but
                // `#@module(...)` places no structural constraint on an ink
                // module's own name either (it accepts any non-empty
                // string, `::`-joined or not — see
                // `modules::known_module_names`'s doc, corrected by the
                // #1686 review), so this is *not* a structural ink no-op.
                // It is a no-op only for the corpus this compiler actually
                // has to stay byte-identical for — no `#@module`/`IMPORT`/
                // `use` construct appears anywhere in the oracle/tier1
                // corpus at all (`modules`'s own Compat doc).
                //
                // Unconditional on `item.alias`, deliberately: this phantom
                // candidate is inserted whether or not the item was
                // aliased, so an aliased trailing segment that resolves to
                // a module (`use a::b as c;` where `b` is a submodule)
                // still licenses qualified access to `b`'s exports under
                // `b`'s *own* name (`b::item`, issue #2287), even though
                // `c` binds nothing useful. That shape
                // has no sound alias representation at all (aliasing a
                // whole module's export set, not one name) and is
                // diagnosed loudly at the whole-project level instead —
                // `modules::check`'s `E129` fires when this pass's
                // `is_module` reading and `item.alias.is_some()` coincide.
                qualified.insert(format!("{}::{}", import.module, item.name));
            }
        } else {
            qualified.insert(import.module.clone());
        }
    }
    (qualified, bare)
}

/// Per-file import context threaded into resolution (M-2d,
/// docs/modules-spec.md §2; issue #790) so a bare reference with multiple
/// cross-module candidates binds to the module *this file* actually imports —
/// "names cross module boundaries only via import" — rather than to the flat
/// duplicate-winner.
///
/// The default (empty) scope reproduces the pre-M-2d flat behavior exactly:
/// the entire strict-ink and single-module world has at most one candidate
/// per (name, kind), so [`lookup_by_name`]'s fast path returns it unchanged
/// and this context never influences the result. It only ever disambiguates
/// the genuinely-new case unlocked by relaxing the #784/#793 stopgap: two
/// *declared* modules publicly defining the same name, now coexisting in the
/// index instead of one being suppressed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportScope {
    /// The referring file's own **declared** module (`None` for an
    /// undeclared stem-module / the legacy world). A candidate declared in
    /// this same module is bare-visible without any import.
    pub file_module: Option<String>,
    /// Modules this file imports **qualified** (`IMPORT mod`) — licenses
    /// `module.name` access to any public export of that module.
    pub qualified_modules: BTreeSet<String>,
    /// `(module, name)` pairs this file imports **bare**
    /// (`IMPORT { name } FROM mod`) — name-precise, matching
    /// `modules::import_covers` exactly so resolution and the E025
    /// import-required diagnostic can never diverge. `BTreeSet` for
    /// determinism. Keyed by the imported item's own (source-module) name
    /// regardless of any local alias — an aliased import still *covers* its
    /// source name for cross-module licensing purposes (§2: the file did
    /// import it), it just isn't the name resolution binds bare (see
    /// `aliases`).
    pub bare_imports: BTreeSet<(String, String)>,
    /// Local alias → `(module, source_name)` for every bare import item that
    /// named one (`IMPORT { name AS alias } FROM mod` / `use mod::name as
    /// alias;`, issue #1590). `index.by_name` is keyed by definitions' own
    /// spellings only, so a plain [`lookup_by_name`] lookup can never find an
    /// alias — this table is the indirection [`lookup_by_name`] falls back to
    /// once the direct-name lookup comes up empty. Additive, not
    /// shadowing: aliasing doesn't revoke the source name's own bare
    /// visibility (still governed by `bare_imports`/`classify` exactly as
    /// before) — it only adds a second local spelling for the same import.
    /// See the doc comment on [`lookup_by_name`] for the alias-vs-original
    /// licensing ruling. `BTreeMap` for determinism.
    pub aliases: BTreeMap<String, (String, String)>,
}

impl ImportScope {
    /// Build the scope for one file from its resolved (declared) module and
    /// its HIR `IMPORT` list.
    #[must_use]
    pub fn new(file_module: Option<String>, imports: &[Import]) -> Self {
        let (qualified, bare) = import_coverage_for_file(imports);
        let mut aliases = BTreeMap::new();
        for import in imports {
            if !import.bare {
                continue;
            }
            for item in &import.items {
                if let Some(alias) = &item.alias {
                    aliases.insert(alias.clone(), (import.module.clone(), item.name.clone()));
                }
            }
        }
        Self {
            file_module,
            qualified_modules: qualified,
            bare_imports: bare
                .into_iter()
                .map(|(module, name)| (module.to_string(), name.to_string()))
                .collect(),
            aliases,
        }
    }
}

/// How a candidate definition relates to the referring file's import scope.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Candidacy {
    /// Bare-visible without an import: the legacy world (`module == None`) or
    /// a definition in the referrer's own declared module.
    InScope,
    /// A public definition in a **declared** module this file imports.
    Imported,
    /// Neither — a cross-module definition this file has no line of sight to.
    Other,
}

/// Classify a candidate against the referring file's import scope (M-2d).
fn classify(scope: &ImportScope, info: &SymbolInfo) -> Candidacy {
    match &info.module {
        // Undeclared stem-module / legacy soup — always bare-visible, so the
        // pre-modules corpus is untouched.
        None => Candidacy::InScope,
        Some(module) => {
            if scope.file_module.as_deref() == Some(module.as_str()) {
                Candidacy::InScope
            } else if info.visibility == Visibility::Public
                && (scope.qualified_modules.contains(module)
                    || scope
                        .bare_imports
                        .contains(&(module.clone(), info.name.clone())))
            {
                Candidacy::Imported
            } else {
                Candidacy::Other
            }
        }
    }
}

/// Resolve all unresolved references across files.
///
/// Per-file concatenation of [`resolve_file`], preserving input file order.
/// The production orchestrator (`analyze_with_options`) drives the per-file
/// [`crate::resolve`] query directly; this whole-project wrapper survives as
/// a test convenience.
#[cfg(test)]
pub fn resolve_refs(
    index: &SymbolIndex,
    files: &[(FileId, &SymbolManifest)],
) -> (ResolutionMap, Vec<Diagnostic>) {
    let mut map = ResolutionMap::new();
    let mut diagnostics = Vec::new();

    let scope = ImportScope::default();
    for &(file_id, manifest) in files {
        let (file_map, file_diags) = resolve_file(index, &scope, file_id, manifest);
        map.extend(file_map);
        diagnostics.extend(file_diags);
    }

    (map, diagnostics)
}

/// Resolve one file's unresolved references against the project-wide index.
///
/// Reads only the symbol index and this file's own manifest — never another
/// file's content. This is the per-file dependency seam the query pipeline
/// relies on (substrate spec §4, layer 2 — `resolve(FileId)`).
///
/// Local (param/temp) lookups read `manifest.locals` — this file's own
/// side table — rather than the project-wide index (issue #517): a knot's
/// body lives in exactly one file, so a local can never legitimately be
/// declared in one file and referenced from another. Scoping the lookup to
/// this file's own locals both restores correct behavior for cross-file
/// duplicate-scoped-locals (slice-A finding 4 — no more merged-index
/// aliasing) and lets the project-wide `resolution_index` drop locals
/// entirely, so a body edit that adds/removes a `~ temp` in file Y no
/// longer invalidates file X's `resolve` memo.
pub fn resolve_file(
    index: &SymbolIndex,
    scope: &ImportScope,
    file_id: FileId,
    manifest: &SymbolManifest,
) -> (ResolutionMap, Vec<Diagnostic>) {
    let mut map = ResolutionMap::new();
    let mut diagnostics = Vec::new();
    let locals = &manifest.locals;

    for uref in &manifest.unresolved {
        match uref.kind {
            RefKind::Divert => {
                resolve_divert(
                    index,
                    scope,
                    locals,
                    file_id,
                    uref,
                    &mut map,
                    &mut diagnostics,
                );
            }
            RefKind::Variable => {
                resolve_variable(
                    index,
                    scope,
                    locals,
                    file_id,
                    uref,
                    &mut map,
                    &mut diagnostics,
                );
            }
            RefKind::Function => {
                resolve_function(
                    index,
                    scope,
                    locals,
                    file_id,
                    uref,
                    &mut map,
                    &mut diagnostics,
                );
            }
            RefKind::List => {
                resolve_list_ref(index, scope, file_id, uref, &mut map, &mut diagnostics);
            }
            RefKind::Struct => {
                resolve_struct_ref(index, scope, file_id, uref, &mut map, &mut diagnostics);
            }
            RefKind::Type => {
                resolve_type_ref(index, scope, file_id, uref, &mut map);
            }
        }
    }

    (map, diagnostics)
}

fn resolve_divert(
    index: &SymbolIndex,
    scope: &ImportScope,
    locals: &[LocalSymbol],
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    if let Some(id) = lookup_divert(index, scope, locals, uref) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        check_divert_arity(index, file_id, uref, id, diagnostics);
    } else {
        diagnostics.push(unresolved_diag(
            index,
            scope,
            file_id,
            uref.range,
            &uref.path,
            DiagnosticCode::E024,
            &[
                SymbolKind::Knot,
                SymbolKind::Stitch,
                SymbolKind::Label,
                SymbolKind::Variable,
                SymbolKind::Constant,
            ],
        ));
    }
}

/// Check a divert-with-args site's argument count against its resolved
/// target's declared parameter count (`E176`, issue #2156) — `E031`'s
/// sibling for the divert call shape (`-> knot(args)`, a tunnel call, or a
/// thread-start), extended to a construct `check_arity` never covered:
/// `RefKind::Divert` refs always carried `arg_count: None` until this issue
/// (`brink_ir::symbols::project::Projector::walk_divert_target`), so
/// `check_arity` — gated on `arg_count.is_some()` — could never fire for a
/// divert on either dialect regardless of how many arguments were given.
///
/// Scoped to a resolution naming a `Knot`/`Stitch`/`Label` — the only
/// symbol kinds with their own declared parameter row — mirroring
/// `resolve_function`'s own `check_arity` call sites, which likewise check
/// only `External`/`Knot` resolutions and skip `Variable`/local ones. A
/// divert resolving to a `Variable` (`-> x` where `x` holds a stored divert
/// target, e.g. `docs/…/WritingWithInk.md`'s "Advanced: sending divert
/// targets as parameters") or to a divert-typed local `Param` (`-> return_to`
/// inside `=== knot(-> return_to) ===`) is an indirection whose real
/// target's arity is not known statically at this site — `index.symbols`
/// has no declared parameter row for either kind, so checking against it
/// would misfire on legitimate code instead of silently doing nothing
/// useful.
fn check_divert_arity(
    index: &SymbolIndex,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    target: DefinitionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(call_arg_count) = uref.arg_count else {
        return;
    };
    let Some(info) = index.symbols.get(&target) else {
        return;
    };
    if !matches!(
        info.kind,
        SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label
    ) {
        return;
    }
    let expected = info.params.len();
    if call_arg_count != expected {
        diagnostics.push(Diagnostic {
            file: file_id,
            range: uref.range,
            message: format!(
                "{}: `{}` expects {} argument(s), got {}",
                DiagnosticCode::E176.title(),
                uref.path,
                expected,
                call_arg_count,
            ),
            code: DiagnosticCode::E176,
        });
    }
}

fn lookup_divert(
    index: &SymbolIndex,
    scope: &ImportScope,
    locals: &[LocalSymbol],
    uref: &brink_ir::UnresolvedRef,
) -> Option<DefinitionId> {
    let path = &uref.path;

    // Module-qualified (`-> barter::haggle`, issue #2287): resolved
    // entirely separately from ink's own dotted `knot.stitch` addressing
    // below — a module qualifier licenses reaching a top-level flow through
    // an imported *module*, never a stitch/label path, and a miss here must
    // not fall through to the single-segment resolution below (that
    // fallback treating the *whole* qualified path as an opaque bare name
    // is exactly how issue #2287's bug (b) let `-> haggle` slip through
    // after the qualified form failed).
    if uref.module_qualified {
        return lookup_qualified_divert(index, scope, path);
    }

    // Dotted path — try exact qualified lookup, then qualify with current knot
    if path.contains('.') {
        if let Some(id) =
            lookup_by_name(index, scope, path, &[SymbolKind::Stitch, SymbolKind::Label])
        {
            return Some(id);
        }
        // Try qualifying with current knot scope (e.g., `a_package.forest` → `adventure.a_package.forest`)
        if let Some(knot) = &uref.scope.knot {
            let qualified = format!("{knot}.{path}");
            if let Some(id) = lookup_by_name(
                index,
                scope,
                &qualified,
                &[SymbolKind::Stitch, SymbolKind::Label],
            ) {
                return Some(id);
            }
        }
        return None;
    }

    // Single segment — ink's hierarchical resolution:
    // 1. Stitch or label in current knot
    if let Some(knot) = &uref.scope.knot {
        let qualified = format!("{knot}.{path}");
        if let Some(id) = lookup_by_name(
            index,
            scope,
            &qualified,
            &[SymbolKind::Stitch, SymbolKind::Label],
        ) {
            return Some(id);
        }
        // Label in current stitch (knot.stitch.label)
        if let Some(stitch) = &uref.scope.stitch
            && let Some(id) = lookup_by_name(
                index,
                scope,
                &format!("{knot}.{stitch}.{path}"),
                &[SymbolKind::Label],
            )
        {
            return Some(id);
        }
    }

    // 2. Knot at top level — a *bare* divert name (no qualifier segment at
    // all in source) must resolve only to a same-module candidate or one a
    // symbol-level/glob `use` actually named (issue #2287): unlike the flat
    // `lookup_by_name`/`classify` most other reference kinds use (which
    // also treats a **qualified**-module import as licensing bare access,
    // since e.g. a function-call callee may itself be spelled either way),
    // a bare divert has no qualifier to license through — a candidate
    // reachable only via a qualified-module import must stay unresolved
    // here, or `-> haggle` would silently accept exactly the over-permissive
    // reading #2287 reported as bug (b).
    if let Some(id) = lookup_knot_bare(index, scope, path) {
        return Some(id);
    }

    // 3. Top-level stitch (bare name, no parent knot). Issue #2298: same
    // `is_qualified_import_only` exclusion as step 2, via the kind-generic
    // [`lookup_bare_excluding_qualified_only`] — a stitch reachable only
    // through a qualified-module import must not resolve a bare divert any
    // more than a knot may. Currently unreachable for native (a `flow`
    // always classifies `SymbolKind::Knot`, never `Stitch`, so no
    // top-level-stitch candidate can carry a real cross-module `module` for
    // this exclusion to fire against) — latent, not dead: fixed here
    // alongside steps 5–6 so the whole post-knot tail obeys one rule
    // instead of the Knot-only gate #2296 shipped, per issue #2298's scope.
    if let Some(id) =
        lookup_bare_excluding_qualified_only(index, scope, path, &[SymbolKind::Stitch])
    {
        return Some(id);
    }

    // 4. Label anywhere in current knot (search by suffix). Deliberately
    // untouched by issue #2298: `knot` here is always `uref.scope.knot`,
    // i.e. the *referring file's own* currently-open knot — a stitch or
    // label found this way is always declared alongside it in the same
    // file, hence the same declared module, hence `Candidacy::InScope`
    // regardless of any import. There is no cross-module reading for
    // `lookup_label_in_knot` to leak through, so the exclusion has nothing
    // to guard here.
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_label_in_knot(index, scope, knot, path)
    {
        return Some(id);
    }

    // 5. Top-level label — stored as bare name (visible from any scope).
    // Issue #2298: same reasoning as step 3.
    if let Some(id) = lookup_bare_excluding_qualified_only(index, scope, path, &[SymbolKind::Label])
    {
        return Some(id);
    }

    // 6. Variable divert target (`VAR x = -> knot`, then `-> x`). Issue
    // #2298: same `is_qualified_import_only` exclusion as steps 2–3/5, via
    // the shared kind-generic lookup — plus the `SymbolKind::Constant`
    // omission recorded on issue #2083's thread (crediting that report):
    // `resolve_function`'s own Variable-divert-target-shaped lookup (its
    // "Try variables and constants" step) was widened to `[Variable,
    // Constant]` by #2947 for the call-site gap #2083 reported, but this
    // divert-target step was left `Variable`-only as a "next-wave
    // candidate" — a `CONST x = -> knot` could never be diverted to via
    // `-> x` even though the call-site twin already accepts a const-bound
    // value. Fixed here in the same pass as the exclusion, not reordered
    // to locals-first the way #2947 reordered `resolve_function`: step 7
    // below already runs after this one, and no test or issue here asks
    // for a local param to shadow a same-named global `VAR`/`CONST` at a
    // divert site — widening the kind list is the whole of what issue
    // #2298 asks for at this step.
    if let Some(id) = lookup_bare_excluding_qualified_only(
        index,
        scope,
        path,
        &[SymbolKind::Variable, SymbolKind::Constant],
    ) {
        return Some(id);
    }

    // 7. Divert parameter in scope (`=== knot(-> x) ===` then `-> x`)
    lookup_local_in_scope(locals, path, &uref.scope)
}

/// Is `info` reachable *only* through a **qualified-module** import
/// (`use ...::mod;` / `import mod;`, `scope.qualified_modules`) with no
/// corresponding bare (symbol-level/glob) import of this exact name (issue
/// #2287)? A qualified-module import licenses `module::name`/`module.name`
/// access to any public export — [`classify`]'s `Candidacy::Imported`
/// correctly grants that for reference kinds whose own syntax can carry a
/// qualifier — but it must never *also* license the bare `name` spelling,
/// which only a symbol-level or glob import brings into scope. `false` for
/// a candidate with no module at all (the legacy/undeclared-module world,
/// always bare-visible) and for one already bare-imported (both readings
/// can hold at once per #1592's dual-reading ruling; the bare one still
/// wins).
fn is_qualified_import_only(scope: &ImportScope, info: &SymbolInfo) -> bool {
    let Some(module) = &info.module else {
        return false;
    };
    info.visibility == Visibility::Public
        && scope.qualified_modules.contains(module)
        && !scope
            .bare_imports
            .contains(&(module.clone(), info.name.clone()))
}

/// Bare (single-segment, no qualifier at all in source) top-level-`Knot`
/// divert lookup (issue #2287). Deliberately duplicates
/// [`lookup_by_name_direct`]'s own fast-path/tie-break structure — same
/// std-reserved-root skip, same "`!multiple` sole match wins regardless of
/// candidacy" byte-identity guarantee that lets a totally *unimported*
/// cross-module `Knot` still resolve here exactly as before (its specific
/// diagnostic — `E087` private-cross-module or `E025` import-required — is
/// `modules::check`'s separate, more precise gate to raise on the
/// resulting `ResolvedRef`, not this function's job to pre-empt) — with
/// exactly one new exclusion, structurally parallel to the std skip: a
/// candidate reachable *only* via a qualified-module import
/// ([`is_qualified_import_only`]) is skipped before it can ever win, since
/// a module import must never license the bare spelling (bug (b)). Not a
/// thin wrapper over `lookup_by_name`/`classify` because that fast path
/// applies its own exclusion **before** counting a candidate into
/// `first_match`/`multiple`, exactly where this one must too — bolting the
/// exclusion on afterward would still let a qualified-only candidate win
/// as the "sole" match.
///
/// Issue #2298 widened this beyond diverts: `resolve_function`'s "try
/// knots" step (ink allows a knot as a function via tunnels — the same
/// divert-shaped addressing space, unlike a call resolving to an
/// `External`/`Variable`/`Constant`, which stays on the ordinary
/// `classify`-based rule per this module's top-of-file doc) reproduced bug
/// (b) for a bare *call* `haggle()`, so it now shares this exact lookup
/// rather than drifting its own copy. Thin wrapper over
/// [`lookup_bare_excluding_qualified_only`], kept as its own named function
/// (rather than inlining `&[SymbolKind::Knot]` at both call sites) because
/// this name is what the doc comments elsewhere in this file point readers
/// at.
fn lookup_knot_bare(index: &SymbolIndex, scope: &ImportScope, name: &str) -> Option<DefinitionId> {
    lookup_bare_excluding_qualified_only(index, scope, name, &[SymbolKind::Knot])
}

/// The kind-generic form of [`lookup_knot_bare`] (issue #2298): the same
/// bare, qualified-import-only-excluding lookup, parameterized over the
/// symbol-kind set so [`lookup_divert`]'s later steps (`Stitch`/`Label`/
/// `Variable`+`Constant`) can share this one implementation too, instead of
/// each hand-copying [`lookup_knot_bare_direct`]'s structure the way that
/// function itself once hand-copied [`lookup_by_name_direct`]'s.
fn lookup_bare_excluding_qualified_only(
    index: &SymbolIndex,
    scope: &ImportScope,
    name: &str,
    kinds: &[SymbolKind],
) -> Option<DefinitionId> {
    if let Some(id) = lookup_bare_excluding_qualified_only_direct(index, scope, name, kinds) {
        return Some(id);
    }
    // Alias fallback (issue #1590), mirroring `lookup_by_name`'s own: an
    // aliased bare-import item (`use ...::haggle as h;`) is never in
    // `index.by_name` under its local alias, so the direct lookup above
    // can never find it — `scope.aliases` is the indirection. The alias
    // entry's mere presence already proves this file bare-imported that
    // exact `(module, source_name)` pair (`ImportScope::new` only ever
    // populates it from a bare import item that named an alias), so no
    // further candidacy check is needed here, exactly as `lookup_by_name`
    // performs none either.
    let (module, source_name) = scope.aliases.get(name)?;
    let ids = index.by_name.get(source_name.as_str())?;
    ids.iter().find_map(|id| {
        let info = index.symbols.get(id)?;
        (kinds.contains(&info.kind) && info.module.as_deref() == Some(module.as_str()))
            .then_some(*id)
    })
}

/// The direct (non-alias) half of [`lookup_bare_excluding_qualified_only`] —
/// see that function's own doc, and [`lookup_knot_bare`]'s original doc, for
/// why this duplicates [`lookup_by_name_direct`]'s structure rather than
/// reusing it.
fn lookup_bare_excluding_qualified_only_direct(
    index: &SymbolIndex,
    scope: &ImportScope,
    name: &str,
    kinds: &[SymbolKind],
) -> Option<DefinitionId> {
    let ids = index.by_name.get(name)?;
    let mut first_match = None;
    let mut first_in_scope = None;
    let mut first_imported = None;
    let mut multiple = false;
    for id in ids {
        let Some(info) = index.symbols.get(id) else {
            continue;
        };
        if !kinds.contains(&info.kind) {
            continue;
        }
        let candidacy = classify(scope, info);
        if candidacy == Candidacy::Other
            && info.module.as_deref().is_some_and(is_reserved_root_module)
        {
            continue;
        }
        if is_qualified_import_only(scope, info) {
            continue;
        }
        if first_match.is_none() {
            first_match = Some(*id);
        } else {
            multiple = true;
        }
        match candidacy {
            Candidacy::InScope if first_in_scope.is_none() => first_in_scope = Some(*id),
            Candidacy::Imported if first_imported.is_none() => first_imported = Some(*id),
            _ => {}
        }
    }
    if !multiple {
        return first_match;
    }
    first_in_scope.or(first_imported).or(first_match)
}

/// Does `module` (a candidate's real, fully `::`-joined declared module
/// name) match a divert's own written qualifier segment(s) — the prefix of
/// `-> qualifier::name` before the final `::`? Exact match covers the
/// common single-level case (`barter` naming module `story::market::barter`
/// end-to-end would require `qualifier == module`); the suffix form covers
/// a qualifier that only spells the module's own trailing component(s),
/// the shape `use story::market::barter;`'s dual-reading licenses (issue
/// #1592) and issue #2287's own repro exercises.
fn module_matches_qualifier(module: &str, qualifier: &str) -> bool {
    module == qualifier || module.ends_with(&format!("::{qualifier}"))
}

/// Module-qualified divert lookup (`-> barter::haggle`, issue #2287):
/// `path` is `uref.path` when `uref.module_qualified` is set —
/// `::`-joined, never `.`-joined, so it splits cleanly into the qualifier
/// prefix and the bare target name. Resolves only against a top-level
/// `Knot` (the sole symbol kind a native `flow` divert target names) whose
/// real declared module matches the qualifier
/// ([`module_matches_qualifier`]) and which this file imported *qualified*
/// (`scope.qualified_modules` — a symbol-level/glob import does not carry
/// this licensing power for the qualified spelling, mirroring
/// [`classify`]'s own module-qualified-import reading).
fn lookup_qualified_divert(
    index: &SymbolIndex,
    scope: &ImportScope,
    path: &str,
) -> Option<DefinitionId> {
    let (qualifier, name) = path.rsplit_once("::")?;
    let ids = index.by_name.get(name)?;
    ids.iter().find_map(|id| {
        let info = index.symbols.get(id)?;
        if info.kind != SymbolKind::Knot || info.visibility != Visibility::Public {
            return None;
        }
        let module = info.module.as_deref()?;
        if !module_matches_qualifier(module, qualifier) {
            return None;
        }
        scope.qualified_modules.contains(module).then_some(*id)
    })
}

fn resolve_variable(
    index: &SymbolIndex,
    scope: &ImportScope,
    locals: &[LocalSymbol],
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    match lookup_variable(index, scope, locals, uref) {
        VarResult::Found(id) => {
            map.push(ResolvedRef {
                file: file_id,
                range: uref.range,
                target: id,
            });
        }
        VarResult::Ambiguous => {
            diagnostics.push(ambiguous_diag(file_id, uref.range, path));
        }
        VarResult::NotFound => {
            // NS-A1 (`docs/stdlib-spec.md` §1.4): an otherwise-unresolved
            // bare `none` is the brink-dialect Option absence literal —
            // same "skip resolution, no diagnostic here" treatment as the
            // T1b stdlib call names in `resolve_function`. Every user
            // symbol interpretation above wins first (a LIST item, VAR,
            // temp, … named `none` shadows the literal, E035-warned at its
            // declaration); `strict-ink` rejection is the dialect gate's
            // job, and the bare-`none`-needs-context declaration rule is
            // E107 (`option_rules`).
            if path == "none" {
                return;
            }

            // Issue #2856 point 3: `is_builtin_function` is checked here —
            // as a post-lookup fallback — not before `lookup_variable`
            // runs, so a declared `VAR`/list item/knot/… of the same name
            // (`manifest.rs`'s `E035` "name shadows a built-in function"
            // warning already documents this as legal, shadowing behavior)
            // wins resolution first. Only a genuinely unresolved reference
            // to one of these classic uppercase ink intrinsics
            // (`TURNS_SINCE`/`RANDOM`/…) falls through to this silent skip
            // — mirroring `is_t1b_stdlib_name`'s already-correct ordering
            // below in `resolve_function`. Before this fix the check ran
            // unconditionally first, so a declared symbol could never
            // shadow it: `VAR RANDOM = 42` / `{RANDOM}` silently dropped
            // the reference (no resolution, no diagnostic) instead of
            // reading 42 — reproduced end-to-end via `brink-cli`.
            if is_builtin_function(path) {
                return;
            }
            diagnostics.push(unresolved_diag(
                index,
                scope,
                file_id,
                uref.range,
                path,
                DiagnosticCode::E025,
                &[],
            ));
        }
    }
}

enum VarResult {
    Found(DefinitionId),
    Ambiguous,
    NotFound,
}

/// Hierarchical variable lookup — returns the first match in priority order.
fn lookup_variable(
    index: &SymbolIndex,
    scope: &ImportScope,
    locals: &[LocalSymbol],
    uref: &brink_ir::UnresolvedRef,
) -> VarResult {
    let path = &uref.path;

    // 1. Locals (params/temps) in scope — they shadow globals
    if let Some(id) = lookup_local_in_scope(locals, path, &uref.scope) {
        return VarResult::Found(id);
    }

    // 2. Global variables / constants
    if let Some(id) = lookup_by_name(
        index,
        scope,
        path,
        &[SymbolKind::Variable, SymbolKind::Constant],
    ) {
        return VarResult::Found(id);
    }

    // 3. List items by bare name
    match lookup_list_item_bare(index, path) {
        BareItemResult::Unique(id) => return VarResult::Found(id),
        BareItemResult::Ambiguous => return VarResult::Ambiguous,
        BareItemResult::NotFound => {}
    }

    // 4. Qualified list item (ListName.ItemName)
    if path.contains('.')
        && let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::ListItem])
    {
        return VarResult::Found(id);
    }

    // 5. List names
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::List]) {
        return VarResult::Found(id);
    }

    // 6. Knots and top-level stitches (visit counts)
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Knot, SymbolKind::Stitch]) {
        return VarResult::Found(id);
    }

    // 7. Stitches in current knot scope
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_by_name(
            index,
            scope,
            &format!("{knot}.{path}"),
            &[SymbolKind::Stitch],
        )
    {
        return VarResult::Found(id);
    }

    // 8. Qualified stitch/label (e.g. `knot.stitch` or `knot.stitch.label` visit count)
    if path.contains('.') {
        if let Some(id) =
            lookup_by_name(index, scope, path, &[SymbolKind::Stitch, SymbolKind::Label])
        {
            return VarResult::Found(id);
        }
        // Try `knot.label` where label is stored as `knot.*.label` (label inside a stitch)
        if let Some((knot, label)) = path.split_once('.')
            && !label.contains('.')
            && let Some(id) = lookup_label_in_knot(index, scope, knot, label)
        {
            return VarResult::Found(id);
        }
    }

    // 9. Labels in current knot
    if let Some(knot) = &uref.scope.knot
        && let Some(id) = lookup_label_in_knot(index, scope, knot, path)
    {
        return VarResult::Found(id);
    }

    // 10. Labels at top level (no knot scope)
    if uref.scope.knot.is_none()
        && let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Label])
    {
        return VarResult::Found(id);
    }

    // 11. TM-4b resolution fallback (docs/typed-mode-spec.md §6): every
    // static dotted-path interpretation above (steps 3-10: list items,
    // lists, knots/stitches, labels — "ink's static dotted paths... resolved
    // first and win") has failed. If the path has more than one segment and
    // its *head* segment alone resolves to a local (param/temp) or a global
    // variable/constant, this is field access on that variable
    // (`p.x`/`p.x.y`) rather than an unresolved static path — the resolved
    // target is the head variable itself; the trailing segment(s) are field
    // names carried structurally by the HIR `Path`, not by this resolution.
    // Struct field-name validity (does `x` exist on `p`'s declared shape?) is
    // a separate construction-time concern (`brink-analyzer::structs`), not
    // resolution's. A single-segment path can never reach here having
    // already failed steps 1-2 above (which already check locals/globals for
    // the *whole* path), so this never fires for a bare variable reference.
    if let Some((head, _rest)) = path.split_once('.') {
        if let Some(id) = lookup_local_in_scope(locals, head, &uref.scope) {
            return VarResult::Found(id);
        }
        if let Some(id) = lookup_by_name(
            index,
            scope,
            head,
            &[SymbolKind::Variable, SymbolKind::Constant],
        ) {
            return VarResult::Found(id);
        }
    }

    VarResult::NotFound
}

/// Resolve a call-path reference (`RefKind::Function`).
///
/// **Range contract (issue #1561):** every `ResolvedRef` pushed below
/// carries `range: uref.range` unchanged — never narrowed to a sub-segment
/// (receiver-only, method-only). By construction (`brink_ir::symbols::
/// project::Projector::walk_expr`'s `Expr::Call` arm) `uref.range` is
/// already the callee `Path`'s own whole span, so this function's only
/// obligation is to *not disturb it*. That whole-path range is the exact
/// `(FileId, TextRange)` lookup key `lir::lower::expr::lower_call` and
/// `ufcs_receiver_path`, `strict::check_void_root`, `coalesce`'s
/// operand classifier, `ufcs::value_receiver_def`, and
/// `infer::body::infer_call` all independently key on — see
/// [`brink_ir::ResolvedRef::range`]'s doc for the full consumer list and
/// the cross-layer regression test. A narrower range here (e.g. to support
/// a rename edit) is a silent miscompile for all four; narrowing for a
/// rename belongs at `brink-ide`'s own consumption layer instead
/// (`ufcs_hover`'s segment-narrowing helpers are the established pattern —
/// see #1550/#1554).
fn resolve_function(
    index: &SymbolIndex,
    scope: &ImportScope,
    locals: &[LocalSymbol],
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Try externals first
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::External]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        check_arity(index, file_id, uref, id, diagnostics);
        return;
    }

    // Try knots (ink allows knots as functions via tunnels). Issue #2298:
    // this is `lookup_knot_bare`, not the flat `lookup_by_name`/`classify`
    // every other lookup in this function uses — a knot-as-tunnel-function
    // call sits in the same divert-shaped addressing space `-> knot`
    // itself does (`lookup_divert`'s step 2), so a bare call `haggle()`
    // after only a module-qualified import (`use story::market::barter;`,
    // no symbol-level import of `haggle`) must be rejected exactly like
    // the bare divert `-> haggle` already is (issue #2287 bug (b)) —
    // before this fix, this step's flat lookup let `Candidacy::Imported`
    // from the qualified-module reading win regardless, reproducing bug
    // (b) for calls instead of diverts. The `External`/`List`/
    // `Variable`+`Constant` lookups below stay on the ordinary
    // `classify`-based rule deliberately: they are not divert-shaped, and
    // this module's top-of-file doc's "calls, struct literals, etc." carve-
    // out still holds for them — only a knot's own tunnel-call addressing
    // is special-cased here.
    if let Some(id) = lookup_knot_bare(index, scope, path) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        check_arity(index, file_id, uref, id, diagnostics);
        return;
    }

    // A real **call site** (`arg_count.is_some()`) naming a reserved builtin
    // (`is_builtin_function`'s classic uppercase intrinsics or
    // `is_t1b_stdlib_name`'s T1b verbs) must not let a `List`/`Variable`
    // symbol of that same name claim the call below — issue #2856 review:
    // `VAR MAX = 10` + `{MAX(1, 2)}` compiled clean and then died at
    // runtime with `RuntimeError::NotCallable("int")`, because the
    // unconditional `SymbolKind::Variable` lookup just below had no
    // call-site guard, unlike the list-item bare-name gate a few lines down
    // (`arg_count.is_none()`, issue #2830) that this mirrors. A `VAR`/`LIST`
    // is not itself callable the way a knot or a variable holding a divert
    // target is — routing a reserved-name call through to the real builtin
    // fallback below (`recognize_builtin`/`lower_t1b_stdlib_call`) is a
    // clean compile with well-defined behavior, exactly matching this same
    // name's pre-#2856 behavior, instead of a `CallVariable`/`ListFromInt`
    // emitted against a value that was never meant to be called. Read-site
    // shadowing (`resolve_variable`'s bare `{MAX}`) is untouched — this
    // guard only narrows the *callee* lookup at a call site.
    let reserved_call_site =
        uref.arg_count.is_some() && (is_builtin_function(path) || is_t1b_stdlib_name(path));

    // Try list names (ink allows `list(n)` as type conversion)
    if !reserved_call_site && let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::List])
    {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try locals (temps/params used as function names, e.g. `{storyletFunction(args)}`)
    // — checked BEFORE the global variable/constant lookup below, so a local
    // (param/temp/`let`) shadows a same-named global at a call site exactly
    // as it does at a bare-read site (`lookup_variable`'s step 1) and at a
    // dotted callee's head (the B3a arm below:
    // `lookup_local_in_scope(..).or_else(|| lookup_by_name(.., [Variable,
    // Constant]))`). Before #2083's fix this arm ran *after* the global
    // lookup, so a global could shadow a local at a call site while a bare
    // read of the same name resolved the local — one name calling one symbol
    // and reading another.
    //
    // ⚠ issue #2867 (open, unruled): unlike the `List` arm above and the
    // `Variable`/`Constant` arm below, this lookup is NOT covered by
    // `reserved_call_site`. Per
    // `push_local`'s own call sites (`brink_ir::symbols::project`),
    // `lookup_local_in_scope` can only ever return a `LocalSymbol` of kind
    // `SymbolKind::Param` (knot/stitch/function params) or
    // `SymbolKind::Temp` (`~ temp`, `for`-loop bindings, lambda params) —
    // that is the complete enumeration of kinds this arm can claim a call
    // site with. Neither kind is gated: both unconditionally claim this
    // arm today. `~ temp MAX = 10` + `{MAX(1, 2)}` under strict-ink compiles clean
    // (no `E035`, no `E183`) and then faults at runtime with
    // `RuntimeError::NotCallable("int")` — no diagnostic of any kind,
    // unlike the sibling `VAR`/`List` cases, which fall through to the
    // real builtin instead. Pinned (not yet fixed) by
    // `crates/brink-compiler/tests/issue_2856_builtin_shadow.rs`'s
    // `local_named_builtin_faults_at_runtime_not_callable` (temp) and
    // `param_named_builtin_faults_at_runtime_not_callable` (param). Fixing
    // this is a maintainer ruling (issue #2867's "Ask"): gate locals like
    // `VAR`/`List` so the call falls through to the real builtin, or
    // refuse with a compile diagnostic naming the local and its type —
    // deliberately not decided here.
    if let Some(id) = lookup_local_in_scope(locals, path, &uref.scope) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try variables and constants (ink allows calling a variable holding a
    // function ref; the native surface additionally allows a `const` to hold
    // one — #1862's bare-name form and #1774's lambda-literal decl default,
    // docs/t1c-spec.md §2a). `resolve_variable`'s own bare-read lookup
    // (`lookup_variable` above) searches `[Variable, Constant]` together,
    // with locals shadowing both (its step 1) — this call-site lookup now
    // mirrors both halves of that shape: the same kind list, in the same
    // locals-first order (see the arm above). It had been left
    // `Variable`-only, so a CONST-bound fn value's call site could never
    // resolve here — issue #2083's root cause. Root-caused to this one-line
    // gap in `brink-analyzer` itself (confirmed to reproduce identically via
    // a direct `brink_analyzer::resolve`/`analyze()` call, with no
    // `brink-db` involved at all — the issue's own suspicion that this was
    // an incremental-resolution bug in `brink-db`'s `resolve_query` was a
    // misdiagnosis: the brink-ir test cited as clean-path evidence never
    // actually asserted on `analyze()`'s resolution diagnostics, only on
    // the declaring global's own lowered default shape).
    if !reserved_call_site
        && let Some(id) = lookup_by_name(
            index,
            scope,
            path,
            &[SymbolKind::Variable, SymbolKind::Constant],
        )
    {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Bare list item name colliding with a stdlib verb (issue #2830): a
    // `RefKind::Function` reference whose path names a real declared list
    // item (e.g. `pop`, when some `LIST` declares an item literally called
    // `pop`) must resolve to that item — an author-declared symbol always
    // shadows a same-named stdlib builtin, exactly as `resolve_variable`'s
    // `lookup_variable` step 3 already does for `RefKind::Variable` refs.
    // Without this step, `is_t1b_stdlib_name` below silently claims the
    // name first (it has no way to know a real symbol exists) and the ref
    // is dropped with neither a resolution nor a diagnostic — the
    // `completeness` proptest counterexample this fixes.
    //
    // `arg_count.is_none()` restricts this to a `#fn(target)` literal site
    // (project_manifest's documented distinction — `#fn` binds a *prefix*
    // of the param row, so it never carries a call arity), the same
    // discriminator the UFCS branch below already uses. A real **call
    // site** (`arg_count.is_some()`, e.g. `push(arr, 5)`) must keep falling
    // through to `is_t1b_stdlib_name` and the t1b stdlib call lowering —
    // resolving it to the list item here would make a stdlib call compile
    // clean and then fault at runtime with `UnresolvedDefinition` when LIR
    // lowering looks it up as a stdlib verb instead of a list item.
    if uref.arg_count.is_none() {
        match lookup_list_item_bare(index, path) {
            BareItemResult::Unique(id) => {
                map.push(ResolvedRef {
                    file: file_id,
                    range: uref.range,
                    target: id,
                });
                return;
            }
            BareItemResult::Ambiguous => {
                diagnostics.push(ambiguous_diag(file_id, uref.range, path));
                return;
            }
            BareItemResult::NotFound => {}
        }
    }

    // T1b stdlib slice 1 (docs/t1b-surface-spec.md §5): `len`/`keys`/
    // `values`/`contains`/`push`/`insert`/`remove`/`remove_at` with no
    // matching user symbol are the brink-dialect builtins, handled at LIR
    // lowering —
    // same "skip resolution, no diagnostic here" treatment as
    // `is_builtin_function` below. Dialect-agnostic at this layer (an
    // author-defined symbol of the same name always wins regardless of
    // dialect, matched by the lookups above before this is reached);
    // `strict-ink` rejection of an unresolved use is a separate diagnostic
    // (`brink-analyzer::dialect_gate`, which — unlike this resolution pass —
    // does know the dialect).
    //
    // Issue #2856 point 3: `is_builtin_function` (the classic uppercase ink
    // intrinsics — `TURNS_SINCE`/`RANDOM`/…) is checked here too, alongside
    // `is_t1b_stdlib_name`, rather than unconditionally before every lookup
    // above as it was before this fix. `manifest.rs`'s `E035` ("name
    // shadows a built-in function") already documents both name sets as
    // author-shadowable with a warning, worded identically for each — but
    // only `is_t1b_stdlib_name` actually honored that here; the
    // `is_builtin_function` check ran first and unconditionally, so a
    // declared external/knot/list/variable/local of the same name was
    // never consulted at a *call* site either. Confirmed end-to-end via
    // `brink-cli`: without this fix a real silent drop occurred, not merely
    // a proptest-generator gap (the generator only ever emits lowercase
    // identifiers, so it could never reach `is_builtin_function`'s
    // all-uppercase names to begin with — see `arb_ident` in
    // `proptest_resolve.rs`).
    if is_t1b_stdlib_name(path) || is_builtin_function(path) {
        return;
    }

    // B3a UFCS-shaped callee (issue #1482, D1–D5 RULED 2026-07-26): every
    // static dotted-path interpretation above has failed, but the path's
    // *head* segment alone names a value in scope — this is method-call
    // syntax on that value (`g.greet(3)`), not an unresolved static path.
    // Exactly the TM-4b fallback `lookup_variable`'s step 11 already applies
    // to a dotted *value* reference (`p.x.y`), applied to the callee
    // position: the resolved target is the head value itself, and the
    // trailing segment is carried structurally by the HIR `Path`.
    //
    // Which of the two meanings that trailing segment has (a callable field
    // of the receiver's type, or a free function to desugar onto) is a
    // *type-directed* question this resolution pass cannot answer — so it
    // resolves the receiver and stays silent, and `brink-analyzer::ufcs`
    // owns the verdict and every diagnostic for the site (`E140`–`E143`).
    // Suppressing `E025` here is what keeps a legal method call
    // diagnostic-free and an illegal one from being reported twice.
    //
    // Inert for the ink corpus by construction: ink's own `FunctionCall`
    // lowering always builds a single-segment callee path, so no ink source
    // can reach this branch (see `ufcs`' module doc).
    //
    // `arg_count.is_some()` narrows this to a real **call site**. A
    // `RefKind::Function` reference is also recorded for a `#fn(target)`
    // literal's target, and that one always carries `arg_count: None`
    // (`project_manifest`'s own documented distinction — `#fn` binds a
    // *prefix* of the param row, so it has no call arity). A dotted `#fn`
    // target is not method-call syntax and has no UFCS verdict; it must
    // keep failing as an unresolved reference here rather than silently
    // resolving to its head value.
    if uref.arg_count.is_some()
        && let Some((head, _rest)) = path.split_once('.')
        && let Some(id) = lookup_local_in_scope(locals, head, &uref.scope).or_else(|| {
            lookup_by_name(
                index,
                scope,
                head,
                &[SymbolKind::Variable, SymbolKind::Constant],
            )
        })
    {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    diagnostics.push(unresolved_diag(
        index,
        scope,
        file_id,
        uref.range,
        path,
        DiagnosticCode::E025,
        &[SymbolKind::Knot],
    ));
}

/// Check that the number of arguments at the call site matches the target's parameter count.
fn check_arity(
    index: &SymbolIndex,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    target: DefinitionId,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let Some(call_arg_count) = uref.arg_count else {
        return;
    };
    let Some(info) = index.symbols.get(&target) else {
        return;
    };
    let expected = info.params.len();
    if call_arg_count != expected {
        diagnostics.push(Diagnostic {
            file: file_id,
            range: uref.range,
            message: format!(
                "{}: `{}` expects {} argument(s), got {}",
                DiagnosticCode::E031.title(),
                uref.path,
                expected,
                call_arg_count,
            ),
            code: DiagnosticCode::E031,
        });
    }
}

fn resolve_list_ref(
    index: &SymbolIndex,
    scope: &ImportScope,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;

    // Try qualified list item (ListName.ItemName)
    if path.contains('.')
        && let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::ListItem])
    {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    // Try bare list item name
    match lookup_list_item_bare(index, path) {
        BareItemResult::Unique(id) => {
            map.push(ResolvedRef {
                file: file_id,
                range: uref.range,
                target: id,
            });
            return;
        }
        BareItemResult::Ambiguous => {
            diagnostics.push(ambiguous_diag(file_id, uref.range, path));
            return;
        }
        BareItemResult::NotFound => {}
    }

    // Try list name
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::List]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }

    diagnostics.push(unresolved_diag(
        index,
        scope,
        file_id,
        uref.range,
        path,
        DiagnosticCode::E025,
        &[],
    ));
}

/// Resolve a struct construction literal's leading shape name (`Name#{…}`,
/// TM-4b, docs/typed-mode-spec.md §6) against declared `SymbolKind::Struct`
/// symbols. Always a bare (undotted) name — the construction-literal grammar
/// only ever puts a single identifier before `#{` — so this is a direct
/// by-name lookup, no hierarchical/qualified fallback chain like diverts or
/// variables need.
fn resolve_struct_ref(
    index: &SymbolIndex,
    scope: &ImportScope,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
    diagnostics: &mut Vec<Diagnostic>,
) {
    let path = &uref.path;
    if let Some(id) = lookup_by_name(index, scope, path, &[SymbolKind::Struct]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
        return;
    }
    diagnostics.push(unresolved_diag(
        index,
        scope,
        file_id,
        uref.range,
        path,
        DiagnosticCode::E068,
        &[],
    ));
}

/// Resolve a TM-2 type annotation's bare nominal leaf name (issue #2249) —
/// a struct field's declared type, or a `VAR`/`CONST`/`temp` annotation —
/// against declared `SymbolKind::Struct` symbols, exactly like
/// [`resolve_struct_ref`]'s lookup.
///
/// **Deliberately no diagnostic on a miss**, unlike `resolve_struct_ref`'s
/// `E068`: a `RefKind::Type` reference's `path` is not guaranteed to name a
/// struct at all — `int`, `float`, `List`, … are equally legal `Named`
/// leaves (`brink_ir::TypeExpr::Named`'s own doc), so "no declared struct
/// named this" is the overwhelmingly common, entirely legal case, not an
/// error. `annotations::check` (`E061`) is the dedicated "is this a
/// recognized type at all" diagnostic, run separately over the same HIR —
/// this function only ever *feeds* lowering's struct-shape chase
/// (`lir::lower::structs::record_global_annotation`,
/// `lir::lower::context::record_temp_annotation`) a resolved identity when
/// one exists, mirroring those two callers' own prior silent-`None`
/// posture (`ShapeTable::resolve` before this issue).
///
/// This *is* still narrower than `E061`'s own vocabulary check
/// (`annotations::declared_struct_names`): that lookup is project-flat,
/// with no referrer-scoping or std-exclusion at all, so a std-only struct
/// name is "recognized" for `E061`'s purposes regardless of importer —
/// unlike this function's `lookup_by_name`, which *does* exclude an
/// unimported std candidate. A `~ temp c: Cue` naming only a mounted std
/// module's `Cue`, with no project-side homonym or import, therefore still
/// raises no diagnostic anywhere today: `E061` accepts it (the name is
/// declared *somewhere*), and this resolution silently misses (by design,
/// per the paragraph above) rather than raising a new one. Issue #2249
/// leaves that specific compounding gap unruled — `annotations::TypeNames`
/// becoming referrer-scoped would be the natural fix, tracked there.
fn resolve_type_ref(
    index: &SymbolIndex,
    scope: &ImportScope,
    file_id: FileId,
    uref: &brink_ir::UnresolvedRef,
    map: &mut ResolutionMap,
) {
    if let Some(id) = lookup_by_name(index, scope, &uref.path, &[SymbolKind::Struct]) {
        map.push(ResolvedRef {
            file: file_id,
            range: uref.range,
            target: id,
        });
    }
}

// ─── Lookup helpers ─────────────────────────────────────────────────

/// Look up a local variable (param or temp) by bare name within the given
/// scope, among *this file's own* locals (issue #517 — locals never resolve
/// across files, since a knot's body lives in exactly one file).
///
/// A local matches if its name equals the bare name AND its scope is compatible:
/// same knot, and either same stitch or a knot-level param (stitch=None) which
/// is visible in all stitches. When multiple candidates match (e.g. a param and
/// a temp with the same name), picks the closest-preceding declaration.
fn lookup_local_in_scope(
    locals: &[LocalSymbol],
    bare_name: &str,
    scope: &Scope,
) -> Option<DefinitionId> {
    let mut best: Option<&LocalSymbol> = None;

    for local in locals {
        if local.name != bare_name {
            continue;
        }
        // Knot must match
        if local.scope.knot != scope.knot {
            continue;
        }
        // A knot-level local (stitch=None) is visible in all stitches.
        // A stitch-level local is only visible in that stitch.
        if local.scope.stitch.is_some() && local.scope.stitch != scope.stitch {
            continue;
        }
        // Pick closest-preceding by range start
        match best {
            Some(prev) if local.range.start() > prev.range.start() => {
                best = Some(local);
            }
            None => {
                best = Some(local);
            }
            _ => {}
        }
    }

    best.map(|local| local_definition_id(&local.scope, &local.name, local.kind))
}

/// Ink built-in functions that are resolved at LIR lowering, not by the
/// symbol index.
///
/// Delegates to `brink_ir::lir::is_builtin_function` (issue #2863) rather
/// than hand-keeping its own copy of the 22-name list: this crate already
/// depends on `brink-ir` (the edge only runs one way — `brink-ir` cannot
/// depend back on `brink-analyzer`), so `brink-ir` is where the canonical
/// list lives, alongside the `recognize_builtin` table it must agree with
/// by construction. Before this fix, the two crates hand-kept two
/// independent copies of this same list — the exact drift risk that PR
/// #2859's review flagged: the lists agreed on *content* (22 names each)
/// and a resolution-*order* bug still reached production, because content
/// equality alone doesn't prove the two call sites consult the list at the
/// same point in resolution. A single shared implementation removes the
/// "one copy edited, the other forgotten" half of that risk; the *order*
/// half is a separate, per-call-site invariant, pinned end-to-end by
/// `crates/brink-compiler/tests/issue_2856_builtin_shadow.rs` and by this
/// crate's own `crates/internal/brink-analyzer/tests/
/// reserved_names_cross_crate.rs`.
///
/// `pub` rather than `pub(crate)` (module `resolve` itself stays private) so
/// `lib.rs`'s `#[doc(hidden)] pub mod test_support` can re-export it —
/// issue #2856: the `completeness` proptest
/// (`tests/proptest_resolve.rs`) needs the REAL name set this function
/// checks, not a hand-duplicated copy.
pub fn is_builtin_function(name: &str) -> bool {
    brink_ir::lir::is_builtin_function(name)
}

/// T1b stdlib slice 1 function names (`docs/t1b-surface-spec.md` §5) plus
/// the TM-3-completion pure conversion intrinsics `int`/`float`/`string`
/// (`docs/typed-mode-spec.md` §4, maintainer ruling 2026-07-13, issue #659,
/// "per the stdlib slice-1 pattern"). Lowercase free functions,
/// brink-dialect-gated.
///
/// Delegates to `brink_ir::lir::is_t1b_stdlib_name` (issue #2863) for the
/// same reason as [`is_builtin_function`] just above — this used to be a
/// second hand-kept copy of `brink_ir`'s LIR-lowering list
/// (`lir::lower::expr::is_t1b_stdlib_name`); now there is exactly one
/// place this list is spelled out, and this function is a thin delegate to
/// it.
///
/// `resolve_function`'s lookup chain (externals, knots, lists, variables,
/// locals) always runs first, so an author-defined symbol of the same name
/// resolves normally — shadowing the builtin (§5's ruling) — and only a
/// resolution *failure* additionally checks this list before falling back
/// to the builtin (silently, no diagnostic) instead of emitting E025.
/// `is_builtin_function` now honors the identical ordering (issue #2856
/// point 3 — before that fix it was checked unconditionally, first, so a
/// declared symbol could never shadow one of *those* names; see that
/// function's own doc for the confirmed silent-drop this caused).
///
/// `pub` rather than `pub(crate)` for the same `test_support` re-export
/// reason as `is_builtin_function` above.
pub fn is_t1b_stdlib_name(name: &str) -> bool {
    brink_ir::lir::is_t1b_stdlib_name(name)
}

/// `pub(crate)`: reused by `signature.rs` (issue #712) to resolve a `#fn`
/// creation-site target to its declaring knot when computing a VAR/CONST
/// global's declaration-derived `fn(T…): R` type — the same "function knot
/// by bare name" lookup [`resolve_function`] itself does first, without
/// needing this pass's locals/scope machinery (a `#fn` target at global-
/// initializer position has no enclosing body to scope against).
///
/// Alias ruling (issue #1590): `use mod::name as alias;` / `IMPORT { name AS
/// alias } FROM mod` binds `alias` to `name`'s target *in addition to* `name`
/// itself — not instead of it. The alias never shadows or revokes the source
/// spelling's own bare visibility (still decided by [`classify`] against
/// `scope.bare_imports`, unchanged by this fallback). This is a deliberate
/// departure from Rust's `use … as` (which drops the original binding):
/// [`lookup_by_name`]'s "byte-identity guarantee" fast path already returns
/// a **globally unique** name unconditionally, ignoring `ImportScope`
/// entirely — so a strict revoke-on-alias rule would only ever bite in the
/// rarer ambiguous-candidate case, silently keeping the source name resolvable
/// everywhere else. Rather than ship a rule that only sometimes holds, `AS`
/// stays purely additive: predictable in every case, in both dialects.
///
/// **Precedence when an alias collides with an in-scope direct name**:
/// [`lookup_by_name_direct`] always runs first, and this function only
/// consults `scope.aliases` when that direct lookup comes up empty. So if
/// `IMPORT { haggle AS start } FROM quest` is written in a file that also
/// defines a knot named `start`, every bare reference to `start` resolves to
/// the *local* `start` knot — the direct match wins silently, and the alias
/// is unreachable under that name. Nothing currently diagnoses this
/// collision (`E089` only dedupes among import items; it never checks
/// against file-local definitions); a shadowing diagnostic is tracked as a
/// follow-up rather than blocking this fix.
pub(crate) fn lookup_by_name(
    index: &SymbolIndex,
    scope: &ImportScope,
    name: &str,
    kinds: &[SymbolKind],
) -> Option<DefinitionId> {
    if let Some(id) = lookup_by_name_direct(index, scope, name, kinds) {
        return Some(id);
    }

    // Alias fallback: `index.by_name` is keyed by definitions' own spellings
    // only, so a bare import's local alias — bound nowhere else — is never
    // found by the direct lookup above. Resolve it explicitly against the
    // specific `(module, source_name)` the import named; `kinds` and the
    // module still gate the match so an alias can never reach into the wrong
    // module or the wrong symbol kind.
    let (module, source_name) = scope.aliases.get(name)?;
    let ids = index.by_name.get(source_name.as_str())?;
    ids.iter().find_map(|id| {
        let info = index.symbols.get(id)?;
        (kinds.contains(&info.kind) && info.module.as_deref() == Some(module.as_str()))
            .then_some(*id)
    })
}

/// The direct (non-alias) name lookup — everything [`lookup_by_name`] did
/// before issue #1590's alias fallback, plus the 2026-08-03 SUBTRACTION
/// RULING's std-invisibility gate (issue #2197, doc below) — no longer
/// byte-identical to that description, but byte-identical for every corpus
/// that never coexists with a `std::…` candidate (the whole
/// pre-stdlib-mount world).
fn lookup_by_name_direct(
    index: &SymbolIndex,
    scope: &ImportScope,
    name: &str,
    kinds: &[SymbolKind],
) -> Option<DefinitionId> {
    let ids = index.by_name.get(name)?;

    let mut first_match: Option<DefinitionId> = None;
    let mut first_in_scope: Option<DefinitionId> = None;
    let mut first_imported: Option<DefinitionId> = None;
    let mut multiple = false;

    for id in ids {
        let Some(info) = index.symbols.get(id) else {
            continue;
        };
        if !kinds.contains(&info.kind) {
            continue;
        }
        let candidacy = classify(scope, info);
        // Issue #2197, per #2080's SCOPE FENCE (`docs/decision-log.md`,
        // "Stdlib mounts into `Environment`'s manifest at the producer, as
        // plain source"): the mount puts std source into every project's
        // manifest, but "nothing in it is marked `pub` and no confinement
        // rule scopes what a project's own `use` may reach into it" — a
        // real `use std::…` still needs #1582's `pub` marker and #2167's
        // confinement, neither built yet. Until then, stdlib symbols are
        // reachable only via that not-yet-existing explicit import — there
        // is no implicit inclusion, so `classify` can never answer
        // `Imported` for a std candidate today (nothing under `std`
        // can be marked public yet) — every std candidate this file does
        // not itself
        // belong to (i.e. not `InScope`, which still covers a std file
        // referencing its own std-declared siblings) is `Other`. Skip it
        // entirely here, *before* it is counted into `first_match`/
        // `multiple` below: an `Other`-classified std candidate must never
        // win the flat-fallback tie-break a few lines down, which would
        // otherwise let a project silently resolve into the mounted
        // preset with no import at all — including when it is the *sole*
        // match, where the `!multiple` fast path below would otherwise
        // return it unconditionally. This interacts with M-2d's own
        // coexistence machinery (`is_cross_declared_module_collision`,
        // which is what lets a std candidate coexist in `by_name` in the
        // first place) by narrowing exactly one of its three resolution
        // tiers — `Other` — for the std case only; `InScope` and
        // `Imported` are untouched, so a std file's own internal
        // references, and a future real `use std::…` import once #1582/
        // #2167 ship, keep resolving normally.
        //
        // #2251: this exclusion is not actually std-specific — it applies
        // to "any reserved-root candidate this file does not itself
        // belong to", so it now checks `is_reserved_root_module` against
        // the whole `RESERVED_ROOTS` set rather than the single `std`
        // literal `is_std_module` used to compare against. Behavior is
        // unchanged today (the set has exactly one member), and a future
        // second mounted library gets this same exclusion for free.
        if candidacy == Candidacy::Other
            && info.module.as_deref().is_some_and(is_reserved_root_module)
        {
            continue;
        }
        if first_match.is_none() {
            first_match = Some(*id);
        } else {
            multiple = true;
        }
        match candidacy {
            Candidacy::InScope if first_in_scope.is_none() => first_in_scope = Some(*id),
            Candidacy::Imported if first_imported.is_none() => first_imported = Some(*id),
            _ => {}
        }
    }

    // Fast path (byte-identity guarantee): with zero or one *non-std*
    // candidate of the requested kind — the entire strict-ink and
    // single-module world — the sole match is returned exactly as the
    // pre-M-2d flat lookup did, so the import scope never changes an
    // existing corpus's resolution. This is no longer quite "zero or one
    // candidate of the requested kind" (2026-08-03, issue #2197): a std
    // candidate that classifies `Other` is skipped above *before* it ever
    // reaches `first_match`/`multiple`, so a name with exactly one ordinary
    // candidate plus any number of coexisting std ones still takes this
    // fast path — and a name with std candidates *only* returns `None`
    // here, not the std candidate, unlike the byte-identical pre-#2197
    // description this comment used to give.
    if !multiple {
        return first_match;
    }

    // Multiple cross-module candidates (only reachable now that the #784/#793
    // stopgap is relaxed and same-name public defs coexist): the referrer's
    // own-module / legacy candidate wins, else an imported public one, else
    // fall back to the flat first-winner — which keeps `modules::check`'s
    // E025 import-required diagnostic (keyed off the resolved target) firing
    // for a genuinely un-imported cross-module reference, exactly as before.
    first_in_scope.or(first_imported).or(first_match)
}

/// The **scope-free** subset of [`lookup_by_name`]: the sole definition of
/// `name` whose kind is in `kinds`, or `None` when there is no such
/// definition **or more than one**.
///
/// This exists for callers that have no full [`ImportScope`] to hand — issue
/// #1909's UFCS-result typing runs inside `infer::body`, whose [`BodyCtx`]
/// (`brink-db`'s narrowed per-def HIR projection never holds a whole
/// `HirFile`) carries no file imports at all. `referrer_module` is the one
/// piece of that missing scope this function *can* still be handed — the
/// referring def's own declared module (issue #2233; `BodyCtx::
/// referrer_module`), the same value `ImportScope::file_module` would carry.
///
/// **It is a strict subset of what [`lookup_by_name`] answers, never a
/// second resolution rule** (issue #2216, unifying it with the
/// std-invisibility gate [`lookup_by_name_direct`] added for the scoped path
/// — 2026-08-03, issue #2197). This function still has no full
/// [`ImportScope`] to consult, so it cannot classify a candidate `Imported`
/// vs cross-module `Other` the way [`classify`] does — a candidate declared
/// in a mounted `std…` module is excluded here exactly as [`lookup_by_name`]
/// excludes it under the default (no-import) scope, **unless** it is
/// declared in the referrer's own module (`referrer_module`), which
/// reproduces exactly the one tier this function *can* fully classify
/// without an `ImportScope`: [`Candidacy::InScope`]'s "referrer and
/// candidate share a declared module" rule (issue #2233 — the fix for the
/// disagreement this doc used to call out; see below). This holds even when
/// the std candidate is the function's *sole* match: it is filtered out
/// before it can ever become `sole` whenever `referrer_module` doesn't match
/// it, so the name resolves as though that candidate did not exist, rather
/// than being returned.
///
/// For every other case, [`lookup_by_name_direct`]'s own "byte-identity
/// guarantee" fast path returns the sole non-std (or referrer-module-owned
/// std) candidate of the requested kinds *unconditionally, ignoring the rest
/// of the import scope*; the scope is consulted only once `multiple` is set.
/// So whenever this function returns `Some(id)`, [`lookup_by_name`] returns
/// the same `id` for any scope whose `file_module` equals this
/// `referrer_module` — pinned by `unique_lookup_agrees_with_scoped_lookup`
/// and, for the std case specifically, by
/// `unique_lookup_reproduces_in_scope_std_sibling_with_referrer_module`. When
/// it returns `None` on an ambiguous name, the caller must fall back to
/// whatever it did before rather than guess.
///
/// [`BodyCtx`]: crate::infer
pub(crate) fn lookup_unique_by_name(
    index: &SymbolIndex,
    name: &str,
    kinds: &[SymbolKind],
    referrer_module: Option<&str>,
) -> Option<DefinitionId> {
    let ids = index.by_name.get(name)?;
    let mut sole = None;
    for id in ids {
        let Some(info) = index.symbols.get(id) else {
            continue;
        };
        if !kinds.contains(&info.kind) {
            continue;
        }
        // Issue #2216, narrowed by #2233: this function has no full
        // `ImportScope`, so — mirroring `lookup_by_name_direct`'s
        // std-invisibility gate for the case where a candidate can never
        // classify `Imported` here — a std-mounted candidate must never win
        // the sole-match count, even alone, UNLESS it is declared in the
        // referrer's own module: that is exactly `Candidacy::InScope`'s
        // "referrer and candidate share a declared module" rule, the one
        // tier this function can reproduce without a full scope. A std
        // candidate in a *different* std module than the referrer's still
        // falls through to `Other` and is excluded, matching
        // `lookup_by_name`'s behavior for a referrer that has not imported
        // that sibling module.
        //
        // #2251: generalized from the single-root `is_std_module` to
        // `is_reserved_root_module` — same reasoning as
        // `lookup_by_name_direct`'s exclusion above.
        if info.module.as_deref().is_some_and(is_reserved_root_module)
            && info.module.as_deref() != referrer_module
        {
            continue;
        }
        if sole.is_some() {
            return None;
        }
        sole = Some(*id);
    }
    sole
}

/// Result of a bare list item lookup.
///
/// `pub(crate)` (issue #628): the phase-0 `Sig` stub's list-literal type
/// inference (`external_check::resolve_list_item_name`) reuses this exact
/// bare-name resolution — the declaring LIST is always a project-global
/// lookup, never locally scoped, so that stub can call straight in without
/// threading an `ImportScope` through `signature()`.
pub(crate) enum BareItemResult {
    /// Exactly one match.
    Unique(DefinitionId),
    /// Multiple matches across different lists — caller must qualify.
    Ambiguous,
    /// No match found.
    NotFound,
}

/// Look up a list item by its bare (unqualified) name.
/// Searches all `ListName.ItemName` entries for a suffix match.
/// Returns `Ambiguous` if multiple lists contain an item with this name.
pub(crate) fn lookup_list_item_bare(index: &SymbolIndex, bare_name: &str) -> BareItemResult {
    let suffix = format!(".{bare_name}");
    let mut found: Option<DefinitionId> = None;
    for (name, ids) in &index.by_name {
        if name.ends_with(&suffix) {
            for id in ids {
                if let Some(info) = index.symbols.get(id)
                    && info.kind == SymbolKind::ListItem
                {
                    if found.is_some() {
                        return BareItemResult::Ambiguous;
                    }
                    found = Some(*id);
                }
            }
        }
    }
    match found {
        Some(id) => BareItemResult::Unique(id),
        None => BareItemResult::NotFound,
    }
}

/// Look up a label within a knot scope. Searches for `knot.label` and
/// `knot.*.label` patterns.
fn lookup_label_in_knot(
    index: &SymbolIndex,
    scope: &ImportScope,
    knot: &str,
    label: &str,
) -> Option<DefinitionId> {
    // Try knot.label
    let direct = format!("{knot}.{label}");
    if let Some(id) = lookup_by_name(index, scope, &direct, &[SymbolKind::Label]) {
        return Some(id);
    }

    // Try knot.*.label (any stitch within this knot).
    // Collect all matches and pick the smallest `DefinitionId` for determinism,
    // since `HashMap` iteration order is not stable across processes.
    let suffix = format!(".{label}");
    let prefix = format!("{knot}.");
    let mut best: Option<DefinitionId> = None;
    for (name, ids) in &index.by_name {
        if name.starts_with(&prefix) && name.ends_with(&suffix) && name.matches('.').count() == 2 {
            for id in ids {
                if let Some(info) = index.symbols.get(id)
                    && info.kind == SymbolKind::Label
                {
                    best = Some(match best {
                        Some(prev) if prev.to_raw() <= id.to_raw() => prev,
                        _ => *id,
                    });
                }
            }
        }
    }
    best
}

fn ambiguous_diag(file: FileId, range: rowan::TextRange, path: &str) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: format!(
            "{}: `{path}` — qualify with the list name (e.g., `ListName.{path}`)",
            DiagnosticCode::E027.title(),
        ),
        code: DiagnosticCode::E027,
    }
}

/// True when `index` holds at least one declared symbol named `path`
/// whose module is a reserved peer root ([`is_reserved_root_module`]) — regardless
/// of `SymbolKind`, since the point here is not "which lookup would have
/// matched" but "does bare-name resolution's std-invisibility gate
/// (`lookup_by_name`, `lookup_unique_by_name`) explain why this path came
/// back empty".
///
/// Issue #2217: the gate that excludes std candidates from bare-name
/// resolution (`lookup_by_name`'s `Candidacy::Other` skip,
/// `lookup_unique_by_name`'s unconditional skip) applies identically to
/// the *embedded* stdlib mount and to a project's own file that legitimately
/// lives at a `std/…` path (`brink_environment::mount_stdlib`'s "project
/// source at the same key wins" carve-out — both mint the identical
/// `std::…` module identity via `native_module_path`, by design; see
/// `docs/modules-spec.md` §4 "peer roots"). Reconsidering `is_std_module`
/// to distinguish the two would undo that ruling, so this does not attempt
/// it — it only turns the resulting unexplained E024/E025/E068 into a
/// diagnosable one, without inventing a new diagnostic code.
fn is_std_shadowed_name(index: &SymbolIndex, path: &str) -> bool {
    index.by_name.get(path).is_some_and(|ids| {
        ids.iter().any(|id| {
            index
                .symbols
                .get(id)
                .is_some_and(|info| info.module.as_deref().is_some_and(is_reserved_root_module))
        })
    })
}

/// Look for a candidate named `path`, of one of `kinds`, that resolution
/// skipped specifically because it is reachable only via a **qualified**
/// module import ([`is_qualified_import_only`]) — the "module-imported-
/// but-bare" row of the four-row resolution table issues #2287/#2296
/// worked through for diverts and #2298 extended to knot-as-tunnel-
/// function calls and `lookup_divert`'s remaining steps. When one exists,
/// [`unresolved_diag`] threads its module into the message so this row
/// gets the same "import it from `module`" framing `modules::check`'s own
/// E025 already gives the "no import at all" row (issue #2298 item 3),
/// instead of a bare, unexplained unresolved-reference message.
///
/// `kinds` must be exactly the symbol kinds the caller's own lookup chain
/// actually applied the `is_qualified_import_only` exclusion to — passing
/// a kind the caller never excluded (e.g. `External` at a function-call
/// site, which stays on the ordinary `classify` rule per this module's
/// top-of-file doc) would name a candidate that was never "skipped" at
/// all, misattributing an unrelated resolution failure. An empty slice
/// (every call site this exclusion does not apply to) always returns
/// `None`, leaving the message exactly as it was before this hint existed.
fn qualified_import_only_hint(
    index: &SymbolIndex,
    scope: &ImportScope,
    path: &str,
    kinds: &[SymbolKind],
) -> Option<String> {
    let ids = index.by_name.get(path)?;
    ids.iter().find_map(|id| {
        let info = index.symbols.get(id)?;
        if !kinds.contains(&info.kind) || !is_qualified_import_only(scope, info) {
            return None;
        }
        info.module.clone()
    })
}

fn unresolved_diag(
    index: &SymbolIndex,
    scope: &ImportScope,
    file: FileId,
    range: rowan::TextRange,
    path: &str,
    code: DiagnosticCode,
    qualified_only_kinds: &[SymbolKind],
) -> Diagnostic {
    let message = if is_std_shadowed_name(index, path) {
        format!(
            "{}: `{path}` — a declaration of this name exists under the `std::` peer root \
             (either the mounted stdlib, or your own project file at a `std/…` path); bare \
             names under `std::` are invisible outside it by rule, not by mistake — reference \
             it with `use std::…` (docs/modules-spec.md §4)",
            code.title(),
        )
    } else if let Some(module) =
        qualified_import_only_hint(index, scope, path, qualified_only_kinds)
    {
        format!(
            "{}: `{path}` — a qualified-module import makes `{module}::{path}` reachable, but \
             not the bare `{path}`; import it from `{module}` (see modules-spec §2)",
            code.title(),
        )
    } else {
        format!("{}: `{path}`", code.title())
    };
    Diagnostic {
        file,
        range,
        message,
        code,
    }
}

#[cfg(test)]
#[expect(clippy::cast_possible_truncation, reason = "test helper ranges")]
mod tests {
    use brink_ir::{DeclaredSymbol, ImportItem, Scope, UnresolvedRef};
    use rowan::TextRange;
    use rowan::TextSize;

    use super::*;
    use crate::manifest::merge_manifests;

    fn range(offset: u32, len: u32) -> TextRange {
        TextRange::new(TextSize::new(offset), TextSize::new(offset + len))
    }

    fn make_manifest(
        knots: &[&str],
        stitches: &[&str],
        variables: &[&str],
        lists: &[(&str, &[&str])],
        externals: &[&str],
        labels: &[&str],
        unresolved: Vec<UnresolvedRef>,
    ) -> SymbolManifest {
        let mut manifest = SymbolManifest::default();
        let mut offset = 0u32;

        for &name in knots {
            let r = range(offset, name.len() as u32);
            manifest.knots.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
                visibility: None,
                was: None,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in stitches {
            let r = range(offset, name.len() as u32);
            manifest.stitches.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
                visibility: None,
                was: None,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in variables {
            let r = range(offset, name.len() as u32);
            manifest.variables.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
                visibility: None,
                was: None,
            });
            offset += name.len() as u32 + 1;
        }
        for &(list_name, items) in lists {
            let r = range(offset, list_name.len() as u32);
            manifest.lists.push(DeclaredSymbol {
                name: list_name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
                visibility: None,
                was: None,
            });
            offset += list_name.len() as u32 + 1;
            for &item in items {
                let qualified = format!("{list_name}.{item}");
                let r = range(offset, item.len() as u32);
                manifest.list_items.push(DeclaredSymbol {
                    name: qualified,
                    range: r,
                    params: Vec::new(),
                    detail: None,
                    visibility: None,
                    was: None,
                });
                offset += item.len() as u32 + 1;
            }
        }
        for &name in externals {
            let r = range(offset, name.len() as u32);
            manifest.externals.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
                visibility: None,
                was: None,
            });
            offset += name.len() as u32 + 1;
        }
        for &name in labels {
            let r = range(offset, name.len() as u32);
            manifest.labels.push(DeclaredSymbol {
                name: name.to_string(),
                range: r,
                params: Vec::new(),
                detail: None,
                visibility: None,
                was: None,
            });
            offset += name.len() as u32 + 1;
        }
        manifest.unresolved = unresolved;
        manifest
    }

    fn uref(path: &str, kind: RefKind, knot: Option<&str>, stitch: Option<&str>) -> UnresolvedRef {
        uref_with_args(path, kind, knot, stitch, None)
    }

    fn uref_with_args(
        path: &str,
        kind: RefKind,
        knot: Option<&str>,
        stitch: Option<&str>,
        arg_count: Option<usize>,
    ) -> UnresolvedRef {
        UnresolvedRef {
            path: path.to_string(),
            range: range(900, path.len() as u32),
            kind,
            scope: Scope {
                knot: knot.map(String::from),
                stitch: stitch.map(String::from),
            },
            arg_count,
            module_qualified: false,
        }
    }

    #[test]
    fn single_knot_divert_resolves() {
        let manifest = make_manifest(
            &["start"],
            &[],
            &[],
            &[],
            &[],
            &[],
            vec![uref("start", RefKind::Divert, None, None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, merge_diags) = merge_manifests(&files);
        let (resolutions, resolve_diags) = resolve_refs(&index, &files);

        assert!(merge_diags.is_empty());
        assert!(resolve_diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        assert_eq!(resolutions[0].file, FileId(0));
    }

    #[test]
    fn qualified_knot_stitch_divert_resolves() {
        let manifest = make_manifest(
            &["kitchen"],
            &["kitchen.look_around"],
            &[],
            &[],
            &[],
            &[],
            vec![uref("kitchen.look_around", RefKind::Divert, None, None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
    }

    #[test]
    fn stitch_local_divert_prefers_local_stitch() {
        let manifest = make_manifest(
            &["bedroom", "kitchen"],
            &["bedroom.look", "kitchen.look"],
            &[],
            &[],
            &[],
            &[],
            vec![uref("look", RefKind::Divert, Some("bedroom"), None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        // The resolved ID should be for bedroom.look
        let info = index.symbols.get(&resolutions[0].target).unwrap();
        assert_eq!(info.name, "bedroom.look");
    }

    #[test]
    fn unresolved_divert_emits_diagnostic() {
        let manifest = make_manifest(
            &["start"],
            &[],
            &[],
            &[],
            &[],
            &[],
            vec![uref("nonexistent", RefKind::Divert, None, None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(resolutions.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E024);
    }

    #[test]
    fn duplicate_knot_emits_warning() {
        let mut m1 = make_manifest(&["start"], &[], &[], &[], &[], &[], vec![]);
        let m2 = make_manifest(&["start"], &[], &[], &[], &[], &[], vec![]);

        // Give m1 different range so they don't collide
        m1.knots[0].range = range(0, 5);

        let files = vec![(FileId(0), &m1), (FileId(1), &m2)];
        let (_index, diags) = merge_manifests(&files);

        // Inklecate permits duplicate definitions — we warn but don't error.
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E022);
    }

    #[test]
    fn cross_file_duplicate_knot_local_does_not_leak_across_files() {
        // #517 (finding-4 fix): file B has a duplicate knot name (`dup`,
        // warned E022) but does *not* declare its own `t`. Before the
        // locals split, `lookup_local_in_scope` searched the merged index
        // and would (wrongly) resolve B's reference against A's
        // same-scoped `t`, since the lookup never checked which file a
        // candidate came from. After the split, resolution reads only the
        // referencing file's own `manifest.locals`, so B's reference to an
        // undeclared `t` must fail to resolve instead of silently aliasing
        // A's declaration.
        let mut a = SymbolManifest::default();
        a.knots.push(brink_ir::DeclaredSymbol {
            name: "dup".to_string(),
            range: range(0, 3),
            params: Vec::new(),
            detail: None,
            visibility: None,
            was: None,
        });
        a.locals.push(LocalSymbol {
            name: "t".to_string(),
            range: range(10, 1),
            scope: Scope {
                knot: Some("dup".to_string()),
                stitch: None,
            },
            kind: SymbolKind::Temp,
            param_detail: None,
            annotation: None,
        });

        let mut b = SymbolManifest::default();
        b.knots.push(brink_ir::DeclaredSymbol {
            name: "dup".to_string(),
            range: range(100, 3),
            params: Vec::new(),
            detail: None,
            visibility: None,
            was: None,
        }); // duplicate name -> E022, not indexed
        b.unresolved
            .push(uref("t", RefKind::Variable, Some("dup"), None));

        let files = vec![(FileId(0), &a), (FileId(1), &b)];
        let (index, merge_diags) = merge_manifests(&files);
        assert_eq!(merge_diags.len(), 1, "duplicate knot should warn once");
        assert_eq!(merge_diags[0].code, DiagnosticCode::E022);

        let (resolutions, diags) = resolve_refs(&index, &files);
        assert!(
            resolutions.is_empty(),
            "B's reference to an undeclared local must not resolve, got {resolutions:?}"
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E025);
    }

    #[test]
    fn list_item_bare_name_resolves() {
        let manifest = make_manifest(
            &[],
            &[],
            &[],
            &[("Colors", &["red", "green", "blue"])],
            &[],
            &[],
            vec![uref("red", RefKind::Variable, None, None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let info = index.symbols.get(&resolutions[0].target).unwrap();
        assert_eq!(info.name, "Colors.red");
    }

    #[test]
    fn end_done_not_in_unresolved() {
        // END/DONE are handled as DivertPath::End/Done at the HIR level,
        // so they never appear as UnresolvedRef entries. This test verifies
        // that the resolution pass doesn't get confused by them.
        let manifest = make_manifest(
            &["start"],
            &[],
            &[],
            &[],
            &[],
            &[],
            vec![], // No unresolved refs for END/DONE
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert!(resolutions.is_empty());
    }

    #[test]
    fn label_in_knot_resolves() {
        let manifest = make_manifest(
            &["meeting"],
            &[],
            &[],
            &[],
            &[],
            &["meeting.greet"],
            vec![uref("greet", RefKind::Divert, Some("meeting"), None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let info = index.symbols.get(&resolutions[0].target).unwrap();
        assert_eq!(info.name, "meeting.greet");
    }

    #[test]
    fn external_function_resolves() {
        let manifest = make_manifest(
            &[],
            &[],
            &[],
            &[],
            &["print_debug"],
            &[],
            vec![uref("print_debug", RefKind::Function, None, None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
    }

    #[test]
    fn global_variable_resolves() {
        let manifest = make_manifest(
            &[],
            &[],
            &["player_name"],
            &[],
            &[],
            &[],
            vec![uref("player_name", RefKind::Variable, None, None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
    }

    #[test]
    fn ambiguous_bare_list_item_emits_diagnostic() {
        let manifest = make_manifest(
            &[],
            &[],
            &[],
            &[("Fruit", &["red"]), ("Color", &["red"])],
            &[],
            &[],
            vec![uref("red", RefKind::Variable, None, None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(resolutions.is_empty());
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E027);
    }

    #[test]
    fn qualified_list_item_resolves_despite_ambiguity() {
        let manifest = make_manifest(
            &[],
            &[],
            &[],
            &[("Fruit", &["red"]), ("Color", &["red"])],
            &[],
            &[],
            vec![uref("Color.red", RefKind::Variable, None, None)],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert!(diags.is_empty());
        assert_eq!(resolutions.len(), 1);
        let info = index.symbols.get(&resolutions[0].target).unwrap();
        assert_eq!(info.name, "Color.red");
    }

    // ── Arity checking ──────────────────────────────────────────────

    /// Make a manifest with a knot that has a specific number of params.
    fn make_manifest_with_params(
        knot_name: &str,
        param_count: usize,
        unresolved: Vec<UnresolvedRef>,
    ) -> SymbolManifest {
        let mut manifest = SymbolManifest::default();
        let r = range(0, knot_name.len() as u32);
        let params: Vec<brink_ir::ParamInfo> = (0..param_count)
            .map(|i| brink_ir::ParamInfo {
                name: format!("p{i}"),
                is_ref: false,
                is_divert: false,
            })
            .collect();
        manifest.knots.push(DeclaredSymbol {
            name: knot_name.to_string(),
            range: r,
            params,
            detail: Some("function".to_string()),
            visibility: None,
            was: None,
        });
        manifest.unresolved = unresolved;
        manifest
    }

    #[test]
    fn arity_match_no_warning() {
        // Call `greet(x)` where greet takes 1 param — no warning.
        let manifest = make_manifest_with_params(
            "greet",
            1,
            vec![uref_with_args(
                "greet",
                RefKind::Function,
                None,
                None,
                Some(1),
            )],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(resolutions.len(), 1);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for matching arity, got: {diags:?}"
        );
    }

    #[test]
    fn arity_mismatch_emits_e031() {
        // Call `greet(x, y)` where greet takes 1 param — E031 warning.
        let manifest = make_manifest_with_params(
            "greet",
            1,
            vec![uref_with_args(
                "greet",
                RefKind::Function,
                None,
                None,
                Some(2),
            )],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(
            resolutions.len(),
            1,
            "should still resolve despite arity mismatch"
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E031);
        assert!(diags[0].message.contains("expects 1"));
        assert!(diags[0].message.contains("got 2"));
    }

    #[test]
    fn arity_check_no_arg_count_no_warning() {
        // A ref with `arg_count: None` (not a call site at all) should
        // never trigger arity checking, regardless of kind. This uses
        // `uref`'s hardcoded `None` directly rather than a real divert
        // pipeline — since issue #2156 a *real* `RefKind::Divert` ref
        // always carries `Some(target.args.len())`
        // (`brink_ir::symbols::project`'s `walk_divert_target`); see
        // `divert_arity_mismatch_emits_e176` below for that path.
        let manifest =
            make_manifest_with_params("greet", 1, vec![uref("greet", RefKind::Divert, None, None)]);
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (_resolutions, diags) = resolve_refs(&index, &files);

        assert!(
            diags.is_empty(),
            "a ref with no arg_count should not trigger arity check: {diags:?}"
        );
    }

    /// Make a manifest with a top-level knot that has a specific number of
    /// params, alongside a top-level `Variable` — for the divert-arity
    /// (`E176`, issue #2156) test family below.
    fn make_manifest_with_knot_and_variable(
        knot_name: &str,
        param_count: usize,
        variable_name: &str,
        unresolved: Vec<UnresolvedRef>,
    ) -> SymbolManifest {
        let mut manifest = make_manifest_with_params(knot_name, param_count, Vec::new());
        let r = range(9000, variable_name.len() as u32);
        manifest.variables.push(DeclaredSymbol {
            name: variable_name.to_string(),
            range: r,
            params: Vec::new(),
            detail: None,
            visibility: None,
            was: None,
        });
        manifest.unresolved = unresolved;
        manifest
    }

    #[test]
    fn divert_arity_match_emits_no_e176() {
        // `-> greet(x)` where `greet` takes 1 param — no warning.
        let manifest = make_manifest_with_params(
            "greet",
            1,
            vec![uref_with_args(
                "greet",
                RefKind::Divert,
                None,
                None,
                Some(1),
            )],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(resolutions.len(), 1);
        assert!(
            diags.is_empty(),
            "expected no diagnostics for matching divert arity, got: {diags:?}"
        );
    }

    #[test]
    fn divert_arity_mismatch_emits_e176() {
        // `-> greet(x, y)` where `greet` takes 1 param — E176 warning, not
        // E031 (that code stays scoped to ordinary calls; this is its
        // sibling for the divert shape).
        let manifest = make_manifest_with_params(
            "greet",
            1,
            vec![uref_with_args(
                "greet",
                RefKind::Divert,
                None,
                None,
                Some(2),
            )],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(
            resolutions.len(),
            1,
            "should still resolve despite arity mismatch"
        );
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E176);
        assert!(diags[0].message.contains("expects 1"));
        assert!(diags[0].message.contains("got 2"));
    }

    #[test]
    fn divert_through_variable_is_not_arity_checked() {
        // `-> holder` where `holder` is a `Variable` (holding a stored
        // divert-target value, e.g. ink's "Advanced: sending divert
        // targets as parameters") must never be arity-checked: a
        // `Variable` symbol carries no declared parameter row of its own,
        // so checking `arg_count` against it would misfire on legitimate
        // code. `lookup_divert`'s case 6 (bare-name fallback to a
        // `Variable`) is exactly the resolution this exercises — the knot
        // `greet` (1 param) is a decoy that must NOT be what `holder`
        // resolves to.
        let manifest = make_manifest_with_knot_and_variable(
            "greet",
            1,
            "holder",
            vec![uref_with_args(
                "holder",
                RefKind::Divert,
                None,
                None,
                Some(3),
            )],
        );
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(resolutions.len(), 1, "the Variable must still resolve");
        assert!(
            diags.is_empty(),
            "a divert through a Variable resolution must never be arity-checked: {diags:?}"
        );
    }

    #[test]
    fn arity_mismatch_external() {
        // Call external with wrong arity.
        let mut manifest = SymbolManifest::default();
        let r = range(0, 5);
        manifest.externals.push(DeclaredSymbol {
            name: "print".to_string(),
            range: r,
            params: vec![brink_ir::ParamInfo {
                name: "msg".into(),
                is_ref: false,
                is_divert: false,
            }],
            detail: None,
            visibility: None,
            was: None,
        });
        manifest.unresolved.push(uref_with_args(
            "print",
            RefKind::Function,
            None,
            None,
            Some(3),
        ));
        let files = vec![(FileId(0), &manifest)];
        let (index, _) = merge_manifests(&files);
        let (resolutions, diags) = resolve_refs(&index, &files);

        assert_eq!(resolutions.len(), 1);
        assert_eq!(diags.len(), 1);
        assert_eq!(diags[0].code, DiagnosticCode::E031);
    }

    // ── TM-4b resolution fallback (docs/typed-mode-spec.md §6) ─────────
    //
    // End-to-end (parse -> HIR lower -> merge_manifests -> resolve_file)
    // rather than the hand-built `make_manifest` fixtures above: the
    // precedence claim is about how a *real* dotted `Path` — produced by
    // the actual parser/lowering pipeline, not a synthesized
    // `UnresolvedRef` — resolves, so the fixture needs the real pipeline to
    // be a faithful test of the claim.

    /// Parse -> HIR lower -> `merge_manifests` -> `resolve_file` (the real
    /// production pipeline). Each fixture below is written to produce
    /// exactly one resolvable reference (the LHS of `~ y = …` is an
    /// undeclared `y`, so it stays unresolved — diagnosed, not resolved —
    /// and `-> DONE` is handled specially at the HIR level, never a
    /// resolvable reference), so `resolutions` always has exactly one entry:
    /// the dotted/bare reference under test.
    fn build_real(src: &str) -> (SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (_hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let (index, _diag) = merge_manifests(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            resolve_file(&index, &ImportScope::default(), FileId(0), &manifest);
        (index, resolutions)
    }

    /// The `SymbolKind` a reference spanning exactly `needle` (the first
    /// occurrence of that substring in `src`) resolved to. Ink's own
    /// lowering can register additional bookkeeping references beyond the
    /// one under test (e.g. an implicit fallthrough divert when a knot's
    /// only content is its first stitch) — matching by the reference's
    /// exact source span, rather than assuming `resolutions` has exactly
    /// one entry, is robust to that noise.
    fn resolved_kind_at(
        src: &str,
        index: &SymbolIndex,
        resolutions: &ResolutionMap,
        needle: &str,
    ) -> SymbolKind {
        // The *last* occurrence — every fixture below places the reference
        // under test after any same-named declaration.
        let start = src
            .rfind(needle)
            .expect("needle not found in fixture source");
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test fixture offsets fit in u32"
        )]
        let range = rowan::TextRange::new(
            rowan::TextSize::from(start as u32),
            rowan::TextSize::from((start + needle.len()) as u32),
        );
        let target = resolutions
            .iter()
            .find(|r| r.range == range)
            .expect("no resolution spanning the needle's exact range")
            .target;
        index
            .symbols
            .get(&target)
            .expect("resolved target missing from index")
            .kind
    }

    #[test]
    fn resolution_fallback_static_dotted_path_wins_over_a_colliding_variable_name() {
        // §6's own precedence claim: "ink's static dotted paths (knot.stitch,
        // List.Item) are resolved first and win". `knot` here is BOTH the
        // head of a real stitch path (`knot.stitch`) AND the name of a
        // declared variable — the static path must win, resolving `knot.x`
        // to the stitch, not falling back to field access on the variable.
        let src = "VAR knot = 0\n=== knot ===\n= x\nHello.\n-> DONE\n\
                   === main ===\n~ y = knot.x\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "knot.x"),
            SymbolKind::Stitch,
            "the static `knot.x` stitch path must win over the colliding `knot` variable"
        );
    }

    #[test]
    fn resolution_fallback_resolves_to_head_variable_when_no_static_path_matches() {
        // No knot/stitch/list/label named `p` or `p.x` exists — the fallback
        // resolves `p.x` to the variable `p` itself (field access on it),
        // not an unresolved reference.
        let src = "VAR p = 0\n=== main ===\n~ y = p.x\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "p.x"),
            SymbolKind::Variable
        );
    }

    #[test]
    fn resolution_fallback_resolves_to_head_param() {
        // Same fallback, but the head is a knot parameter rather than a
        // global — the fallback checks locals (params/temps) first, per
        // `lookup_variable`'s existing local-shadows-global ordering.
        let src = "=== main(p) ===\n~ y = p.x\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "p.x"),
            SymbolKind::Param
        );
    }

    #[test]
    fn resolution_fallback_does_not_apply_to_a_single_segment_path() {
        // A bare `p` (no dot at all) must resolve exactly as it always has
        // — the fallback is gated on `path.contains('.')` and must never
        // fire for an ordinary single-segment variable reference.
        let src = "VAR p = 0\n=== main ===\n~ y = p\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "p"),
            SymbolKind::Variable
        );
    }

    #[test]
    fn struct_literal_resolves_shape_name_to_the_declared_struct() {
        let src = "STRUCT Point = #{x: float}\n=== main ===\n~ p = Point#{x: 1.0}\n-> DONE\n";
        let (index, resolutions) = build_real(src);
        assert_eq!(
            resolved_kind_at(src, &index, &resolutions, "Point"),
            SymbolKind::Struct
        );
    }

    #[test]
    fn struct_literal_unresolved_shape_name_is_e068() {
        let src = "=== main ===\n~ p = Bogus#{x: 1}\n-> DONE\n";
        let parsed = brink_syntax::parse(src);
        let (_hir, manifest, _diag) = brink_ir::hir::lower(FileId(0), &parsed.tree());
        let (index, _diag) = merge_manifests(&[(FileId(0), &manifest)]);
        let (resolutions, diags) =
            resolve_file(&index, &ImportScope::default(), FileId(0), &manifest);
        assert!(
            resolutions.is_empty(),
            "no resolution for an undeclared shape: {resolutions:?}"
        );
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E068),
            "{diags:?}"
        );
    }

    // ── M-2d import-scoped lookup (issue #790) ────────────────────────

    /// A hand-built index with two `Knot`s named `ambush` in *different*
    /// declared modules (the coexistence unlocked by relaxing the #784/#793
    /// stopgap). Insertion order is `quest_a`, then `quest_b`, so the flat
    /// first-winner is always `quest_a` — the import scope is what makes
    /// `quest_b` reachable.
    fn two_module_ambush_index() -> (SymbolIndex, DefinitionId, DefinitionId) {
        use brink_format::DefinitionTag;
        let mut index = SymbolIndex::default();
        let mk = |index: &mut SymbolIndex, module: &str, hash: u64| {
            let id = DefinitionId::new(DefinitionTag::Address, hash);
            index.symbols.insert(
                id,
                SymbolInfo {
                    kind: SymbolKind::Knot,
                    file: FileId(0),
                    range: TextRange::default(),
                    id,
                    name: "ambush".to_string(),
                    params: Vec::new(),
                    detail: None,
                    scope: None,
                    param_detail: None,
                    module: Some(module.to_string()),
                    visibility: Visibility::Public,
                },
            );
            index
                .by_name
                .entry("ambush".to_string())
                .or_default()
                .push(id);
            id
        };
        let a = mk(&mut index, "quest_a", 0xA);
        let b = mk(&mut index, "quest_b", 0xB);
        (index, a, b)
    }

    #[test]
    fn import_scope_binds_each_importer_to_its_own_module() {
        let (index, a, b) = two_module_ambush_index();

        let scope_a = ImportScope {
            file_module: None,
            qualified_modules: ["quest_a".to_string()].into_iter().collect(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &scope_a, "ambush", &[SymbolKind::Knot]),
            Some(a),
            "a file importing quest_a binds quest_a's ambush"
        );

        let scope_b = ImportScope {
            file_module: None,
            qualified_modules: ["quest_b".to_string()].into_iter().collect(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &scope_b, "ambush", &[SymbolKind::Knot]),
            Some(b),
            "a file importing quest_b binds quest_b's ambush — not the flat first-winner"
        );
    }

    /// Issue #1909's fence: [`lookup_unique_by_name`] must only ever answer
    /// where [`lookup_by_name`] would answer identically **for every scope
    /// whose `file_module` equals the `referrer_module` hint this function
    /// was handed** (issue #2233 narrowed the guarantee from "every scope
    /// unconditionally" to this — a std-mounted candidate's visibility now
    /// depends on that hint agreeing with the scope's own `file_module`; see
    /// `unique_lookup_reproduces_in_scope_std_sibling_with_referrer_module`
    /// below for the case that distinguishes them). Both halves are
    /// asserted — the sole-candidate name agrees with two deliberately
    /// opposed scopes, and the ambiguous name declines.
    ///
    /// This fixture has no std-mounted candidate; the std-mounted case
    /// (issue #2216, the #2197 follow-up this doc used to flag as an
    /// un-pinned gap) is covered separately by
    /// `unique_lookup_excludes_std_mounted_sole_candidate`,
    /// `unique_lookup_skips_std_candidate_and_returns_the_ordinary_one`, and
    /// `unique_lookup_reproduces_in_scope_std_sibling_with_referrer_module`
    /// below, now that [`lookup_unique_by_name`] applies the same
    /// std-invisibility gate as [`lookup_by_name_direct`], narrowed by a
    /// `referrer_module` hint (issue #2233).
    #[test]
    fn unique_lookup_agrees_with_scoped_lookup() {
        let (index, a, b) = two_module_ambush_index();
        assert_eq!(
            lookup_unique_by_name(&index, "ambush", &[SymbolKind::Knot], None),
            None,
            "two same-named candidates: only the scoped lookup can decide, so decline"
        );
        assert_ne!(a, b, "the fixture must really hold two distinct candidates");

        // The same index, filtered to a kind exactly one candidate has:
        // now `lookup_by_name`'s own byte-identity fast path returns it
        // regardless of scope, so the scope-free answer must match both.
        let mut single = SymbolIndex::default();
        let (&only_id, only_info) = index
            .symbols
            .iter()
            .find(|(id, _)| **id == a)
            .expect("fixture id present");
        single.symbols.insert(only_id, only_info.clone());
        single.by_name.insert("ambush".to_string(), vec![only_id]);
        let scope_a = ImportScope {
            file_module: None,
            qualified_modules: ["quest_a".to_string()].into_iter().collect(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        let scope_none = ImportScope {
            file_module: None,
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        let unique = lookup_unique_by_name(&single, "ambush", &[SymbolKind::Knot], None);
        assert_eq!(unique, Some(a));
        assert_eq!(
            unique,
            lookup_by_name(&single, &scope_a, "ambush", &[SymbolKind::Knot])
        );
        assert_eq!(
            unique,
            lookup_by_name(&single, &scope_none, "ambush", &[SymbolKind::Knot]),
            "the sole-candidate answer must not depend on the scope at all"
        );
        assert_eq!(
            lookup_unique_by_name(&single, "ambush", &[SymbolKind::External], None),
            None,
            "the kind filter still gates the match"
        );
    }

    #[test]
    fn same_module_candidate_wins_over_imported_one() {
        let (index, a, b) = two_module_ambush_index();
        // A file *inside* quest_b that also imports quest_a: its own module's
        // `ambush` is bare-visible and wins over the imported homonym.
        let scope = ImportScope {
            file_module: Some("quest_b".to_string()),
            qualified_modules: ["quest_a".to_string()].into_iter().collect(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(b),
            "own-module definition beats an imported homonym"
        );
        let _ = a;
    }

    /// Build a `SymbolIndex` with one `Knot` named `ambush` per module in
    /// `modules`, all `Public` — the shape [`two_module_ambush_index`] hands
    /// M-2d, generalized so a std-shaped module string can sit alongside an
    /// ordinary one.
    fn ambush_index_with_modules(modules: &[&str]) -> (SymbolIndex, Vec<DefinitionId>) {
        use brink_format::DefinitionTag;
        let mut index = SymbolIndex::default();
        let mut ids = Vec::new();
        for (i, module) in modules.iter().enumerate() {
            let id = DefinitionId::new(DefinitionTag::Address, 0xA + i as u64);
            index.symbols.insert(
                id,
                SymbolInfo {
                    kind: SymbolKind::Knot,
                    file: FileId(0),
                    range: TextRange::default(),
                    id,
                    name: "ambush".to_string(),
                    params: Vec::new(),
                    detail: None,
                    scope: None,
                    param_detail: None,
                    module: Some((*module).to_string()),
                    visibility: Visibility::Public,
                },
            );
            index
                .by_name
                .entry("ambush".to_string())
                .or_default()
                .push(id);
            ids.push(id);
        }
        (index, ids)
    }

    /// Issue #2197, per #2080's SCOPE FENCE (`docs/decision-log.md`,
    /// "Stdlib mounts into `Environment`'s manifest at the producer, as
    /// plain source"): a std-mounted candidate must be invisible to
    /// bare-name resolution — not merely deprioritized — even when it is
    /// the *sole* candidate. Before this fix, `lookup_by_name_direct`'s
    /// `!multiple` fast path returned any sole candidate unconditionally
    /// regardless of scope, which would have let a project silently reach
    /// into `std::…` with zero imports.
    #[test]
    fn std_mounted_sole_candidate_is_invisible_with_no_import() {
        let (index, _ids) = ambush_index_with_modules(&["std::conventions::screenplay"]);
        assert_eq!(
            lookup_by_name(
                &index,
                &ImportScope::default(),
                "ambush",
                &[SymbolKind::Knot]
            ),
            None,
            "a std-mounted definition must not resolve by bare name with no `use std::…` \
             import — reaching it requires an explicit import, which does not exist yet \
             (#1582/#2167), so today it must resolve to nothing rather than silently reach std"
        );
    }

    /// The E060 collision shape, at the resolution layer rather than the
    /// LIR-lowering self-identity layer `stdlib_mount_no_longer_collides_
    /// with_a_projects_own_scene_entered` (brink-test-harness) proves
    /// end to end: a project's own declared module and the std mount both
    /// declare `ambush`. A file *inside* the project's own module resolves
    /// its own `ambush` via the pre-existing `Candidacy::InScope` tier,
    /// which already wins the tie-break with or without this issue's std
    /// gate — `project_referencing_a_third_module_still_skips_a_coexisting_
    /// std_candidate` below is the case that actually distinguishes pre-fix
    /// from post-fix behavior for the `Other`/`Other` shape this gate
    /// targets. Kept as its own test because the `InScope` tier is a real,
    /// separate guarantee worth pinning on its own.
    #[test]
    fn project_own_module_wins_over_a_coexisting_std_mount_candidate() {
        let (index, ids) =
            ambush_index_with_modules(&["story::story", "std::conventions::screenplay"]);
        let project_id = ids[0];
        let scope = ImportScope {
            file_module: Some("story::story".to_string()),
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(project_id),
            "a file inside `story::story` must resolve its OWN `ambush`, never the coexisting \
             std mount's same-named one"
        );
    }

    /// Review finding on #2197: the test above does **not** actually
    /// exercise the std-`Other` exclusion — with `file_module ==
    /// "story::story"`, the project candidate classifies `Candidacy::
    /// InScope` and wins via `first_in_scope` regardless of whether the std
    /// gate exists at all (reverting it changes nothing about that test's
    /// outcome). This test instead puts the referring file in a **third**
    /// declared module, so *neither* candidate is `InScope`: the project's
    /// `ambush` classifies `Other` (a real cross-module reference this file
    /// has no import for) and the std mount's `ambush` also classifies
    /// `Other`. Without the gate, the flat fallback picks whichever `Other`
    /// candidate was inserted first in `by_name` — here, the std one,
    /// listed first — so this test fails with the gate removed and passes
    /// only because the std candidate is skipped before it can ever become
    /// `first_match`.
    #[test]
    fn project_referencing_a_third_module_still_skips_a_coexisting_std_candidate() {
        let (index, ids) =
            ambush_index_with_modules(&["std::conventions::screenplay", "story::story"]);
        let project_id = ids[1];
        let scope = ImportScope {
            file_module: Some("story::another_module".to_string()),
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(project_id),
            "with neither candidate `InScope`, the std `Other` candidate must still be \
             skipped rather than winning the flat first-inserted tie-break"
        );
    }

    /// Issue #2216 (the #2197 follow-up this doc's own "known gap" pointed
    /// at), narrowed by #2233: with no `referrer_module` hint at all (`None`
    /// — the shape every pre-#2233 caller effectively had), a std-mounted
    /// candidate must still be unconditionally invisible here, exactly as it
    /// is to [`lookup_by_name`] with the default (no-import) scope, even
    /// when it is the function's *sole* candidate — the case the old
    /// `!multiple` style fast path would otherwise return unconditionally.
    #[test]
    fn unique_lookup_excludes_std_mounted_sole_candidate() {
        let (index, _ids) = ambush_index_with_modules(&["std::conventions::screenplay"]);
        assert_eq!(
            lookup_unique_by_name(&index, "ambush", &[SymbolKind::Knot], None),
            None,
            "a std-mounted sole candidate must not resolve through the scope-free path when \
             the caller has no referrer-module hint — lookup_by_name returns None for it under \
             the default scope, so lookup_unique_by_name must agree rather than silently \
             reaching into std with no import"
        );
    }

    /// The unification half of #2216: with a std-mounted candidate
    /// coexisting alongside one ordinary candidate, [`lookup_unique_by_name`]
    /// must still resolve the ordinary one (not decline as ambiguous, and
    /// not pick the std one) — agreeing with [`lookup_by_name`] for a scope
    /// where neither candidate is `InScope` (asserted below with
    /// `file_module = "story::another_module"`, passed through as
    /// `referrer_module`). That is not the *only* scope where the two
    /// agree — a scope with `file_module = "story::story"` (the ordinary
    /// candidate's own module) also agrees, since the ordinary candidate
    /// then classifies `InScope` and wins [`lookup_by_name`]'s own
    /// tie-break. The std-referrer case — once the one deliberate
    /// disagreement this doc used to call out — is now covered separately
    /// by `unique_lookup_reproduces_in_scope_std_sibling_with_referrer_module`
    /// below, now that issue #2233 threads a `referrer_module` hint through.
    #[test]
    fn unique_lookup_skips_std_candidate_and_returns_the_ordinary_one() {
        let (index, ids) =
            ambush_index_with_modules(&["std::conventions::screenplay", "story::story"]);
        let project_id = ids[1];
        assert_eq!(
            lookup_unique_by_name(&index, "ambush", &[SymbolKind::Knot], None),
            Some(project_id),
            "the std candidate must be excluded from the sole-match count entirely, leaving \
             the one ordinary candidate as the unique match"
        );
        let other_scope = ImportScope {
            file_module: Some("story::another_module".to_string()),
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_unique_by_name(
                &index,
                "ambush",
                &[SymbolKind::Knot],
                Some("story::another_module")
            ),
            lookup_by_name(&index, &other_scope, "ambush", &[SymbolKind::Knot]),
            "the scope-free answer must agree with the scoped one for a scope where neither \
             candidate is InScope"
        );
        let project_scope = ImportScope {
            file_module: Some("story::story".to_string()),
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_unique_by_name(&index, "ambush", &[SymbolKind::Knot], Some("story::story")),
            lookup_by_name(&index, &project_scope, "ambush", &[SymbolKind::Knot]),
            "the scope-free answer must also agree with the scoped one for a scope where the \
             ordinary candidate (not the std one) is InScope"
        );
    }

    /// Issue #2233: the fix for the one deliberate disagreement
    /// `unique_lookup_excludes_std_mounted_sole_candidate`'s old sibling doc
    /// used to describe (the pre-fix version of this file documented it on
    /// `unique_lookup_skips_std_candidate_and_returns_the_ordinary_one`). A
    /// referrer whose own `file_module` IS the std module keeps resolving
    /// the std candidate via [`lookup_by_name_direct`]'s `InScope` tier
    /// (std's own internal references are untouched by the #2197/#2216
    /// gates) — and now that `lookup_unique_by_name` is handed that same
    /// module string as `referrer_module`, it reproduces the identical
    /// answer instead of excluding the std candidate unconditionally, for
    /// the case that matters: the std candidate is the *sole* match once
    /// visible (no coexisting ordinary candidate of the same name — see
    /// `unique_lookup_still_declines_when_a_visible_std_sibling_is_ambiguous`
    /// for what happens when one does coexist).
    #[test]
    fn unique_lookup_reproduces_in_scope_std_sibling_with_referrer_module() {
        let (index, ids) = ambush_index_with_modules(&["std::conventions::screenplay"]);
        let std_id = ids[0];
        let std_scope = ImportScope {
            file_module: Some("std::conventions::screenplay".to_string()),
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &std_scope, "ambush", &[SymbolKind::Knot]),
            Some(std_id),
            "a referrer whose own file_module IS the std module keeps resolving the std \
             candidate via lookup_by_name_direct's InScope tier — std's own internal \
             references are untouched by the #2197/#2216 gates"
        );
        assert_eq!(
            lookup_unique_by_name(
                &index,
                "ambush",
                &[SymbolKind::Knot],
                Some("std::conventions::screenplay")
            ),
            lookup_by_name(&index, &std_scope, "ambush", &[SymbolKind::Knot]),
            "with the referrer's own module threaded through, lookup_unique_by_name now agrees \
             with lookup_by_name for a referrer inside the std tree looking up a std sibling — \
             the #2233 fix"
        );
    }

    /// Issue #2249: `resolve_type_ref` (the new `RefKind::Type` arm) routes
    /// a TM-2 annotation through this file's own `lookup_by_name` — full
    /// `ImportScope`/`Candidacy` semantics — rather than
    /// `lir::lower::decls::lookup_global`'s narrower fallback, which
    /// `ShapeTable::resolve` used before this issue (deleted; see
    /// `brink-ir::lir::lower::structs`'s module doc). The two primitives
    /// disagree on exactly this shape: a referrer *inside* a std module
    /// referencing a **sibling** struct in the *same* std module, declared
    /// in a different file, with no explicit import — `lookup_global`
    /// excludes every std-declared candidate unconditionally in its
    /// fallback arm (no referrer-is-std carve-out, `decls.rs`'s own doc),
    /// so this would have resolved to `None` under the old
    /// `ShapeTable::resolve`; `lookup_by_name_direct`'s `InScope` tier
    /// (std's own internal references are untouched by the #2197/#2216
    /// gates, same as the `Knot` case `unique_lookup_reproduces_in_scope_
    /// std_sibling_with_referrer_module` above proves) resolves it. This is
    /// the one behavioral delta issue #2249's PR body must call out: a
    /// std convention file's own `~ temp c: Cue`-shaped annotation
    /// referencing a *sibling* std file's struct now gets the static-offset
    /// chase (`known_shape`) it silently lost before.
    #[test]
    fn resolve_type_ref_reproduces_in_scope_std_sibling_with_referrer_module() {
        let mut index = SymbolIndex::default();
        let cue_id = DefinitionId::new(brink_format::DefinitionTag::StructDef, 0xC0E);
        index.symbols.insert(
            cue_id,
            SymbolInfo {
                kind: SymbolKind::Struct,
                file: FileId(9),
                range: TextRange::default(),
                id: cue_id,
                name: "Cue".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("std::conventions::screenplay".to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("Cue".to_string())
            .or_default()
            .push(cue_id);

        let std_scope = ImportScope {
            file_module: Some("std::conventions::screenplay".to_string()),
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        let referrer_file = FileId(1); // a *different* std file, same module
        let uref = UnresolvedRef {
            path: "Cue".to_string(),
            range: range(0, 3),
            kind: RefKind::Type,
            scope: Scope::default(),
            arg_count: None,
            module_qualified: false,
        };
        let mut map: ResolutionMap = Vec::new();
        resolve_type_ref(&index, &std_scope, referrer_file, &uref, &mut map);

        assert_eq!(
            map,
            vec![ResolvedRef {
                file: referrer_file,
                range: uref.range,
                target: cue_id,
            }],
            "a referrer inside std referencing a sibling std file's struct with no import \
             resolves via lookup_by_name_direct's InScope tier — the exact case \
             ShapeTable::resolve's old lookup_global-based fallback could never reach \
             (it excluded every std-declared candidate unconditionally, referrer or not)"
        );
    }

    /// Issue #2249: the flip side of the test above — a scalar/tower/
    /// generic-head keyword (never a declared struct) must resolve to
    /// nothing and raise no diagnostic, because `resolve_type_ref`'s own
    /// doc is explicit that "no declared struct named this" is the
    /// legal, common case for a `RefKind::Type` reference, not an error.
    #[test]
    fn resolve_type_ref_silently_misses_a_scalar_keyword_name() {
        let index = SymbolIndex::default();
        let uref = UnresolvedRef {
            path: "int".to_string(),
            range: range(0, 3),
            kind: RefKind::Type,
            scope: Scope::default(),
            arg_count: None,
            module_qualified: false,
        };
        let mut map: ResolutionMap = Vec::new();
        resolve_type_ref(&index, &ImportScope::default(), FileId(0), &uref, &mut map);
        assert!(
            map.is_empty(),
            "`int` never names a declared STRUCT — this must not resolve, and (unlike \
             RefKind::Struct's E068) resolve_type_ref never diagnoses a miss either, since a \
             miss here is not necessarily wrong"
        );
    }

    /// PR #2271 review finding: `lir::lower::structs`'s own tests
    /// (`lookup_global_excludes_a_sole_std_declared_struct_with_no_project_
    /// homonym`, `lookup_global_picks_the_referrers_own_shape_when_names_
    /// collide`) were retargeted at `decls::lookup_global` after issue
    /// #2249 deleted `ShapeTable::resolve` — but annotations no longer call
    /// `lookup_global` at all; they go through this file's own
    /// `resolve_type_ref`/`lookup_by_name`/`ImportScope` machinery (this
    /// module's own doc on `resolve_type_ref`). Neither retargeted test, nor
    /// `resolve_type_ref_silently_misses_a_scalar_keyword_name` above (an
    /// empty index, no std/project homonym in play at all), exercises the
    /// std-exclusion property through the real annotation path. This test
    /// closes that gap's negative half: a struct only a mounted std module
    /// declares, referenced from an ordinary (non-std, non-importing) project
    /// file, must not resolve — mirroring
    /// `std_mounted_sole_candidate_is_invisible_with_no_import` above, but
    /// through `resolve_type_ref` (the real TM-2 annotation path) instead of
    /// `lookup_by_name` directly.
    #[test]
    fn resolve_type_ref_excludes_a_std_only_struct_with_no_project_homonym_or_import() {
        let mut index = SymbolIndex::default();
        let cue_id = DefinitionId::new(brink_format::DefinitionTag::StructDef, 0xC0F);
        index.symbols.insert(
            cue_id,
            SymbolInfo {
                kind: SymbolKind::Struct,
                file: FileId(9),
                range: TextRange::default(),
                id: cue_id,
                name: "Cue".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("std::conventions::screenplay".to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("Cue".to_string())
            .or_default()
            .push(cue_id);

        // An ordinary project file, no `#@module`/`use` in play at all —
        // the default scope every non-modules file carries.
        let uref = UnresolvedRef {
            path: "Cue".to_string(),
            range: range(0, 3),
            kind: RefKind::Type,
            scope: Scope::default(),
            arg_count: None,
            module_qualified: false,
        };
        let mut map: ResolutionMap = Vec::new();
        resolve_type_ref(&index, &ImportScope::default(), FileId(0), &uref, &mut map);
        assert!(
            map.is_empty(),
            "a struct only a mounted std module declares must not resolve for a `~ temp c: Cue`- \
             shaped annotation with no project-side homonym and no import — the sole-candidate \
             std-exclusion property `resolve_type_ref`'s own doc claims, unproven by any test \
             through this path before: {map:?}"
        );
    }

    /// PR #2271 review finding, positive half: a project's own struct and a
    /// coexisting mounted std struct sharing a bare name (M-2d, issue
    /// #2238) — a referrer *inside* the project's own declared module must
    /// resolve its own struct through `resolve_type_ref`, never the std
    /// mount's same-named one. Mirrors `project_own_module_wins_over_a_
    /// coexisting_std_mount_candidate` above, but through the real
    /// annotation path.
    #[test]
    fn resolve_type_ref_picks_the_referrers_own_project_struct_over_a_coexisting_std_homonym() {
        let mut index = SymbolIndex::default();
        let std_cue_id = DefinitionId::new(brink_format::DefinitionTag::StructDef, 0xC10);
        index.symbols.insert(
            std_cue_id,
            SymbolInfo {
                kind: SymbolKind::Struct,
                file: FileId(9),
                range: TextRange::default(),
                id: std_cue_id,
                name: "Cue".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("std::conventions::screenplay".to_string()),
                visibility: Visibility::Public,
            },
        );
        let project_cue_id = DefinitionId::new(brink_format::DefinitionTag::StructDef, 0xC11);
        index.symbols.insert(
            project_cue_id,
            SymbolInfo {
                kind: SymbolKind::Struct,
                file: FileId(1),
                range: TextRange::default(),
                id: project_cue_id,
                name: "Cue".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("story::market".to_string()),
                visibility: Visibility::Public,
            },
        );
        for id in [std_cue_id, project_cue_id] {
            index.by_name.entry("Cue".to_string()).or_default().push(id);
        }

        let scope = ImportScope {
            file_module: Some("story::market".to_string()),
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        let uref = UnresolvedRef {
            path: "Cue".to_string(),
            range: range(0, 3),
            kind: RefKind::Type,
            scope: Scope::default(),
            arg_count: None,
            module_qualified: false,
        };
        let mut map: ResolutionMap = Vec::new();
        resolve_type_ref(&index, &scope, FileId(1), &uref, &mut map);
        assert_eq!(
            map,
            vec![ResolvedRef {
                file: FileId(1),
                range: uref.range,
                target: project_cue_id,
            }],
            "a `story::market` file's own `~ temp c: Cue` must resolve to `story::market`'s own \
             Cue, never the coexisting std mount's same-named one: {map:?}"
        );
    }

    /// Issue #2233: threading `referrer_module` through fixes the *sole
    /// candidate* disagreement above, but does not (and cannot, without
    /// replicating `lookup_by_name_direct`'s full `InScope`-beats-`Other`
    /// tie-break, a strictly larger change than a referrer hint) make
    /// `lookup_unique_by_name` reproduce [`lookup_by_name`]'s tie-break when
    /// an ordinary same-name candidate coexists with the now-visible std
    /// sibling. There, un-excluding the std candidate makes it a *second*
    /// candidate rather than the resolved one, so this function declines
    /// (`None`) exactly per its own documented contract ("when it returns
    /// `None` on an ambiguous name, the caller must fall back... rather than
    /// guess") — a safe decline, not the silently-wrong `Some(project_id)`
    /// answer this exact fixture produced before #2233 (the referrer-module
    /// hint being `None` in every pre-#2233 call site is what caused that:
    /// the std candidate was excluded unconditionally, leaving the ordinary
    /// one as a false "sole" match).
    #[test]
    fn unique_lookup_still_declines_when_a_visible_std_sibling_is_ambiguous() {
        let (index, _ids) =
            ambush_index_with_modules(&["std::conventions::screenplay", "story::story"]);
        assert_eq!(
            lookup_unique_by_name(
                &index,
                "ambush",
                &[SymbolKind::Knot],
                Some("std::conventions::screenplay")
            ),
            None,
            "with the std candidate now visible (referrer inside its own module) alongside a \
             coexisting ordinary candidate, the name is genuinely ambiguous to this scope-free \
             function — it must decline rather than silently pick either one"
        );
    }

    /// Issue #2233: a referrer *inside* std but in a *different* std module
    /// than the candidate must not gain visibility either — only an exact
    /// module match reproduces `InScope`; a cross-std-submodule reference is
    /// `Other` under `classify` (no import machinery to promote it to
    /// `Imported` here), so it stays excluded exactly like any other
    /// cross-module std reference.
    #[test]
    fn unique_lookup_still_excludes_a_different_std_sibling_module() {
        let (index, ids) =
            ambush_index_with_modules(&["std::conventions::screenplay", "std::conventions::other"]);
        let other_id = ids[1];
        assert_eq!(
            lookup_unique_by_name(
                &index,
                "ambush",
                &[SymbolKind::Knot],
                Some("std::conventions::other")
            ),
            Some(other_id),
            "the referrer's own std module's candidate still resolves (InScope, exact match)"
        );
        let screenplay_scope = ImportScope {
            file_module: Some("std::conventions::other".to_string()),
            qualified_modules: BTreeSet::new(),
            bare_imports: BTreeSet::new(),
            aliases: BTreeMap::new(),
        };
        assert_eq!(
            lookup_by_name(&index, &screenplay_scope, "ambush", &[SymbolKind::Knot]),
            Some(other_id),
            "sanity: lookup_by_name agrees — the referrer's own module wins, not the sibling"
        );
    }

    /// Issue #2217: `is_std_module`/the std-invisibility gate excludes a
    /// name from bare-name resolution identically whether the candidate is
    /// the *embedded* stdlib mount or a project's own file that legitimately
    /// lives at a `std/…` path (`mount_stdlib`'s "project source at the same
    /// key wins" carve-out — both mint the same `std::…` module identity,
    /// by design, per #2245's "peer roots" ruling). Before this fix, the
    /// resulting diagnostic was a bare "unresolved name" with nothing
    /// distinguishing "no such symbol anywhere" from "this symbol exists,
    /// but only under `std::`, invisible by rule" — an author who names
    /// their own directory `std/` had no way to learn that from the
    /// diagnostic alone.
    #[test]
    fn is_std_shadowed_name_true_when_only_a_std_candidate_exists() {
        let (index, _ids) = ambush_index_with_modules(&["std::conventions::screenplay"]);
        assert!(
            is_std_shadowed_name(&index, "ambush"),
            "a name whose sole declaration lives under the std peer root must be reported as \
             std-shadowed"
        );
    }

    #[test]
    fn is_std_shadowed_name_false_for_an_ordinary_project_name() {
        let (index, _ids) = ambush_index_with_modules(&["story::story"]);
        assert!(
            !is_std_shadowed_name(&index, "ambush"),
            "a name declared only in an ordinary project module must not be reported as \
             std-shadowed"
        );
        assert!(
            !is_std_shadowed_name(&index, "no_such_name"),
            "a name with no declaration at all must not be reported as std-shadowed either"
        );
    }

    #[test]
    fn unresolved_diag_hints_at_std_shadowing_when_a_std_candidate_exists() {
        let (index, _ids) = ambush_index_with_modules(&["std::conventions::screenplay"]);
        let diag = unresolved_diag(
            &index,
            &ImportScope::default(),
            FileId(0),
            range(0, 6),
            "ambush",
            DiagnosticCode::E025,
            &[],
        );
        assert_eq!(diag.code, DiagnosticCode::E025);
        assert!(
            diag.message.contains("std::") && diag.message.contains("peer root"),
            "the diagnostic for a name that IS declared, but only under std, must say so \
             rather than reading as an ordinary unresolved-name error: {}",
            diag.message
        );
    }

    #[test]
    fn unresolved_diag_stays_plain_when_no_std_candidate_exists() {
        let index = SymbolIndex::default();
        let diag = unresolved_diag(
            &index,
            &ImportScope::default(),
            FileId(0),
            range(0, 6),
            "nope",
            DiagnosticCode::E025,
            &[],
        );
        assert_eq!(
            diag.message,
            format!("{}: `nope`", DiagnosticCode::E025.title()),
            "a genuinely unresolved name (no std candidate anywhere) must keep the plain \
             message — the hint must not fire spuriously"
        );
    }

    /// End-to-end through the real resolution entry point, not just the
    /// diagnostic-formatting helper directly: a project file that places its
    /// own `Variable` declaration under a `std/…` path (the exact landmine
    /// #2217 describes — nothing marks this as the embedded stdlib) is
    /// invisible to a bare reference from the rest of the project, and the
    /// `E025` this produces must carry the std-shadowing hint.
    #[test]
    fn resolve_variable_hints_std_shadowing_for_a_projects_own_std_path_file() {
        let mut index = SymbolIndex::default();
        let id = DefinitionId::new(brink_format::DefinitionTag::Address, 0xF00D);
        index.symbols.insert(
            id,
            SymbolInfo {
                kind: SymbolKind::Variable,
                file: FileId(0),
                range: TextRange::default(),
                id,
                name: "screenplay_intro".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("std::conventions::screenplay".to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("screenplay_intro".to_string())
            .or_default()
            .push(id);

        let scope = ImportScope::default();
        let locals: Vec<LocalSymbol> = Vec::new();
        let mut map: ResolutionMap = Vec::new();
        let mut diagnostics = Vec::new();
        let uref = uref("screenplay_intro", RefKind::Variable, None, None);

        resolve_variable(
            &index,
            &scope,
            &locals,
            FileId(1),
            &uref,
            &mut map,
            &mut diagnostics,
        );

        assert!(map.is_empty(), "a std-shadowed candidate must not resolve");
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::E025);
        assert!(
            diagnostics[0].message.contains("std::"),
            "the real resolution path's E025 must carry the std-shadowing hint too, not just \
             the diagnostic-formatting helper in isolation: {}",
            diagnostics[0].message
        );
    }

    #[test]
    fn default_scope_falls_back_to_flat_first_winner() {
        // Byte-identity guard: with no import context (the pre-M-2d world),
        // a multi-candidate lookup returns the flat first-inserted winner,
        // exactly as the old flat resolver did.
        let (index, a, _b) = two_module_ambush_index();
        assert_eq!(
            lookup_by_name(
                &index,
                &ImportScope::default(),
                "ambush",
                &[SymbolKind::Knot]
            ),
            Some(a),
            "no imports → flat first-winner, unchanged from pre-M-2d"
        );
    }

    /// `ImportScope` granularity regression (issue #790 review): a bare
    /// import must be name-precise, matching `modules::import_covers`
    /// exactly. `ImportScope::new` used to collapse every import to just its
    /// module name (`imports.iter().map(|i| i.module.clone())`), so a bare
    /// `IMPORT { other } FROM quest_a` wrongly counted as importing *all* of
    /// `quest_a` — including its unrelated public `ambush`.
    #[test]
    fn bare_import_grants_candidacy_only_for_its_own_named_item() {
        let (index, _a, b) = two_module_ambush_index();
        let scope = ImportScope::new(
            None,
            &[
                Import {
                    module: "quest_a".to_string(),
                    module_range: TextRange::default(),
                    items: vec![ImportItem {
                        name: "other".to_string(),
                        alias: None,
                        range: TextRange::default(),
                    }],
                    bare: true,
                    range: TextRange::default(),
                },
                Import {
                    module: "quest_b".to_string(),
                    module_range: TextRange::default(),
                    items: vec![ImportItem {
                        name: "ambush".to_string(),
                        alias: None,
                        range: TextRange::default(),
                    }],
                    bare: true,
                    range: TextRange::default(),
                },
            ],
        );
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(b),
            "bare-importing `other` from quest_a must not license quest_a's `ambush` — \
             only quest_b's `ambush` (actually bare-imported) is a candidate"
        );
    }

    /// A qualified `IMPORT mod` still licenses every public export of that
    /// module (unlike a bare import, which is name-precise) — the
    /// granularity fix must not regress this path.
    #[test]
    fn qualified_import_still_grants_candidacy_for_any_export() {
        let (index, a, _b) = two_module_ambush_index();
        let scope = ImportScope::new(
            None,
            &[Import {
                module: "quest_a".to_string(),
                module_range: TextRange::default(),
                items: Vec::new(),
                bare: false,
                range: TextRange::default(),
            }],
        );
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(a),
            "a qualified `IMPORT quest_a` still licenses quest_a's `ambush`"
        );
    }

    // ── Import aliasing (issue #1590) ───────────────────────────────

    /// The headline bug: `IMPORT { ambush AS b } FROM quest_a` must make `b`
    /// resolve — before the fix, `ImportItem.alias` was read only by the
    /// E089 duplicate check, so a reference to the alias found nothing in
    /// `index.by_name` (keyed by definitions' own spellings only) and
    /// resolution silently failed.
    #[test]
    fn aliased_bare_import_resolves_via_its_local_alias() {
        let (index, a, _b) = two_module_ambush_index();
        let scope = ImportScope::new(
            None,
            &[Import {
                module: "quest_a".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "ambush".to_string(),
                    alias: Some("b".to_string()),
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        );
        assert_eq!(
            lookup_by_name(&index, &scope, "b", &[SymbolKind::Knot]),
            Some(a),
            "`ambush AS b` must make `b` resolve to quest_a's `ambush`"
        );
    }

    /// Additive ruling (issue #1590 — "is the original name still
    /// licensed?"): brink's alias is additive, not Rust's shadow-and-revoke.
    /// The source spelling stays resolvable through the very same import —
    /// see the doc comment on [`lookup_by_name`] for the full justification
    /// (the fast path's byte-identity guarantee already ignores `ImportScope`
    /// for a globally-unique name, so a strict revoke would only sometimes
    /// hold).
    #[test]
    fn aliased_bare_import_also_still_resolves_via_its_original_name() {
        let (index, a, _b) = two_module_ambush_index();
        let scope = ImportScope::new(
            None,
            &[Import {
                module: "quest_a".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "ambush".to_string(),
                    alias: Some("b".to_string()),
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        );
        assert_eq!(
            lookup_by_name(&index, &scope, "ambush", &[SymbolKind::Knot]),
            Some(a),
            "the source name `ambush` must still resolve alongside its alias `b`"
        );
    }

    /// Negative case: an alias is scoped to the exact `(module, kind)` its
    /// import named — it must never resolve against a same-named symbol of a
    /// different kind, nor leak into a file that never declared it.
    #[test]
    fn alias_does_not_resolve_the_wrong_kind() {
        let (index, _a, _b) = two_module_ambush_index();
        let scope = ImportScope::new(
            None,
            &[Import {
                module: "quest_a".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "ambush".to_string(),
                    alias: Some("b".to_string()),
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        );
        assert_eq!(
            lookup_by_name(&index, &scope, "b", &[SymbolKind::Variable]),
            None,
            "`b` aliases a Knot; it must not resolve when a Variable is requested"
        );
    }

    /// Negative case: a name that is neither imported nor aliased in this
    /// file's scope must not resolve just because *some* file's `ImportScope`
    /// carries an alias for it — `lookup_by_name` is per-file.
    #[test]
    fn unrelated_scope_has_no_alias_and_does_not_resolve() {
        let (index, _a, _b) = two_module_ambush_index();
        assert_eq!(
            lookup_by_name(&index, &ImportScope::default(), "b", &[SymbolKind::Knot]),
            None,
            "a file with no import scope must never resolve an alias it never declared"
        );
    }

    /// Precedence when an alias collides with an in-scope direct name (see
    /// the doc comment on [`lookup_by_name`]): `lookup_by_name_direct` always
    /// runs first, so a local knot named `start` wins over an alias `start`
    /// that a bare import bound to a *different* knot — the alias fallback
    /// only ever fires once the direct lookup comes up empty. `IMPORT {
    /// haggle AS start } FROM quest_a` in a file that also defines `start`
    /// silently loses the alias to the local definition.
    #[test]
    fn alias_colliding_with_an_in_scope_direct_name_resolves_to_the_direct_name() {
        use brink_format::DefinitionTag;
        let mut index = SymbolIndex::default();
        let local_start = DefinitionId::new(DefinitionTag::Address, 0x51A47);
        index.symbols.insert(
            local_start,
            SymbolInfo {
                kind: SymbolKind::Knot,
                file: FileId(0),
                range: TextRange::default(),
                id: local_start,
                name: "start".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: None,
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("start".to_string())
            .or_default()
            .push(local_start);

        let (ambush_index, aliased_target, _b) = two_module_ambush_index();
        for (name, ids) in ambush_index.by_name {
            index.by_name.entry(name).or_default().extend(ids);
        }
        for (id, info) in ambush_index.symbols {
            index.symbols.insert(id, info);
        }

        let scope = ImportScope::new(
            None,
            &[Import {
                module: "quest_a".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "ambush".to_string(),
                    alias: Some("start".to_string()),
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        );

        assert_eq!(
            lookup_by_name(&index, &scope, "start", &[SymbolKind::Knot]),
            Some(local_start),
            "a direct in-scope `start` must win over the colliding alias — \
             `ambush AS start` never reaches quest_a's ambush ({aliased_target:?}) \
             under that name"
        );
    }

    // ── Issue #2287: module-qualified divert resolution ──────────────
    //
    // `market/barter.brink` declaring `pub flow haggle()` (module
    // `story::market::barter`), referenced from `story.brink`. Four rows,
    // the maintainer's corrected model (issue #2287's comment):
    //
    //   | import                             | `-> barter::haggle` | `-> haggle` |
    //   |-------------------------------------|----------------------|-------------|
    //   | *(none)*                            | rejected             | rejected    |
    //   | `use story::market::barter;`        | accepted             | rejected    |
    //   | `use story::market::barter::haggle;`| —                    | accepted    |
    //
    // The fourth row (`use story::market::barter::*;`, a glob import) is
    // not exercised here — the native grammar has no glob-`use` production
    // at all (`brink_syntax_native::parser::decl::use_tree` accepts only an
    // `IDENT` segment, a `{ … }` group, or a trailing `as` alias; no `*`
    // arm), so that import spelling cannot be constructed as HIR in the
    // first place. See this PR's body for the full finding.

    fn haggle_index(module: &str) -> (SymbolIndex, DefinitionId) {
        use brink_format::DefinitionTag;
        let mut index = SymbolIndex::default();
        let id = DefinitionId::new(DefinitionTag::Address, 0x748);
        index.symbols.insert(
            id,
            SymbolInfo {
                kind: SymbolKind::Knot,
                file: FileId(1),
                range: TextRange::default(),
                id,
                name: "haggle".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some(module.to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("haggle".to_string())
            .or_default()
            .push(id);
        (index, id)
    }

    fn divert_uref(path: &str, module_qualified: bool) -> UnresolvedRef {
        UnresolvedRef {
            path: path.to_string(),
            range: range(0, path.len() as u32),
            kind: RefKind::Divert,
            scope: Scope::default(),
            arg_count: None,
            module_qualified,
        }
    }

    /// Bug (a): `use story::market::barter;` (a module-qualified import —
    /// lowered as a bare import of item `barter` from `story::market`, whose
    /// #1592 dual-reading also licenses `story::market::barter` as a
    /// qualified module candidate) must accept `-> barter::haggle`.
    ///
    /// Reverting `lookup_divert`'s `uref.module_qualified` branch makes this
    /// fail: the old code only ever tried `path.contains('.')`, and a
    /// `::`-joined path with no dot at all falls through every branch to
    /// `None` — reproducing the exact reported defect (`unresolved divert
    /// target: barter.haggle`, before this fix normalized `::` to `.` and
    /// lost the distinction entirely).
    #[test]
    fn qualified_divert_resolves_via_module_qualified_import() {
        let (index, haggle_id) = haggle_index("story::market::barter");
        let scope = ImportScope::new(
            None,
            &[Import {
                module: "story::market".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "barter".to_string(),
                    alias: None,
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        );
        let uref = divert_uref("barter::haggle", true);
        assert_eq!(
            lookup_divert(&index, &scope, &[], &uref),
            Some(haggle_id),
            "`use story::market::barter;` must license the module-qualified \
             `-> barter::haggle` divert"
        );
    }

    /// The other half of bug (a): with no import at all, `-> barter::haggle`
    /// must stay rejected — and the diagnostic must spell the qualifier with
    /// `::`, not the confusing `.` the pre-fix path-joining produced.
    #[test]
    fn qualified_divert_rejected_with_no_import_and_message_uses_double_colon() {
        let (index, _haggle_id) = haggle_index("story::market::barter");
        let scope = ImportScope::default();
        let uref = divert_uref("barter::haggle", true);
        assert_eq!(
            lookup_divert(&index, &scope, &[], &uref),
            None,
            "no import at all must not license the qualified divert"
        );

        let mut diagnostics = Vec::new();
        let mut map = ResolutionMap::new();
        resolve_divert(
            &index,
            &scope,
            &[],
            FileId(0),
            &uref,
            &mut map,
            &mut diagnostics,
        );
        assert!(map.is_empty());
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::E024);
        assert!(
            diagnostics[0].message.contains("barter::haggle"),
            "the E024 message must spell the qualified path with `::` (the \
             native separator actually written), not `.`: {}",
            diagnostics[0].message
        );
    }

    /// Bug (b), the dangerous half: `use story::market::barter;` licenses
    /// the module-qualified spelling (test above) but must NOT also license
    /// the bare `-> haggle` — that was the over-permissive defect issue
    /// #2287 reported as its more dangerous half. Reverting
    /// `lookup_divert`'s step 2 back to `lookup_by_name` makes this fail:
    /// the dual-reading phantom `story::market::barter` entry in
    /// `qualified_modules` would classify `haggle` `Candidacy::Imported`,
    /// and being the sole candidate, the old fast path returned it
    /// unconditionally.
    #[test]
    fn bare_divert_rejected_after_qualified_module_import_only() {
        let (index, _haggle_id) = haggle_index("story::market::barter");
        let scope = ImportScope::new(
            None,
            &[Import {
                module: "story::market".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "barter".to_string(),
                    alias: None,
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        );
        let uref = divert_uref("haggle", false);
        assert_eq!(
            lookup_divert(&index, &scope, &[], &uref),
            None,
            "a qualified-module-only import must not license the bare \
             `-> haggle` spelling"
        );
    }

    /// Row 3 of the corrected table: `use story::market::barter::haggle;`
    /// (a genuine symbol-level bare import — no dual-reading ambiguity,
    /// since the whole prefix `story::market::barter` is unambiguously the
    /// module and `haggle` is unambiguously the item) must license the bare
    /// `-> haggle` spelling.
    #[test]
    fn bare_divert_resolves_via_symbol_level_import() {
        let (index, haggle_id) = haggle_index("story::market::barter");
        let scope = ImportScope::new(
            None,
            &[Import {
                module: "story::market::barter".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "haggle".to_string(),
                    alias: None,
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        );
        let uref = divert_uref("haggle", false);
        assert_eq!(
            lookup_divert(&index, &scope, &[], &uref),
            Some(haggle_id),
            "`use story::market::barter::haggle;` must license the bare \
             `-> haggle` divert"
        );
    }

    /// With no import at all, `lookup_divert` must still resolve the bare
    /// `-> haggle` — end-to-end it is correctly rejected (issue #2287
    /// confirms this row already worked), but that rejection is
    /// `modules::check`'s separate whole-project `E025`/`E087` gate's job,
    /// which only fires on a *resolved* reference (`ResolutionMap` walk,
    /// `check_cross_module_refs`'s own doc). This is the pre-existing,
    /// deliberate byte-identity guarantee `lookup_by_name_direct`'s own doc
    /// describes (`!multiple` sole match wins regardless of candidacy) —
    /// `lookup_knot_bare` must reproduce it exactly, not just for bug (b)'s
    /// qualified-import-only case. `crates/internal/brink-db/src/db.rs`'s
    /// `private_cross_module_reference_is_e087_through_db` and
    /// `public_cross_module_reference_without_import_is_e025` are the real
    /// end-to-end regression pins for this — this unit test is the
    /// resolve-layer half, added after those two briefly regressed to
    /// `E024` during this fix's own development (an over-broad first draft
    /// of `lookup_knot_bare` rejected *any* non-`InScope`/non-bare-imported
    /// candidate, not only a qualified-import-only one).
    #[test]
    fn bare_divert_still_resolves_with_no_import_deferring_to_the_e025_e087_gate() {
        let (index, haggle_id) = haggle_index("story::market::barter");
        let uref = divert_uref("haggle", false);
        assert_eq!(
            lookup_divert(&index, &ImportScope::default(), &[], &uref),
            Some(haggle_id),
            "a totally unimported cross-module Knot must still resolve here — \
             `modules::check`'s E025/E087 gate is what rejects it, with a far \
             more precise diagnostic than a bare E024 would give"
        );
    }

    // ── Issue #2298: the remainder #2287/#2296 deliberately left ─────
    //
    // Item 1 (the live gap): `resolve_function`'s "try knots" step is bug
    // (b)'s call-site twin — a bare `haggle()` after only a module-qualified
    // import must be rejected exactly like bare `-> haggle` already is.
    // Item 2 (latent): `lookup_divert`'s Stitch/Label/Variable+Constant
    // steps share the same exclusion now, plus the `Constant` omission
    // recorded on issue #2083's thread. Item 3: the rejection message
    // names the qualified-import-only candidate it skipped, mirroring
    // `modules::check`'s own E025 "import it from" framing.

    fn function_uref(path: &str, arg_count: usize) -> UnresolvedRef {
        UnresolvedRef {
            path: path.to_string(),
            range: range(0, path.len() as u32),
            kind: RefKind::Function,
            scope: Scope::default(),
            arg_count: Some(arg_count),
            module_qualified: false,
        }
    }

    fn qualified_module_only_scope() -> ImportScope {
        ImportScope::new(
            None,
            &[Import {
                module: "story::market".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "barter".to_string(),
                    alias: None,
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        )
    }

    fn symbol_level_import_scope() -> ImportScope {
        ImportScope::new(
            None,
            &[Import {
                module: "story::market::barter".to_string(),
                module_range: TextRange::default(),
                items: vec![ImportItem {
                    name: "haggle".to_string(),
                    alias: None,
                    range: TextRange::default(),
                }],
                bare: true,
                range: TextRange::default(),
            }],
        )
    }

    /// Item 1's RED case: before this fix, `resolve_function`'s "try knots"
    /// step used the flat `lookup_by_name`/`classify`, under which the
    /// dual-reading phantom `story::market::barter` qualified-module entry
    /// classified `haggle` `Candidacy::Imported` and — being the sole
    /// candidate — the old fast path returned it unconditionally, exactly
    /// reproducing #2287 bug (b) for a call instead of a divert.
    #[test]
    fn bare_call_rejected_after_qualified_module_import_only() {
        let (index, _haggle_id) = haggle_index("story::market::barter");
        let scope = qualified_module_only_scope();
        let uref = function_uref("haggle", 0);
        let mut map = ResolutionMap::new();
        let mut diagnostics = Vec::new();
        resolve_function(
            &index,
            &scope,
            &[],
            FileId(0),
            &uref,
            &mut map,
            &mut diagnostics,
        );
        assert!(
            map.is_empty(),
            "a qualified-module-only import must not license the bare call \
             `haggle()` — issue #2298's live gap, the call-site twin of \
             #2287 bug (b): {map:?}"
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::E025);
    }

    /// Row 3's call-site twin: a genuine symbol-level bare import
    /// (`use story::market::barter::haggle;`) must still license the bare
    /// call `haggle()` — the exclusion must not overcorrect into rejecting
    /// a legitimately bare-imported knot-as-tunnel-function.
    #[test]
    fn bare_call_resolves_via_symbol_level_import() {
        let (index, haggle_id) = haggle_index("story::market::barter");
        let scope = symbol_level_import_scope();
        let uref = function_uref("haggle", 0);
        let mut map = ResolutionMap::new();
        let mut diagnostics = Vec::new();
        resolve_function(
            &index,
            &scope,
            &[],
            FileId(0),
            &uref,
            &mut map,
            &mut diagnostics,
        );
        assert!(
            diagnostics.is_empty(),
            "unexpected diagnostics: {diagnostics:?}"
        );
        assert_eq!(
            map.iter().map(|r| r.target).collect::<Vec<_>>(),
            vec![haggle_id],
            "`use story::market::barter::haggle;` must license the bare call \
             `haggle()`"
        );
    }

    /// Row 4's call-site twin: with no import at all, the call still
    /// resolves here — `modules::check`'s separate E025/E087 gate is the
    /// one that rejects a genuinely unimported cross-module reference, not
    /// this lookup (same deferral `bare_divert_still_resolves_with_no_import_deferring_to_the_e025_e087_gate`
    /// pins for diverts).
    #[test]
    fn bare_call_still_resolves_with_no_import_deferring_to_the_e025_e087_gate() {
        let (index, haggle_id) = haggle_index("story::market::barter");
        let uref = function_uref("haggle", 0);
        let mut map = ResolutionMap::new();
        let mut diagnostics = Vec::new();
        resolve_function(
            &index,
            &ImportScope::default(),
            &[],
            FileId(0),
            &uref,
            &mut map,
            &mut diagnostics,
        );
        assert_eq!(
            map.iter().map(|r| r.target).collect::<Vec<_>>(),
            vec![haggle_id],
            "a totally unimported cross-module Knot must still resolve at a \
             call site, exactly as it does at a divert site"
        );
    }

    /// Item 3: the module-imported-but-bare divert row gets the same
    /// "import it from `module`" framing `modules::check`'s own E025
    /// already gives the "no import at all" row, instead of a bare,
    /// unexplained `E024` naming just `haggle`.
    #[test]
    fn unresolved_diag_hints_at_qualified_import_only_candidate_for_divert() {
        let (index, _haggle_id) = haggle_index("story::market::barter");
        let scope = qualified_module_only_scope();
        let uref = divert_uref("haggle", false);
        let mut map = ResolutionMap::new();
        let mut diagnostics = Vec::new();
        resolve_divert(
            &index,
            &scope,
            &[],
            FileId(0),
            &uref,
            &mut map,
            &mut diagnostics,
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::E024);
        assert!(
            diagnostics[0]
                .message
                .contains("import it from `story::market::barter`"),
            "the module-imported-but-bare row must name the qualified-\
             import-only candidate it skipped: {}",
            diagnostics[0].message
        );
    }

    /// Item 3's call-site twin.
    #[test]
    fn unresolved_diag_hints_at_qualified_import_only_candidate_for_call() {
        let (index, _haggle_id) = haggle_index("story::market::barter");
        let scope = qualified_module_only_scope();
        let uref = function_uref("haggle", 0);
        let mut map = ResolutionMap::new();
        let mut diagnostics = Vec::new();
        resolve_function(
            &index,
            &scope,
            &[],
            FileId(0),
            &uref,
            &mut map,
            &mut diagnostics,
        );
        assert_eq!(diagnostics.len(), 1);
        assert_eq!(diagnostics[0].code, DiagnosticCode::E025);
        assert!(
            diagnostics[0]
                .message
                .contains("import it from `story::market::barter`"),
            "the module-imported-but-bare call row must name the qualified-\
             import-only candidate it skipped too: {}",
            diagnostics[0].message
        );
    }

    fn constant_index(name: &str) -> (SymbolIndex, DefinitionId) {
        use brink_format::DefinitionTag;
        let mut index = SymbolIndex::default();
        let id = DefinitionId::new(DefinitionTag::Address, 0x749);
        index.symbols.insert(
            id,
            SymbolInfo {
                kind: SymbolKind::Constant,
                file: FileId(0),
                range: TextRange::default(),
                id,
                name: name.to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: None,
                visibility: Visibility::Public,
            },
        );
        index.by_name.entry(name.to_string()).or_default().push(id);
        (index, id)
    }

    /// Item 2's `Constant` omission, credited to issue #2083's thread:
    /// `resolve_function`'s own "try variables and constants" step was
    /// widened to `[Variable, Constant]` by #2947 for the call-site gap
    /// #2083 reported, but `lookup_divert`'s step 6 — the divert-target
    /// twin of that same lookup — was left `Variable`-only. A `CONST
    /// target = -> knot` could never be diverted to via `-> target` even
    /// though the call-site twin already accepts a const-bound value.
    ///
    /// Probed directly against a synthetic index (not a real native
    /// compile): today's native surface has no way to construct a
    /// `Constant`-kind divert-target candidate through real source, since
    /// a `flow` always classifies `SymbolKind::Knot` — this is the only
    /// way to exercise step 6's kind list at all, matching the "probe it;
    /// if it turns out unreachable, record WHY" instruction this issue's
    /// build carried.
    #[test]
    fn divert_target_resolves_a_constant_symbol() {
        let (index, const_id) = constant_index("target");
        let uref = divert_uref("target", false);
        assert_eq!(
            lookup_divert(&index, &ImportScope::default(), &[], &uref),
            Some(const_id),
            "a top-level Constant-kind divert target must resolve via step \
             6, matching resolve_function's own [Variable, Constant] \
             call-site lookup (issue #2083's thread)"
        );
    }

    /// Item 2's plumbing proof: the shared [`lookup_bare_excluding_qualified_only`]
    /// honors the exclusion for a non-`Knot` kind too — not just asserted
    /// by code inspection. Unreachable via a real native compile today for
    /// the same reason as the test above (no `Stitch` candidate can carry
    /// a real cross-module `module` yet), so this is a synthetic-index
    /// probe of the shared helper directly.
    #[test]
    fn lookup_bare_excluding_qualified_only_respects_the_exclusion_for_non_knot_kinds() {
        use brink_format::DefinitionTag;
        let mut index = SymbolIndex::default();
        let id = DefinitionId::new(DefinitionTag::Address, 0x750);
        index.symbols.insert(
            id,
            SymbolInfo {
                kind: SymbolKind::Stitch,
                file: FileId(1),
                range: TextRange::default(),
                id,
                name: "haggle".to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: Some("story::market::barter".to_string()),
                visibility: Visibility::Public,
            },
        );
        index
            .by_name
            .entry("haggle".to_string())
            .or_default()
            .push(id);

        let scope = qualified_module_only_scope();
        assert_eq!(
            lookup_bare_excluding_qualified_only(&index, &scope, "haggle", &[SymbolKind::Stitch]),
            None,
            "a qualified-module-only import must not license a bare Stitch \
             lookup any more than it licenses a bare Knot lookup"
        );

        // Should-not-fire control: no import at all still defers to the
        // resolved-but-unimported gate, exactly like the Knot case.
        assert_eq!(
            lookup_bare_excluding_qualified_only(
                &index,
                &ImportScope::default(),
                "haggle",
                &[SymbolKind::Stitch]
            ),
            Some(id)
        );
    }
}
