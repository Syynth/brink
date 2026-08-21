use brink_format::{DefinitionId, DefinitionTag};
use rowan::TextRange;

use crate::determinism::LookupMap;
use crate::symbols::{SymbolIndex, SymbolKind, is_reserved_root_module};
use crate::{Diagnostic, DiagnosticCode, FileId, hir};

use super::context::{
    AnalyzerTables, IdAllocator, NameTable, ResolutionLookup, StructCtx, TempMap,
};
use super::expr::const_value_to_map_key;
use super::lambda;
use super::lir;
use super::structs::ShapeTable;

/// Collect global variable/constant definitions from HIR files.
///
/// Evaluates constants first so that variable initializers like `VAR x = c`
/// can resolve constant references to their values.
///
/// `shapes` is the project's already-built [`ShapeTable`] — a struct
/// construction literal used as a declaration default needs its shape's id
/// and field order to fold into [`lir::ConstValue::Record`] (issue #1530),
/// which is why [`super::build_prelude_decls`] builds the shape table
/// *before* calling this.
///
/// `lambda_ctx` bundles the resources needed only when a default is itself
/// a lambda literal (issue #1774, [`GlobalLambdaCtx`]'s own doc) — kept out
/// of the always-paid parameter list because the overwhelmingly common case
/// (no file-scope lambda anywhere) never touches it.
pub fn collect_globals(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    names: &mut NameTable,
    resolutions: &ResolutionLookup,
    shapes: &ShapeTable,
    diagnostics: &mut Vec<Diagnostic>,
    lambda_ctx: &mut GlobalLambdaCtx<'_>,
) -> Vec<lir::GlobalDef> {
    // Pass 1: evaluate all constants and build a value lookup (keyed
    // lookup only — `LookupMap`, issue #801's audited alias).
    let mut const_values: LookupMap<DefinitionId, lir::ConstValue> = LookupMap::new();
    let mut globals = Vec::new();

    for &(file_id, hir_file) in files {
        // #1504/#1774: qualify this file's lambda-lifted container paths by
        // the owning file, same qualifier `lower_root_content_chunks` gives
        // that file's anonymous choice/gather containers — otherwise two
        // files' `VAR f = |x| x;` at the same source offset would mint the
        // same `DefinitionId`.
        lambda_ctx.ids.set_path_prefix(hir::root_content_scope_path(
            lambda_ctx.file_paths.get(&file_id).map(String::as_str),
        ));
        for cst in &hir_file.constants {
            if let Some(id) = lookup_global_or_diagnose(
                index,
                file_id,
                &cst.name.text,
                SymbolKind::Constant,
                cst.name.range,
                diagnostics,
            ) {
                let name = names.intern(&cst.name.text);
                let env = ConstEvalEnv {
                    index,
                    resolutions,
                    file: file_id,
                    const_values: &const_values,
                    shapes,
                    native: hir_file.native,
                };
                let default = eval_decl_default(
                    &cst.value,
                    cst.ptr.text_range(),
                    env,
                    names,
                    lambda_ctx,
                    diagnostics,
                );
                const_values.insert(id, default.clone());
                globals.push(lir::GlobalDef {
                    id,
                    name,
                    mutable: false,
                    default,
                    local: false,
                });
            }
        }
    }

    // Pass 2: evaluate variables (may reference constants).
    for &(file_id, hir_file) in files {
        // See the matching comment in the constants pass above.
        lambda_ctx.ids.set_path_prefix(hir::root_content_scope_path(
            lambda_ctx.file_paths.get(&file_id).map(String::as_str),
        ));
        for var in &hir_file.variables {
            if let Some(id) = lookup_global_or_diagnose(
                index,
                file_id,
                &var.name.text,
                SymbolKind::Variable,
                var.name.range,
                diagnostics,
            ) {
                let name = names.intern(&var.name.text);
                let env = ConstEvalEnv {
                    index,
                    resolutions,
                    file: file_id,
                    const_values: &const_values,
                    shapes,
                    native: hir_file.native,
                };
                let default = eval_decl_default(
                    &var.value,
                    var.ptr.text_range(),
                    env,
                    names,
                    lambda_ctx,
                    diagnostics,
                );
                globals.push(lir::GlobalDef {
                    id,
                    name,
                    mutable: true,
                    default,
                    local: var.is_local,
                });
            }
        }
    }

    globals
}

/// Fold one `CONST`/`VAR` declaration default — the body both of
/// [`collect_globals`]' passes share.
///
/// #692: a bare non-constant reference/call as the *whole* default (not
/// nested inside a collection/struct/fn literal, which do their own
/// E075/E076/E077 checks one level in) is a real compile error (`E083`),
/// never a silent `Null` fold. A lambda literal folds through
/// [`eval_const_lambda`] (#1774); everything else through
/// [`eval_const_expr`].
fn eval_decl_default(
    value: &hir::Expr,
    decl_range: TextRange,
    env: ConstEvalEnv<'_>,
    names: &mut NameTable,
    lambda_ctx: &mut GlobalLambdaCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    if !is_const_foldable_decl_default(value, env.index, env.resolutions, env.file) {
        diagnostics.push(Diagnostic {
            file: env.file,
            range: decl_range,
            message: DiagnosticCode::E083.title().to_string(),
            code: DiagnosticCode::E083,
        });
    }
    if let hir::Expr::Lambda(l) = value {
        eval_const_lambda(
            l,
            env.file,
            env.native,
            env.index,
            env.resolutions,
            names,
            lambda_ctx,
            diagnostics,
        )
    } else {
        eval_const_expr(value, env, diagnostics)
    }
}

/// Resources [`collect_globals`] needs only for a lambda-literal decl
/// default (issue #1774) — bundled so the common lambda-free case pays
/// nothing extra in the parameter list.
///
/// RULED 2026-08-01 (`docs/decision-log.md` #1774): a native `var`/`const`
/// may hold a fn value, including a lambda literal — not just the bare-name
/// reference #1862 already shipped. The gate that used to block this was
/// `is_const_foldable_decl_default`'s `Lambda` arm (raising `E083` in
/// [`collect_globals`] above) — *not* [`is_const_foldable_kind`], which
/// governs a lambda nested one level inside a collection/struct/`#fn` bound
/// arg and is deliberately left unchanged by this issue (out of scope: see
/// the PR description). The 2026-07-23 "flows-as-actors" direction homes a
/// *capturing* fn value to its creating flow to protect `#@local` privacy —
/// but that reasoning never reaches file scope, because a file-scope lambda
/// has no enclosing frame to capture from at all (no knot/stitch params, no
/// `~ temp` locals) — see [`eval_const_lambda`]'s doc for how that is made
/// mechanical, not just argued.
pub struct GlobalLambdaCtx<'a> {
    /// Shared across every file's decl pass — a lifted function's identity
    /// is content-derived, so one allocator (re-prefixed per file, same as
    /// [`super::lower_root_content_chunks`]) is enough for the whole
    /// project.
    pub ids: &'a mut IdAllocator,
    /// Lambda-lifted containers synthesized while folding decl defaults,
    /// destined to become siblings of the project's knots — the same
    /// placement [`lambda::lower_lambda`]'s own doc gives every lifted
    /// function, for the same reason (only ever entered through its own fn
    /// value).
    pub lifted: &'a mut Vec<lir::Container>,
    /// Per-file registered paths, for the `set_path_prefix` qualifier above.
    pub file_paths: &'a LookupMap<FileId, String>,
    /// Whole-program struct-shape data — `lower_lambda`'s body lowering
    /// needs it for the same TM-4c reasons any other body lowering does.
    pub structs: &'a StructCtx<'a>,
    /// Analyzer side-tables (UFCS/`or`-coalescing) — the real,
    /// whole-project verdict tables `brink-db`'s `lir_prelude_decls_query`
    /// builds from `ufcs_resolution_query`/`coalesce_types_query` (review
    /// finding on #1774: this used to be an unconditionally-empty pair,
    /// which risked a decl-default lambda body silently losing a resolved
    /// UFCS call's *meaning* — see [`super::expr::lower_ufcs_call`]'s doc
    /// for why that arm is not actually a silent miscompile, just a hard
    /// `E144` refusal).
    ///
    /// The `or`-coalescing half is genuinely complete now: `coalesce::
    /// resolve` already hand-recurses over `hir.variables`/`hir.constants`
    /// (issue #1764) specifically because their initializers sit outside
    /// `visit::visit`'s block-tree walk, so a decl-default lambda's chains
    /// are recorded in the real table this field now carries.
    ///
    /// The UFCS half is now complete too (issue #2096): `ufcs::resolve`
    /// drives its `HirVisitor`-shaped `UfcsVisitor` with
    /// `visit_with_decl_initializers` (mirroring `coalesce::resolve`'s own
    /// precedent, though by switching walkers rather than hand-recursing —
    /// see `ufcs::resolve`'s own doc for why the two passes' shapes led to
    /// different fixes), so a method call inside a decl-default lambda body
    /// is visited and gets a real verdict recorded, same as everywhere
    /// else. The old defensive `E144` refusal
    /// (`compile_path_native_ufcs_call_in_lambda_decl_default_is_e144`,
    /// since renamed/replaced) is now reachable only for a caller that
    /// genuinely never ran the UFCS pass at all — a receiver whose type
    /// stays undecidable even after visiting (an unannotated lambda param
    /// with no other constraint) still refuses, but with `E142` (D3:
    /// "annotate the receiver"), the ordinary diagnostic this pass has
    /// always used for that case — never a silent miscompile either way.
    pub tables: AnalyzerTables<'a>,
    /// The root container's `DefinitionId` — see
    /// [`super::context::root_definition_id`].
    pub root_id: brink_format::DefinitionId,
}

/// Fold a file-scope lambda literal used directly as a `VAR`/`CONST`
/// declaration default (issue #1774) — the lambda half of "a native
/// `var`/`const` may hold a fn value" ([`GlobalLambdaCtx`]'s doc has the
/// ruling).
///
/// Reuses [`lambda::lower_lambda`] verbatim rather than a parallel
/// mechanism: a file-scope lambda has no enclosing frame, so this hands it
/// an **empty** `TempMap`/`visible_temps` — every free name in the body
/// then misses `ctx.temp_slot`, and `lower_lambda`'s own `captured_locals`
/// contract is "everything that misses `ctx.temp_slot` is a legitimate
/// non-local" (a global `VAR`/`CONST` cell or a knot/function name), left
/// alone rather than captured. That is the 2026-08-01 ruling's safety
/// argument made mechanical rather than merely argued: the creation-site-
/// capture rule exists to keep a captured `#@local` cell from leaking
/// outside its home flow, and that can never be at stake here because
/// there is no capture *at all* — [`lir::Expr::MakeFnValue`]'s `bound` row
/// is always empty for a file-scope lambda, by construction of the empty
/// frame handed to it, not by a special case added here. Pinned by
/// `decls::tests::file_scope_lambda_cannot_capture_flow_local` — if a
/// future change ever gives file scope a real temp/param frame, that test
/// starts failing loudly instead of a capture silently smuggling
/// `#@local` state out through a fn value.
#[expect(
    clippy::too_many_arguments,
    reason = "mirrors GlobalLambdaCtx's own field count; a context struct already absorbed the growth"
)]
fn eval_const_lambda(
    l: &hir::LambdaExpr,
    file: FileId,
    native: bool,
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    names: &mut NameTable,
    lambda_ctx: &mut GlobalLambdaCtx<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let empty_temps = TempMap::new();
    let mut next_block_slot = 0u16;
    let mut ctx = super::make_ctx(
        file,
        native,
        resolutions,
        index,
        &empty_temps,
        names,
        lambda_ctx.ids,
        lambda_ctx.root_id,
        String::new(),
        false,
        &[],
        lambda_ctx.file_paths,
        &mut next_block_slot,
        diagnostics,
        lambda_ctx.structs,
        lambda_ctx.tables,
        lambda_ctx.lifted,
    );
    match lambda::lower_lambda(l, &mut ctx) {
        lir::Expr::MakeFnValue { target, bound } if bound.is_empty() => {
            lir::ConstValue::FnRef(target)
        }
        // Structurally unreachable at file scope today (see this fn's doc:
        // `temp_slot` can never hit with an empty `TempMap`/`visible_temps`,
        // so `bound` is always empty) — but "unreachable today" is exactly
        // the invariant a future change could break silently. Review
        // finding: falling back to a bare `Null` here would turn that break
        // into a silently-wrong global default (a captured `#@local`
        // snapshot that was never computed, quietly discarded) rather than
        // a refusal — this crate's own "flag silent data drops" rule. Emit
        // the same `E083` `is_const_foldable_decl_default` would have
        // raised for this position instead: a future break of the
        // empty-frame invariant becomes a loud compile refusal, never a
        // silent one.
        _ => {
            diagnostics.push(Diagnostic {
                file,
                range: l.ptr.text_range(),
                message: DiagnosticCode::E083.title().to_string(),
                code: DiagnosticCode::E083,
            });
            lir::ConstValue::Null
        }
    }
}

/// Collect list definitions, items, and corresponding global variables from HIR files.
///
/// Each LIST declaration creates:
/// 1. A `ListDef` (the enum type)
/// 2. `ListItemDef`s (the enum members)
/// 3. A mutable `GlobalDef` (the variable initialized to the active items)
///
/// The global variable uses the same hash as the `ListDef` but with a `GlobalVar` tag,
/// so `$03_abc` (`ListDef`) becomes `$02_abc` (`GlobalVar`).
pub fn collect_lists(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    names: &mut NameTable,
) -> (
    Vec<lir::ListDef>,
    Vec<lir::ListItemDef>,
    Vec<lir::GlobalDef>,
) {
    let mut lists = Vec::new();
    let mut items = Vec::new();
    let mut list_globals = Vec::new();

    for &(file_id, hir_file) in files {
        for list_decl in &hir_file.lists {
            let Some(list_id) =
                lookup_global(index, file_id, &list_decl.name.text, SymbolKind::List)
            else {
                continue;
            };
            let list_name = names.intern(&list_decl.name.text);

            let mut list_items = Vec::new();
            let mut active_item_ids = Vec::new();
            let mut next_ordinal = 1i32;

            for member in &list_decl.members {
                let ordinal = member.value.unwrap_or(next_ordinal);
                next_ordinal = ordinal + 1;

                let qualified = format!("{}.{}", list_decl.name.text, member.name.text);
                let item_name = names.intern(&qualified);

                if let Some(item_id) =
                    lookup_global(index, file_id, &qualified, SymbolKind::ListItem)
                {
                    list_items.push((item_name, ordinal));
                    items.push(lir::ListItemDef {
                        id: item_id,
                        name: item_name,
                        origin: list_id,
                        ordinal,
                    });
                    if member.is_active {
                        active_item_ids.push(item_id);
                    }
                }
            }

            lists.push(lir::ListDef {
                id: list_id,
                name: list_name,
                items: list_items,
            });

            // Create a mutable global variable for the list, initialized to its active items.
            let global_id = list_def_to_global_var(list_id);
            list_globals.push(lir::GlobalDef {
                id: global_id,
                name: list_name,
                mutable: true,
                default: lir::ConstValue::List {
                    items: active_item_ids,
                    origins: vec![list_id],
                },
                local: false,
            });
        }
    }

    (lists, items, list_globals)
}

/// Convert a `ListDef` id (`$03_xxx`) to its corresponding `GlobalVar` id (`$02_xxx`).
///
/// Same hash, different tag. This is used both when creating list globals and
/// when resolving references to list variables in expressions and assignments.
pub fn list_def_to_global_var(list_id: DefinitionId) -> DefinitionId {
    DefinitionId::new(DefinitionTag::GlobalVar, list_id.hash())
}

/// Collect external function declarations from HIR files.
///
/// `diagnostics` (issue #2262) is the same `decl_diagnostics` accumulator
/// `build_prelude_decls` threads through `collect_globals`/`build_shape_table`
/// — pushed to only when an `EXTERNAL`'s own self-declaration lookup below
/// comes back `None` (the `E184` backstop; see that code's own doc for the
/// exact, narrow drop condition, and issue #2240/`E181` for the identical
/// class one declaration kind over).
pub fn collect_externals(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    names: &mut NameTable,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<lir::ExternalDef> {
    let mut externals = Vec::new();

    for &(file_id, hir_file) in files {
        for ext in &hir_file.externals {
            if let Some(id) = lookup_global_or_diagnose(
                index,
                file_id,
                &ext.name.text,
                SymbolKind::External,
                ext.name.range,
                diagnostics,
            ) {
                let name = names.intern(&ext.name.text);
                // Look for an ink-defined function with the same name to use
                // as fallback — preferring one declared in this same file
                // (issue #2197), but a same-file entry is not the only
                // legitimate answer: an `extern foo` here and `=== function
                // foo` in an `INCLUDE`d sibling is a real, supported
                // cross-file fallback pair. What must never happen is
                // falling through to a **mounted `std…`** module's
                // same-named fallback with no import — e.g. the stdlib
                // mount's own `extern scene_entered` + `fn scene_entered`
                // pair binding to an unrelated project-declared `extern` of
                // the same name — which is exactly what `lookup_global`'s
                // own doc (the std exclusion in its unscoped fallback arm)
                // now rules out.
                let fallback = lookup_global(index, file_id, &ext.name.text, SymbolKind::Knot);
                externals.push(lir::ExternalDef {
                    id,
                    name,
                    arg_count: ext.param_count,
                    fallback,
                });
            }
        }
    }

    externals
}

/// Look up a project-wide global by name, preferring the entry declared in
/// `file`.
///
/// **File-scoped, not project-flat** (issue #2197): a bare `(name, kind)`
/// can have more than one coexisting candidate once M-2d
/// (`is_cross_declared_module_collision`) lets same-name definitions in
/// different *declared* modules coexist in `index.by_name` — which is
/// exactly what happens once `brink_environment`'s stdlib mount (#2080)
/// puts, say, a project's own `extern scene_entered` alongside
/// `std/conventions/screenplay.brink`'s own same-named extern. Almost every
/// call site here is a **self-declaration** lookup — "what id did the
/// analyzer already assign to the definition *this file itself* just
/// declared" — never a cross-file reference (those go through
/// `resolve.rs`'s `ImportScope`-aware `lookup_by_name` instead), so the
/// entry declared in `file` is always the right answer whenever one exists.
///
/// **Former exception, closed by issue #2249:** `ShapeTable::resolve`'s own
/// two production callers (a `VAR`/`CONST`/`temp` TM-2 struct annotation)
/// used to pass a **referrer** file that did not itself declare the
/// annotated shape — a genuine cross-file reference routed through this
/// file-scoped fallback rather than `resolve.rs`'s full-`Candidacy`
/// machinery, because there was no analyzer-recorded resolution to consume
/// (no HIR reference was walked for a `TypeExpr` annotation at all).
/// `symbols::project`'s walk now registers one (`RefKind::Type`), so both
/// callers — `structs::record_global_annotation`,
/// `context::LowerCtx::record_temp_annotation` — consume the analyzer's own
/// `resolve::resolve_type_ref` resolution directly and no longer reach this
/// function at all; a struct field's own nested-type lookup
/// (`build_shape_table`/`build_struct_shape_data`) made the identical move.
///
/// **Two other call sites here were audited against the same "does this
/// need a `RefKind`?" question and found NOT to fit it (issue #2249):**
/// `collect_externals`' fallback-fn lookup and `context::LowerCtx::
/// lookup_address_id` are both genuine **self-declaration** lookups in the
/// sense the paragraph above already describes — an `extern foo`'s
/// same-named `fn` fallback and a locally-declared label's own address are
/// never something the *user* wrote a textual reference to at this exact
/// call site; the pairing/existence check is inferred by the compiler from
/// two declarations' matching names (or a scope-qualified label's own
/// declaration), not resolved from a written path the way a divert target,
/// a variable read, or a type annotation is. There is no HIR reference to
/// register a `RefKind` for at either site — inventing a synthetic one with
/// no real source span would be exactly the "second scoped-resolution
/// implementation" issue #2249 warns against growing, not a reduction of
/// one. Both sites already get this function's std-exclusion for free, as
/// every other caller does; they are a genuinely different shape of gap
/// than the four `RefKind::Type` sites, not an oversight.
///
/// The unscoped fallback (no same-file entry) **excludes any candidate
/// declared in a mounted `std…` module** (review finding on #2197):
/// without this exclusion, a file that declares an `extern` with **no
/// fallback of its own** — a legal, common shape — would silently fall
/// through to whichever *other* file's same-named `fn` happens to sort
/// first in `by_name`, including the stdlib mount's own same-named
/// fallback (e.g. `std/conventions/screenplay.brink`'s `fn scene_entered`
/// binding to a project's unrelated `extern scene_entered(title, slug)`
/// with different params). That is exactly the "silently reach into std
/// with no import" class this issue's bare-name-visibility half closes
/// everywhere else (`resolve.rs`'s `lookup_by_name_direct`); this lookup is
/// a different call path into the same index and needed its own carve-out.
/// A **legitimate** cross-file fallback — `extern foo` in one file and
/// `=== function foo` in an `INCLUDE`d sibling, neither of them std — is
/// unaffected: only a `std…`-declared candidate is skipped, so
/// every pre-#2197 non-std caller keeps its byte-identical unscoped match.
///
/// #2251: generalized from the single-root `is_std_module` to
/// `is_reserved_root_module` — the exclusion was never really std-specific,
/// it excludes any mounted-library candidate, so it now checks the whole
/// `RESERVED_ROOTS` set. Behavior is unchanged today (one root mounted).
pub(super) fn lookup_global(
    index: &SymbolIndex,
    file: FileId,
    name: &str,
    kind: SymbolKind,
) -> Option<DefinitionId> {
    index.by_name.get(name).and_then(|ids| {
        ids.iter()
            .find(|&&id| {
                index
                    .symbols
                    .get(&id)
                    .is_some_and(|info| info.kind == kind && info.file == file)
            })
            .or_else(|| {
                ids.iter().find(|&&id| {
                    index.symbols.get(&id).is_some_and(|info| {
                        info.kind == kind
                            && !info.module.as_deref().is_some_and(is_reserved_root_module)
                    })
                })
            })
            .copied()
    })
}

/// [`lookup_global`], plus the `E184` non-suppressible backstop diagnostic
/// (issue #2262) when it comes back `None`.
///
/// This is [`crate::DiagnosticCode::E181`]'s own drop class — `structs::
/// build_shape_table`'s self-declaration lookup for `STRUCT` (issue #2240)
/// — recurring at every *other* declaration kind that self-resolves through
/// [`lookup_global`]: `CONST`/`VAR` (both in [`collect_globals`]) and
/// `EXTERNAL` ([`collect_externals`]). Same narrow condition as `E181`'s own
/// doc: the exact-file arm always matches a declaration against itself
/// *unless* `brink-analyzer` already dropped this HIR decl's own symbol
/// entry as a true intra-module duplicate, and even then the unscoped
/// fallback normally rescues the surviving sibling's id — this only fires
/// when *that* fallback also misses because every surviving same-name/
/// same-kind candidate is std-declared. Before `E184` existed, that
/// combination silently dropped the declaration from `PreludeDecls` with no
/// diagnostic at all (CLAUDE.md: "silent drops are always bugs until proven
/// otherwise") — see [`crate::DiagnosticCode::E184`]'s own doc for the full
/// argument, including why it is reachable today for `EXTERNAL` specifically
/// (a plain, no-`#@module` `EXTERNAL scene_entered(...)` colliding with the
/// std-mounted screenplay preset's own `extern scene_entered`).
///
/// Callers still receive `None` and skip the declaration exactly as before
/// — this makes the drop loud, it does not stop it from happening (the same
/// posture `E181` itself takes; see that code's own doc).
fn lookup_global_or_diagnose(
    index: &SymbolIndex,
    file: FileId,
    name: &str,
    kind: SymbolKind,
    range: rowan::TextRange,
    diagnostics: &mut Vec<Diagnostic>,
) -> Option<DefinitionId> {
    let id = lookup_global(index, file, name, kind);
    if id.is_none() {
        diagnostics.push(Diagnostic {
            file,
            range,
            message: DiagnosticCode::E184.title().to_string(),
            code: DiagnosticCode::E184,
        });
    }
    id
}

/// The read-only environment a declaration-default constant fold resolves
/// against. Bundled rather than threaded positionally: the fold is deeply
/// recursive (every collection/struct/`#fn` literal arm re-enters
/// [`eval_const_expr`] once per element), so passing five unchanging lookups
/// through each of those call sites buried the one thing that varies — the
/// expression — and cost nothing in clarity. `Copy`, so an arm can hand it
/// straight down; the diagnostic sink stays a separate `&mut` parameter,
/// which is the only thing the fold actually writes to.
#[derive(Clone, Copy)]
pub struct ConstEvalEnv<'a> {
    /// Whole-project symbol index — resolves a folded path to its kind.
    pub index: &'a SymbolIndex,
    /// Range → `DefinitionId` resolutions for the file being folded.
    pub resolutions: &'a ResolutionLookup,
    /// The file the default being folded was declared in.
    pub file: FileId,
    /// Already-folded `CONST` values, so `VAR x = SOME_CONST` resolves.
    /// Populated as [`collect_globals`]' first pass runs.
    pub const_values: &'a LookupMap<DefinitionId, lir::ConstValue>,
    /// The project's struct shapes — a construction literal default needs
    /// its shape's id and declaration field order (issue #1530).
    pub shapes: &'a ShapeTable,
    /// Whether the file the default being folded belongs to is the native
    /// (`.brink`) frontend — [`hir::HirFile::native`]. Mirrors the gate
    /// `lir::lower::expr::lower_path` applies to a bare function name in
    /// expression position (issue #1862): a native declaration initializer
    /// (`var f = double;`) must fold a bare reference to a function
    /// definition to [`lir::ConstValue::FnRef`], the same value the `#fn`
    /// literal arm below already produces, not the generic
    /// [`lir::ConstValue::DivertTarget`] every other symbol kind falls
    /// through to.
    pub native: bool,
}

/// Evaluate a compile-time constant expression.
#[expect(
    clippy::cast_possible_truncation,
    reason = "f64→f32 is intentional per ink spec"
)]
pub fn eval_const_expr(
    expr: &hir::Expr,
    env: ConstEvalEnv<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let ConstEvalEnv {
        index,
        resolutions,
        file,
        const_values,
        native,
        ..
    } = env;
    match expr {
        hir::Expr::Int(n) => lir::ConstValue::Int(*n),
        hir::Expr::Float(bits) => lir::ConstValue::Float(bits.to_f64() as f32),
        hir::Expr::Bool(b) => lir::ConstValue::Bool(*b),
        hir::Expr::String(s) => eval_const_string(s, file, diagnostics),
        hir::Expr::Prefix(hir::PrefixOp::Negate, inner) => {
            match eval_const_expr(inner, env, diagnostics) {
                lir::ConstValue::Int(n) => lir::ConstValue::Int(-n),
                lir::ConstValue::Float(f) => lir::ConstValue::Float(-f),
                _ => lir::ConstValue::Null,
            }
        }
        hir::Expr::Prefix(hir::PrefixOp::Not, inner) => {
            match eval_const_expr(inner, env, diagnostics) {
                lir::ConstValue::Bool(b) => lir::ConstValue::Bool(!b),
                lir::ConstValue::Int(n) => lir::ConstValue::Bool(n == 0),
                lir::ConstValue::Float(f) => lir::ConstValue::Bool(f == 0.0),
                lir::ConstValue::Null => lir::ConstValue::Bool(true),
                _ => lir::ConstValue::Null,
            }
        }
        hir::Expr::Infix(ie) => {
            let l = eval_const_expr(&ie.lhs, env, diagnostics);
            let r = eval_const_expr(&ie.rhs, env, diagnostics);
            eval_const_infix(&l, ie.op, &r)
        }
        hir::Expr::Path(path) => {
            fold_path_ref(path, file, resolutions, index, const_values, native)
        }
        hir::Expr::DivertTarget(path) => {
            if let Some(id) = resolutions.resolve(file, path.range) {
                lir::ConstValue::DivertTarget(id)
            } else {
                lir::ConstValue::Null
            }
        }
        hir::Expr::ListLiteral(paths) => {
            let mut items = Vec::new();
            let mut origins = Vec::new();
            for path in paths {
                if let Some(id) = resolutions.resolve(file, path.range)
                    && let Some(info) = index.symbols.get(&id)
                {
                    if info.kind == SymbolKind::ListItem {
                        items.push(id);
                        // Derive the origin list from the item's qualified name.
                        if let Some(dot) = info.name.rfind('.') {
                            let list_name = &info.name[..dot];
                            if let Some(list_ids) = index.by_name.get(list_name) {
                                for &list_id in list_ids {
                                    if index
                                        .symbols
                                        .get(&list_id)
                                        .is_some_and(|s| s.kind == SymbolKind::List)
                                        && !origins.contains(&list_id)
                                    {
                                        origins.push(list_id);
                                    }
                                }
                            }
                        }
                    } else if info.kind == SymbolKind::List {
                        origins.push(id);
                    }
                }
            }
            lir::ConstValue::List { items, origins }
        }
        // #673: `VAR`/`CONST arr = #[…]` — see `eval_const_array_literal`'s
        // doc.
        hir::Expr::ArrayLiteral(arr) => eval_const_array_literal(arr, env, diagnostics),
        // #673: `VAR`/`CONST m = #{…}` — see `eval_const_map_literal`'s doc.
        hir::Expr::MapLiteral(map) => eval_const_map_literal(map, env, diagnostics),
        // #673/#1530: `VAR`/`CONST p = Name#{…}` — see
        // `eval_const_struct_literal`'s doc.
        hir::Expr::StructLiteral(sl) => eval_const_struct_literal(sl, env, diagnostics),
        // T1c-2: `VAR f = #fn(…)` — see `eval_const_fn_literal`'s doc.
        hir::Expr::FnLiteral(fl) => eval_const_fn_literal(fl, env, diagnostics),
        _ => lir::ConstValue::Null,
    }
}

/// Fold a bare `Path` reference used as (or nested inside) a declaration
/// default — [`eval_const_expr`]'s `Path` arm, split out to keep that match
/// under clippy's line budget.
///
/// Native bare-name fn value in declaration-initializer position (RULED
/// 2026-08-01, `docs/t1c-spec.md` §2a, issue #1862): `var f = double;` must
/// fold the same way the zero-bound `#fn(double)` literal arm
/// (`eval_const_fn_literal`) does — mirrors the gate
/// `lir::lower::expr::lower_path` applies in expression position, so a
/// native declaration default and a native expression-position reference to
/// the same function never disagree on what value they produce.
fn fold_path_ref(
    path: &hir::Path,
    file: FileId,
    resolutions: &ResolutionLookup,
    index: &SymbolIndex,
    const_values: &LookupMap<DefinitionId, lir::ConstValue>,
    native: bool,
) -> lir::ConstValue {
    let Some(id) = resolutions.resolve(file, path.range) else {
        return lir::ConstValue::Null;
    };
    let Some(info) = index.symbols.get(&id) else {
        return lir::ConstValue::Null;
    };
    if native && info.is_function_definition() {
        return lir::ConstValue::FnRef(id);
    }
    match info.kind {
        SymbolKind::ListItem => lir::ConstValue::List {
            items: vec![id],
            origins: vec![],
        },
        SymbolKind::Constant => const_values
            .get(&id)
            .cloned()
            .unwrap_or(lir::ConstValue::Null),
        SymbolKind::Variable => lir::ConstValue::Null,
        _ => lir::ConstValue::DivertTarget(id),
    }
}

/// #673: constant-fold a literal-only array default into a real
/// `ConstValue::Array`, exactly the representation `build_globals`
/// (brink-codegen-inkb) already materializes into `Value::array` for any
/// global default; this is wiring `decls` into a codegen path that already
/// exists for expression-position array literals (`expr::lower_array_
/// literal`), not new collection semantics. Elements recurse through
/// `eval_const_expr` itself (not `expr::try_const_fold`) so a constant
/// reference nested inside the array (`#[SOME_CONST, 2]`) resolves via
/// `const_values`, same as a bare scalar default would. An element whose
/// source expression kind can never constant-fold is a real compile error
/// (`E077`), never a silently-`Null` element — see
/// [`is_const_foldable_kind`].
fn eval_const_array_literal(
    arr: &hir::ArrayLiteral,
    env: ConstEvalEnv<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let ConstEvalEnv {
        index,
        resolutions,
        file,
        ..
    } = env;
    let mut items = Vec::with_capacity(arr.elements.len());
    for e in &arr.elements {
        if !is_const_foldable_kind(e, index, resolutions, file) {
            diagnostics.push(Diagnostic {
                file,
                range: arr.ptr.text_range(),
                message: DiagnosticCode::E077.title().to_string(),
                code: DiagnosticCode::E077,
            });
        }
        items.push(eval_const_expr(e, env, diagnostics));
    }
    lir::ConstValue::Array(items)
}

/// #673: same constant-folding story as [`eval_const_array_literal`], for
/// `ConstValue::Map`. A key that doesn't fold into the ratified map-key
/// domain (int/string/bool) is a real compile error (`E076`), not a silent
/// drop of that entry — unlike `expr::lower_map_literal`'s expression-
/// position twin, a declaration default has no `MapNew` runtime-
/// construction step left to fault at, so this is the compile-time
/// equivalent of that runtime fault. A *value* whose source expression
/// kind can never constant-fold is likewise a real compile error (`E077`),
/// never a silently-`Null` entry — see [`is_const_foldable_kind`]. (A
/// never-constant *key* already lands in the `E076` arm: it folds to
/// `Null`, which is outside the scalar key domain.)
fn eval_const_map_literal(
    map: &hir::MapLiteral,
    env: ConstEvalEnv<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let ConstEvalEnv {
        index,
        resolutions,
        file,
        ..
    } = env;
    let mut entries = Vec::with_capacity(map.entries.len());
    for (k, v) in &map.entries {
        if !is_const_foldable_kind(v, index, resolutions, file) {
            diagnostics.push(Diagnostic {
                file,
                range: map.ptr.text_range(),
                message: DiagnosticCode::E077.title().to_string(),
                code: DiagnosticCode::E077,
            });
        }
        let key_val = eval_const_expr(k, env, diagnostics);
        let value = eval_const_expr(v, env, diagnostics);
        match const_value_to_map_key(key_val) {
            Some(key) => entries.push((key, value)),
            None => {
                diagnostics.push(Diagnostic {
                    file,
                    range: map.ptr.text_range(),
                    message: DiagnosticCode::E076.title().to_string(),
                    code: DiagnosticCode::E076,
                });
            }
        }
    }
    lir::ConstValue::Map(entries)
}

/// #1530: `VAR`/`CONST p = Name#{…}` (brink dialect) / `Name { … }`
/// (native) — constant-fold a well-formed construction literal into a real
/// [`lir::ConstValue::Record`], the same way [`eval_const_array_literal`]
/// and [`eval_const_map_literal`] already fold their own literals.
///
/// #673 left this arm as an unconditional `E075` refusal because
/// `ConstValue` had no record-carrying variant, which made a **struct-typed
/// durable global unspellable**: `docs/t1e-spec.md` §2 requires a
/// projection's root to be a durable cell, so no real source could reach
/// the T1e projection-receiver path end to end, and `E143`'s own remediation
/// advice ("bind the receiver to a durable cell") pointed at something the
/// language could not express. The variant is now the same shape-ordered
/// flat field vector `Value::Record` and `RecordNew` use, so codegen's
/// `const_to_value` materializes it with no reordering.
///
/// Unlike the expression-position twin ([`super::expr`]'s
/// `lower_struct_literal`) there is no `RecordNew` runtime construction step
/// left for a malformed literal to fault at — a declaration default is baked
/// into `StoryData`. So the two malformed cases stay real compile errors,
/// exactly the compile-time-equivalent-of-the-runtime-fault posture
/// [`eval_const_map_literal`]'s `E076` already takes for a bad map key:
///
/// - an **unresolved shape name** reports `E073`, the same code the
///   expression-position path's `reject_unresolved_struct_shape` uses;
/// - a **missing or undeclared field** reports the (now narrowed) `E075`.
///   Under `types = strict` `brink-analyzer`'s `structs::check` reports the
///   more precise `E069`/`E070` for the same literal, but that check is
///   strict-only by ratified policy, so this is the policy-independent
///   backstop that keeps a `gradual` project from baking a half-built
///   record into its story data.
///
/// A field *value* whose source expression kind can never constant-fold is
/// the usual `E077`, identical to an array element or map value one level
/// in. A duplicate field name is `brink-analyzer`'s policy-independent
/// `E084`; last-wins placement here matches `lower_struct_literal`'s.
fn eval_const_struct_literal(
    sl: &hir::StructLiteral,
    env: ConstEvalEnv<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let ConstEvalEnv {
        index,
        resolutions,
        file,
        shapes,
        ..
    } = env;
    // Issue #2246: `sl.shape` is a `RefKind::Struct` reference — a VAR/CONST
    // decl default is walked the same as any other expression
    // (`symbols::project::project_manifest` walks `v.value`/`c.value`), so
    // the analyzer already resolved it against `file`'s module scope
    // (`resolve::resolve_struct_ref`, full `Candidacy`). Consume that
    // recorded resolution directly rather than re-deriving it through
    // `ShapeTable::resolve`'s own narrower file-scoped fallback — see
    // `expr::lower_struct_literal`'s matching doc, the expression-position
    // twin of this decl-default fold.
    let shape = resolutions
        .resolve(file, sl.shape.range)
        .and_then(|id| shapes.get_by_def(id));
    let Some(shape) = shape else {
        diagnostics.push(Diagnostic {
            file,
            range: sl.ptr.text_range(),
            message: DiagnosticCode::E073.title().to_string(),
            code: DiagnosticCode::E073,
        });
        return lir::ConstValue::Null;
    };

    let mut placed: Vec<Option<lir::ConstValue>> = vec![None; shape.fields.len()];
    let mut has_extra = false;
    for (name, value) in &sl.fields {
        if !is_const_foldable_kind(value, index, resolutions, file) {
            diagnostics.push(Diagnostic {
                file,
                range: sl.ptr.text_range(),
                message: DiagnosticCode::E077.title().to_string(),
                code: DiagnosticCode::E077,
            });
        }
        // Every supplied initializer is evaluated, in source order, even one
        // whose name the shape doesn't declare — a nested literal's own
        // `E073`/`E076`/`E077` must still be reported rather than skipped
        // because an unrelated sibling field was misspelled.
        let folded = eval_const_expr(value, env, diagnostics);
        match shape.field(&name.text) {
            Some((offset, _)) => {
                if let Some(slot) = placed.get_mut(offset as usize) {
                    *slot = Some(folded);
                }
            }
            None => has_extra = true,
        }
    }

    if has_extra || placed.iter().any(Option::is_none) {
        diagnostics.push(Diagnostic {
            file,
            range: sl.ptr.text_range(),
            message: DiagnosticCode::E075.title().to_string(),
            code: DiagnosticCode::E075,
        });
        return lir::ConstValue::Null;
    }

    lir::ConstValue::Record {
        shape_id: shape.id,
        // The guard above just proved every slot is `Some`; `map_or_else`
        // rather than `unwrap` anyway (denied in production code), so a
        // future refactor that weakens that proof degrades to a `Null` field
        // instead of a panic — the same posture `lower_struct_literal` takes.
        fields: placed
            .into_iter()
            .map(|v| v.unwrap_or(lir::ConstValue::Null))
            .collect(),
    }
}

/// T1c-2: `VAR f = #fn(name, args…)` — bake a function value into the
/// declaration default (docs/t1c-spec.md §2/§6). This is the declaration-
/// default half of the T1c-1 E052 fence removal (the expression-position half
/// is `expr::lower_fn_literal`): a zero-bound `#fn(name)` folds to
/// [`lir::ConstValue::FnRef`]; a bound `#fn(name, args…)` folds to
/// [`lir::ConstValue::Closure`], with each `ref` param bound to a durable
/// global cell and each `val` param to a compile-time snapshot. Creation-site
/// validity (E079/E080/E081) is `brink-analyzer`'s job; an unresolved target
/// leaves the analyzer's own diagnostic to stand and folds to `Null`.
fn eval_const_fn_literal(
    fl: &hir::FnLiteral,
    env: ConstEvalEnv<'_>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let ConstEvalEnv {
        index,
        resolutions,
        file,
        ..
    } = env;
    let Some(target_id) = resolutions.resolve(file, fl.target.range) else {
        return lir::ConstValue::Null;
    };
    let Some(target_info) = index.symbols.get(&target_id) else {
        return lir::ConstValue::Null;
    };
    if fl.args.is_empty() {
        return lir::ConstValue::FnRef(target_id);
    }
    let mut bound = Vec::with_capacity(fl.args.len());
    for (i, arg) in fl.args.iter().enumerate() {
        let param = target_info.params.get(i);
        let name = param.map_or_else(String::new, |p| p.name.clone());
        let is_ref = param.is_some_and(|p| p.is_ref);
        if is_ref {
            // A `ref` bound arg must name a durable global cell (analyzer E080
            // guaranteed this under `dialect = brink`); resolve it to the cell
            // id so codegen bakes a `VariablePointer`. An unresolved cell (the
            // arg isn't a path, or the path doesn't resolve) means the
            // analyzer's own diagnostic already stands for this site — fold
            // the whole literal to `Null` rather than sentinel-binding the
            // function's own `target_id` as a fake cell (T1c-2 rider, #721).
            let Some(cell) = (match arg {
                hir::Expr::Path(p) => resolutions.resolve(file, p.range),
                _ => None,
            }) else {
                return lir::ConstValue::Null;
            };
            bound.push(lir::ConstClosureEntry::Ref { name, cell });
        } else {
            // #743: a `val` bound arg is exactly an array-element/map-value
            // position one level inside the `#fn(…)` literal — same E077
            // non-constant-kind check, so a bare `VAR` reference or a
            // never-foldable kind (call, index, field access, …) bound by
            // value no longer silently folds to `Null` with zero diagnostic.
            if !is_const_foldable_kind(arg, index, resolutions, file) {
                diagnostics.push(Diagnostic {
                    file,
                    range: fl.ptr.text_range(),
                    message: DiagnosticCode::E077.title().to_string(),
                    code: DiagnosticCode::E077,
                });
            }
            let value = eval_const_expr(arg, env, diagnostics);
            bound.push(lir::ConstClosureEntry::Val { name, value });
        }
    }
    lir::ConstValue::Closure {
        target: target_id,
        env: bound,
    }
}

/// #679 review (#743 closed the `Path`-to-`Variable` residue): can this
/// source expression *kind* ever constant-fold in a declaration default?
/// `false` means `eval_const_expr` is guaranteed to land in a `Null`
/// fallthrough — a function call, postfix indexing, field access, or
/// `++`/`--` has no compile-time evaluation and no runtime construction step
/// left to defer to, so an array element / map value / struct field / `#fn`
/// bound `val` arg of that kind is a real compile error (`E077`), #673's
/// silent-`Null` bug one level down inside the literal.
///
/// Deliberately keyed off the expression kind, never the folded result:
///
/// - `Expr::Null` is HIR error recovery (or a missing initializer), not
///   author-writable source — folding it to `Null` is correct and already
///   diagnosed upstream, so it must not double-report here.
/// - `Expr::Path` constness depends on what the path resolves to — same
///   resolution `is_const_foldable_decl_default` does one level up: a bare
///   reference to another `VAR` (`SymbolKind::Variable`) is never a
///   compile-time constant one level in either (#743; #679's scope notes
///   originally left this nested case unchanged, tracked separately and
///   now closed), while a reference to a `CONST`/list item/knot/stitch/
///   function still folds for real. An unresolved path leaves the
///   analyzer's own unresolved-reference diagnostic (E024/E025) to stand —
///   not double-reported here, so it stays foldable.
/// - Collection/struct literals recurse through their own `eval_const_*`
///   arms, which do their own per-element checking (`E075`/`E076`/`E077`).
///
/// Exhaustive on purpose: a new `hir::Expr` variant must decide its
/// declaration-default story here instead of silently inheriting `true`.
fn is_const_foldable_kind(
    expr: &hir::Expr,
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    file: FileId,
) -> bool {
    match expr {
        hir::Expr::Int(_)
        | hir::Expr::Float(_)
        | hir::Expr::Bool(_)
        | hir::Expr::String(_)
        | hir::Expr::Null
        | hir::Expr::DivertTarget(_)
        | hir::Expr::ListLiteral(_)
        | hir::Expr::ArrayLiteral(_)
        | hir::Expr::MapLiteral(_)
        | hir::Expr::StructLiteral(_) => true,
        hir::Expr::Path(path) => !matches!(
            resolutions
                .resolve(file, path.range)
                .and_then(|id| index.symbols.get(&id)),
            Some(info) if info.kind == SymbolKind::Variable
        ),
        hir::Expr::Prefix(_, inner) => is_const_foldable_kind(inner, index, resolutions, file),
        hir::Expr::Infix(ie) => {
            is_const_foldable_kind(&ie.lhs, index, resolutions, file)
                && is_const_foldable_kind(&ie.rhs, index, resolutions, file)
        }
        hir::Expr::Postfix(..)
        | hir::Expr::Call(..)
        | hir::Expr::Index(_)
        | hir::Expr::FieldAccess(_)
        // T1c-1: `#fn(…)` never constant-folds — as a declaration default it
        // is already a targeted E052 at `eval_const_expr`'s own arm; as an
        // array/map element it reports the standard E077.
        | hir::Expr::FnLiteral(_)
        // T1e-1: `ref lvalue-path` never constant-folds — it isn't a legal
        // ref-argument position at all here (`brink-analyzer`'s own E097
        // already covers it), and even where legal it has no compile-time
        // value.
        | hir::Expr::RefArg(_)
        // NS-A5 v1: range literals don't constant-fold into declaration
        // defaults — ranges are runtime values built by `RangeMake*`
        // (`~ temp r = 1..=6` / assignment into a VAR both work); the
        // "CONST refs fold" leg of the F7 evidence rule is about a range's
        // *bounds* referencing CONSTs, not about range-valued CONSTs.
        // Wiring `ConstValue::Range` through the decl-default pipeline is
        // a follow-up if authoring demand appears.
        | hir::Expr::Range(_)
        // Lambdas (issue #1685) never constant-fold *nested one level in* —
        // a collection/struct-field/`#fn`-bound-`val` element position. This
        // deliberately stays `false` even though a lambda literal used as
        // the *whole* top-level default now folds
        // (`is_const_foldable_decl_default`'s own arm, issue #1774): the
        // 2026-08-01 ruling that lifted the top-level gate is about "a
        // native `var`/`const` may hold a fn value", not about a lambda
        // appearing inside a collection that itself becomes a global's
        // default (`#[|x| x, 2]`) — genuinely reachable, but out of this
        // issue's scope; see the #1774 PR description.
        | hir::Expr::Lambda(_)
        // Internal-only (issue #1839) — never surface syntax, so it can
        // never appear as a declaration default in the first place; `false`
        // for the same reason as every other non-literal shape here.
        | hir::Expr::Fragment(_) => false,
    }
}

/// #692: can this source expression *kind* ever be a compile-time constant
/// at the top level of a `VAR`/`CONST` declaration default (the whole
/// default, not an element nested inside a collection/struct/fn literal)?
/// Sibling check to [`is_const_foldable_kind`], which governs collection
/// *elements* one level in; this one governs the position `eval_const_expr`
/// itself is called from directly (`collect_globals`'s two call sites).
///
/// The one place this genuinely differs from `is_const_foldable_kind`:
/// `Expr::Path` is resolved here, because at this position (unlike a
/// collection element, #679 scope notes) the issue this fixes (#692) is
/// exactly a bare reference to another `VAR` — `SymbolKind::Variable` is
/// never a compile-time constant (global mutable state doesn't exist yet
/// during the const-fold pass) — while a reference to a `CONST`/list
/// item/knot/stitch/function *is*, same as `eval_const_expr`'s own arm
/// already treats it. An unresolved path leaves the analyzer's own
/// unresolved-reference diagnostic (E024/E025) to stand — not double-
/// reported here.
///
/// `Expr::Prefix`/`Expr::Infix` recurse (a wrapped non-constant, e.g.
/// `VAR x = -f()` or `VAR x = 1 + someVar`, is still a bare top-level
/// default, not a collection element). Collection/struct/fn literals do
/// their own per-element checking one level in (`E075`/`E076`/`E077`), and
/// a bare `#fn(…)` *is* a supported constant default (T1c-2) — both report
/// `true` here to avoid double-reporting.
///
/// Exhaustive on purpose: a new `hir::Expr` variant must decide its
/// top-level declaration-default story here instead of silently inheriting
/// `true`.
fn is_const_foldable_decl_default(
    expr: &hir::Expr,
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    file: FileId,
) -> bool {
    match expr {
        hir::Expr::Int(_)
        | hir::Expr::Float(_)
        | hir::Expr::Bool(_)
        | hir::Expr::String(_)
        | hir::Expr::Null
        | hir::Expr::DivertTarget(_)
        | hir::Expr::ListLiteral(_)
        | hir::Expr::ArrayLiteral(_)
        | hir::Expr::MapLiteral(_)
        | hir::Expr::StructLiteral(_)
        | hir::Expr::FnLiteral(_)
        // A lambda literal as the *whole* top-level default (`VAR f = |x|
        // x + 1;`) folds — RULED 2026-08-01, `docs/decision-log.md` #1774:
        // a native `var`/`const` may hold a fn value, both a bare-name
        // reference (already legal, #1862) and a lambda literal. This is
        // the E083 gate the ruling lifts; `collect_globals` special-cases
        // `Expr::Lambda` before calling `eval_const_expr` (see
        // `eval_const_lambda`) rather than folding it through the shared
        // recursive path, since `eval_const_expr` has no Lambda arm of its
        // own — a lambda nested one level in (a collection element, a
        // struct field, a `#fn` bound `val` arg) stays ungated by this
        // arm but still unsupported by `eval_const_expr`, so it still folds
        // to `Null` behind its own `E077`/`E076` diagnostic
        // (`is_const_foldable_kind`'s Lambda arm, deliberately unchanged —
        // out of this issue's scope).
        | hir::Expr::Lambda(_) => true,
        hir::Expr::Path(path) => !matches!(
            resolutions
                .resolve(file, path.range)
                .and_then(|id| index.symbols.get(&id)),
            Some(info) if info.kind == SymbolKind::Variable
        ),
        hir::Expr::Prefix(_, inner) => {
            is_const_foldable_decl_default(inner, index, resolutions, file)
        }
        hir::Expr::Infix(ie) => {
            is_const_foldable_decl_default(&ie.lhs, index, resolutions, file)
                && is_const_foldable_decl_default(&ie.rhs, index, resolutions, file)
        }
        hir::Expr::Postfix(..)
        | hir::Expr::Call(..)
        | hir::Expr::Index(_)
        | hir::Expr::FieldAccess(_)
        // T1e-1: `ref lvalue-path` is never a legal top-level declaration
        // default (it's a ref-argument-only construct — `brink-analyzer`'s
        // own E097 covers the standalone-position case) and has no
        // compile-time value even where legal.
        | hir::Expr::RefArg(_)
        // NS-A5 v1: see `is_const_foldable_kind`'s Range arm.
        | hir::Expr::Range(_)
        // Internal-only (issue #1839): see `is_const_foldable_kind`'s
        // Fragment arm.
        | hir::Expr::Fragment(_) => false,
    }
}

/// Evaluate a compile-time string, emitting E030 if interpolation is present.
fn eval_const_string(
    s: &hir::StringExpr,
    file: FileId,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let mut has_interpolation = false;
    let text: String = s
        .parts
        .iter()
        .filter_map(|p| match p {
            hir::StringPart::Literal(t) => Some(t.as_str()),
            hir::StringPart::Interpolation(_) => {
                has_interpolation = true;
                None
            }
        })
        .collect();
    if has_interpolation {
        diagnostics.push(Diagnostic {
            file,
            range: rowan::TextRange::default(),
            message: DiagnosticCode::E030.title().to_string(),
            code: DiagnosticCode::E030,
        });
    }
    lir::ConstValue::String(text)
}

/// Evaluate a binary operation on two const values.
fn eval_const_infix(
    lhs: &lir::ConstValue,
    op: hir::InfixOp,
    rhs: &lir::ConstValue,
) -> lir::ConstValue {
    use hir::InfixOp;
    use lir::ConstValue;

    // List operations are not const-foldable.
    if matches!(op, InfixOp::Has | InfixOp::HasNot | InfixOp::Intersect) {
        return ConstValue::Null;
    }

    // String concatenation: Add on String×String → String.
    if op == InfixOp::Add
        && let (ConstValue::String(a), ConstValue::String(b)) = (lhs, rhs)
    {
        return ConstValue::String(format!("{a}{b}"));
    }

    // Promote to float if either side is float.
    match (lhs, rhs) {
        (ConstValue::Int(a), ConstValue::Int(b)) => eval_int_infix(*a, op, *b),
        (ConstValue::Float(a), ConstValue::Float(b)) => {
            eval_float_infix(f64::from(*a), op, f64::from(*b))
        }
        (ConstValue::Int(a), ConstValue::Float(b)) => {
            eval_float_infix(f64::from(*a), op, f64::from(*b))
        }
        (ConstValue::Float(a), ConstValue::Int(b)) => {
            eval_float_infix(f64::from(*a), op, f64::from(*b))
        }
        (ConstValue::Bool(a), ConstValue::Bool(b)) => eval_bool_infix(*a, op, *b),
        _ => ConstValue::Null,
    }
}

fn eval_int_infix(a: i32, op: hir::InfixOp, b: i32) -> lir::ConstValue {
    use hir::InfixOp;
    use lir::ConstValue;

    match op {
        InfixOp::Add => ConstValue::Int(a.wrapping_add(b)),
        InfixOp::Sub => ConstValue::Int(a.wrapping_sub(b)),
        InfixOp::Mul => ConstValue::Int(a.wrapping_mul(b)),
        InfixOp::Div => {
            if b == 0 {
                ConstValue::Null
            } else {
                ConstValue::Int(a.wrapping_div(b))
            }
        }
        InfixOp::Mod => {
            if b == 0 {
                ConstValue::Null
            } else {
                ConstValue::Int(a.wrapping_rem(b))
            }
        }
        InfixOp::Eq => ConstValue::Bool(a == b),
        InfixOp::NotEq => ConstValue::Bool(a != b),
        InfixOp::Lt => ConstValue::Bool(a < b),
        InfixOp::Gt => ConstValue::Bool(a > b),
        InfixOp::LtEq => ConstValue::Bool(a <= b),
        InfixOp::GtEq => ConstValue::Bool(a >= b),
        InfixOp::And => ConstValue::Bool(a != 0 && b != 0),
        InfixOp::Or => ConstValue::Bool(a != 0 || b != 0),
        _ => ConstValue::Null,
    }
}

#[expect(
    clippy::cast_possible_truncation,
    clippy::float_cmp,
    reason = "f64→f32 is intentional per ink spec; ink uses exact float comparison"
)]
fn eval_float_infix(a: f64, op: hir::InfixOp, b: f64) -> lir::ConstValue {
    use hir::InfixOp;
    use lir::ConstValue;

    match op {
        InfixOp::Add => ConstValue::Float((a + b) as f32),
        InfixOp::Sub => ConstValue::Float((a - b) as f32),
        InfixOp::Mul => ConstValue::Float((a * b) as f32),
        InfixOp::Div => ConstValue::Float((a / b) as f32),
        InfixOp::Mod => ConstValue::Float((a % b) as f32),
        InfixOp::Eq => ConstValue::Bool(a == b),
        InfixOp::NotEq => ConstValue::Bool(a != b),
        InfixOp::Lt => ConstValue::Bool(a < b),
        InfixOp::Gt => ConstValue::Bool(a > b),
        InfixOp::LtEq => ConstValue::Bool(a <= b),
        InfixOp::GtEq => ConstValue::Bool(a >= b),
        InfixOp::And => ConstValue::Bool(a != 0.0 && b != 0.0),
        InfixOp::Or => ConstValue::Bool(a != 0.0 || b != 0.0),
        _ => ConstValue::Null,
    }
}

fn eval_bool_infix(a: bool, op: hir::InfixOp, b: bool) -> lir::ConstValue {
    use hir::InfixOp;
    use lir::ConstValue;

    match op {
        InfixOp::And => ConstValue::Bool(a && b),
        InfixOp::Or => ConstValue::Bool(a || b),
        InfixOp::Eq => ConstValue::Bool(a == b),
        InfixOp::NotEq => ConstValue::Bool(a != b),
        _ => ConstValue::Null,
    }
}

#[cfg(test)]
mod tests {
    use brink_format::DefinitionTag;
    use rowan::TextRange;

    use super::*;
    use crate::symbols::{SymbolInfo, Visibility};

    /// Insert one bare-name `SymbolInfo` into `index`, in `module` (`None`
    /// for the undeclared/legacy world), owned by `file`.
    fn insert(
        index: &mut SymbolIndex,
        id: DefinitionId,
        file: FileId,
        name: &str,
        kind: SymbolKind,
        module: Option<&str>,
    ) {
        index.symbols.insert(
            id,
            SymbolInfo {
                kind,
                file,
                range: TextRange::default(),
                id,
                name: name.to_string(),
                params: Vec::new(),
                detail: None,
                scope: None,
                param_detail: None,
                module: module.map(str::to_string),
                visibility: Visibility::Public,
            },
        );
        index.by_name.entry(name.to_string()).or_default().push(id);
    }

    /// Issue #2197 review finding: a project file declaring `extern foo`
    /// with **no fallback of its own** must not have `collect_externals`'
    /// fallback lookup silently fall through to a mounted `std…`
    /// module's own same-named `fn foo` — the exact "silently reach into
    /// std with no import" class the bare-name-visibility half of #2197
    /// closes at the `resolve.rs` layer, reached here through a completely
    /// different call path (`lookup_global`'s unscoped `.or_else`).
    #[test]
    fn extern_only_no_fallback_does_not_bind_a_std_mounts_fn_of_the_same_name() {
        let mut index = SymbolIndex::default();
        let project_file = FileId(0);
        let std_file = FileId(1);

        // The project's own `extern scene_entered(...)` — no fallback fn
        // anywhere in the project.
        let project_extern = DefinitionId::new(DefinitionTag::ExternalFn, 1);
        insert(
            &mut index,
            project_extern,
            project_file,
            "scene_entered",
            SymbolKind::External,
            Some("story::story"),
        );

        // The stdlib mount declares its OWN extern + fallback pair under
        // the same bare name, in a `std…` module.
        let std_extern = DefinitionId::new(DefinitionTag::ExternalFn, 2);
        insert(
            &mut index,
            std_extern,
            std_file,
            "scene_entered",
            SymbolKind::External,
            Some("std::conventions::screenplay"),
        );
        let std_fallback = DefinitionId::new(DefinitionTag::Address, 3);
        insert(
            &mut index,
            std_fallback,
            std_file,
            "scene_entered",
            SymbolKind::Knot,
            Some("std::conventions::screenplay"),
        );

        // The project's own extern still resolves (same-file entry exists).
        assert_eq!(
            lookup_global(&index, project_file, "scene_entered", SymbolKind::External),
            Some(project_extern)
        );

        // But the fallback lookup — no same-file `Knot` named
        // `scene_entered` exists in the project — must NOT fall through to
        // the std mount's `fn scene_entered`. It must resolve to nothing.
        assert_eq!(
            lookup_global(&index, project_file, "scene_entered", SymbolKind::Knot),
            None,
            "an extern with no project-side fallback must not silently bind \
             a same-named fn from a mounted std:: module"
        );
    }

    /// The non-std twin of the test above: a **legitimate** cross-file
    /// fallback (an `extern` in one file, its `=== function ===` fallback in
    /// an `INCLUDE`d sibling, neither of them std) must still resolve via
    /// the unscoped fallback exactly as before #2197 — the std exclusion
    /// must not overreach into ordinary cross-file cases.
    #[test]
    fn extern_only_no_fallback_still_finds_a_non_std_sibling_fallback() {
        let mut index = SymbolIndex::default();
        let project_file = FileId(0);
        let sibling_file = FileId(1);

        let project_extern = DefinitionId::new(DefinitionTag::ExternalFn, 10);
        insert(
            &mut index,
            project_extern,
            project_file,
            "narrate",
            SymbolKind::External,
            None,
        );
        let sibling_fallback = DefinitionId::new(DefinitionTag::Address, 11);
        insert(
            &mut index,
            sibling_fallback,
            sibling_file,
            "narrate",
            SymbolKind::Knot,
            None,
        );

        assert_eq!(
            lookup_global(&index, project_file, "narrate", SymbolKind::Knot),
            Some(sibling_fallback),
            "a non-std cross-file fallback pair must still resolve, unchanged \
             from pre-#2197 behavior"
        );
    }

    // ── issue #2262: E181's own drop class, for CONST/VAR/EXTERNAL ─────

    fn hir_for(src: &str) -> hir::HirFile {
        let parsed = brink_syntax::parse(src);
        let (hir, _manifest, _diag) = hir::lower(FileId(0), &parsed.tree());
        hir
    }

    /// [`lookup_global_or_diagnose`] direct unit coverage for `CONST`/`VAR`
    /// (issue #2262): `std` declares neither today, so — unlike
    /// `EXTERNAL`'s own end-to-end test below — this stays a hand-built
    /// `SymbolIndex`, same "reachable in principle, not in practice yet"
    /// status `E181` itself carried before its own reachable `STRUCT` case
    /// was found (see `E184`'s own doc). Modeling the drop condition
    /// directly: `ghost_file` carries no own-file symbol at all — the
    /// aftermath of `brink-analyzer` dropping it as a true intra-module
    /// duplicate — and the only surviving same-name/same-kind candidate is
    /// std-declared, so `lookup_global`'s fallback also misses.
    #[test]
    fn lookup_global_or_diagnose_pushes_e184_when_every_surviving_candidate_is_std_declared() {
        for kind in [SymbolKind::Constant, SymbolKind::Variable] {
            let mut index = SymbolIndex::default();
            let ghost_file = FileId(7);
            let std_file = FileId(9);
            let std_def_id = DefinitionId::new(DefinitionTag::GlobalVar, 1);
            insert(
                &mut index,
                std_def_id,
                std_file,
                "MAX_HP",
                kind,
                Some("std::x"),
            );

            let mut diagnostics = Vec::new();
            let range = TextRange::new(3.into(), 9.into());
            let id = lookup_global_or_diagnose(
                &index,
                ghost_file,
                "MAX_HP",
                kind,
                range,
                &mut diagnostics,
            );

            assert_eq!(
                id, None,
                "the declaration still can't resolve its own identity for kind {kind:?}"
            );
            assert_eq!(
                diagnostics.len(),
                1,
                "the unresolvable lookup must raise exactly one diagnostic for kind {kind:?}"
            );
            assert_eq!(diagnostics[0].code, DiagnosticCode::E184);
            assert_eq!(diagnostics[0].file, ghost_file);
            assert_eq!(diagnostics[0].range, range);
        }
    }

    /// Sanity twin: an ordinary same-file self-declaration lookup that
    /// succeeds must push no diagnostic at all — `E184` is a backstop for
    /// the narrow drop case, not a diagnostic that fires on every call.
    #[test]
    fn lookup_global_or_diagnose_pushes_nothing_when_the_lookup_succeeds() {
        let mut index = SymbolIndex::default();
        let project_file = FileId(0);
        let expected_id = DefinitionId::new(DefinitionTag::GlobalVar, 2);
        insert(
            &mut index,
            expected_id,
            project_file,
            "score",
            SymbolKind::Variable,
            None,
        );

        let mut diagnostics = Vec::new();
        let id = lookup_global_or_diagnose(
            &index,
            project_file,
            "score",
            SymbolKind::Variable,
            TextRange::default(),
            &mut diagnostics,
        );

        assert_eq!(id, Some(expected_id));
        assert!(
            diagnostics.is_empty(),
            "an ordinary successful lookup must not raise E184: {diagnostics:?}"
        );
    }

    /// [`collect_externals`]'s own end-to-end regression (issue #2262):
    /// reachable **today**, unlike the `CONST`/`VAR` case above — `std`
    /// declares `extern scene_entered(title, slug)`
    /// (`std/conventions/screenplay.brink`), so a project's own
    /// `EXTERNAL scene_entered` colliding with it and losing the
    /// intra-module duplicate elimination is a real shape, not merely a
    /// hypothetical one (see `brink-environment`'s own
    /// `external_self_declaration_silently_drops_when_colliding_with_a_std_preset_name`
    /// for that compiled end to end through the real analyzer drop, not a
    /// hand-built `SymbolIndex`, matching `E181`'s own precedent). This
    /// test pins the same condition at the `collect_externals` unit level.
    ///
    /// Rule 20a: verified this assertion fails (`diagnostics` stays empty,
    /// `externals` silently empty with no signal at all) with the `E184`
    /// push removed from `lookup_global_or_diagnose` — restored before
    /// committing.
    #[test]
    fn collect_externals_reports_e184_when_every_surviving_candidate_is_std_declared() {
        let ghost_file = FileId(7);
        let ghost_hir = hir_for("EXTERNAL scene_entered(title, slug)\nHello.\n-> END\n");

        let mut index = SymbolIndex::default();
        let std_file = FileId(9);
        let std_def_id = DefinitionId::new(DefinitionTag::ExternalFn, 1);
        insert(
            &mut index,
            std_def_id,
            std_file,
            "scene_entered",
            SymbolKind::External,
            Some("std::conventions::screenplay"),
        );

        let mut names = NameTable::new();
        let mut diagnostics = Vec::new();
        let externals = collect_externals(
            &[(ghost_file, &ghost_hir)],
            &index,
            &mut names,
            &mut diagnostics,
        );

        assert!(
            externals.is_empty(),
            "the external still can't resolve its own identity, so it still \
             contributes no ExternalDef — E184 makes the drop loud, not \
             stops it from happening"
        );
        assert_eq!(
            diagnostics.len(),
            1,
            "the unresolvable self-declaration lookup must raise exactly one diagnostic"
        );
        assert_eq!(diagnostics[0].code, DiagnosticCode::E184);
        assert_eq!(diagnostics[0].file, ghost_file);
        assert_eq!(
            diagnostics[0].range, ghost_hir.externals[0].name.range,
            "reported at the external's own name span"
        );
    }
}
