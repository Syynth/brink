//! T1e-1 `ref lvalue-path` projection checks (docs/t1e-spec.md §2/§6, issue
//! #831, tracking #828).
//!
//! `ref lvalue-path` (`ref npc.hp`, `ref inventory[idx]`, `ref
//! party[leader].hp`) is the T1e-1 slice of grammar/HIR/analyzer work for
//! path projections: a `Expr::RefArg` HIR node that always parses (superset
//! grammar, same doctrine `dialect_gate` already applies to every other
//! brink extension) but is legal only in ref-argument position — a direct
//! argument of a call, `#fn(…)`, or `bind(…)`. No LIR/VM lands in this
//! slice (that's T1e-2, tracking #828); every construct this module accepts
//! still hits a defensive E052-style fence at LIR lowering
//! (`brink_ir::lir::lower::expr::lower_ref_arg_fence`, `E099`) unless it
//! degrades to a bare single-name `ref x` (zero path segments), which lowers
//! exactly like today's unmarked ref-argument binding.
//!
//! Two checks, run at different gates:
//!
//! - [`check`] — **E097** (standalone position) + **E080** (durable root).
//!   Brink-dialect-only, policy-independent (same rule `fn_values::check`
//!   already follows for `#fn`'s own E079/E080/E081): under `strict-ink`
//!   the whole `ref` expression is already rejected as extension syntax
//!   (`dialect_gate`'s own E051), so double-reporting content diagnostics
//!   on rejected syntax is noise.
//! - [`check_strict`] — **E098** (a projection segment disagrees with the
//!   root's statically-known shape). `types = strict` only, reusing
//!   `structs::declared_shapes`/`ShapeInfo` — the same shape table
//!   `structs::check`'s missing/extra/mistyped trio (E069–E071) already
//!   builds for construction literals — rather than a second one. The seed
//!   shape comes from the projection root's own `VAR name: Shape = …`
//!   annotation (TM-2); anything else (`Ty::Unknown`, no annotation, a
//!   non-struct declared type) is silently unchecked — "Unknown never
//!   disagrees", the same spirit `structs`/`conversions` already use.

use std::collections::{BTreeMap, BTreeSet};

use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Diagnostic, DiagnosticCode, Expr, FileId, HirFile, Name, Path, RefArgExpr, ResolutionMap,
    SymbolIndex, SymbolKind,
};
use rowan::TextRange;

use crate::annotations;
use crate::infer::Ty;
use crate::structs::{ShapeInfo, declared_shapes};

/// `(start, end)` key for range-indexed lookups (`TextRange` has no `Ord`) —
/// same convention `fn_values`/`dialect_gate` already use.
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

// ─── E097 (standalone position) + E080 (durable root) ────────────────

/// Walk every file's HIR once, validating every `Expr::RefArg` reachable
/// as a direct argument of a `Call`/`FnLiteral` (durable-root, `E080`) and
/// flagging every other `Expr::RefArg` as a standalone use (`E097`).
///
/// Callers only reach this under `dialect = brink` (mirrors
/// `fn_values::check`'s own entry condition) — `per_file_diagnostics`.
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    file_resolutions: &ResolutionMap,
    index: &SymbolIndex,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let by_range: BTreeMap<(u32, u32), brink_format::DefinitionId> = file_resolutions
            .iter()
            .filter(|r| r.file == file)
            .map(|r| (range_key(r.range), r.target))
            .collect();
        let mut v = RefArgVisitor {
            file,
            by_range: &by_range,
            index,
            claimed: BTreeSet::new(),
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
    }
    out
}

struct RefArgVisitor<'a> {
    file: FileId,
    by_range: &'a BTreeMap<(u32, u32), brink_format::DefinitionId>,
    index: &'a SymbolIndex,
    /// Ranges of `RefArg` nodes already validated as a legal direct
    /// ref-argument — the generic `Expr::RefArg` arm below skips these
    /// (`walk_expr` visits the parent's `enter_expr` before descending into
    /// its children, so a `Call`/`FnLiteral`'s own `enter_expr` claims its
    /// direct-arg `RefArg`s before the walker ever reaches them generically
    /// — see `hir::visit`'s doc: "dumb walker + stateful visitor").
    claimed: BTreeSet<(u32, u32)>,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for RefArgVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        match expr {
            Expr::Call(_, args) => self.claim_ref_args(args),
            Expr::FnLiteral(fl) => self.claim_ref_args(&fl.args),
            Expr::RefArg(ra) if !self.claimed.contains(&range_key(ra.ptr.text_range())) => {
                self.standalone(ra);
            }
            _ => {}
        }
    }
}

impl RefArgVisitor<'_> {
    fn push(&mut self, range: TextRange, message: String, code: DiagnosticCode) {
        self.diagnostics.push(Diagnostic {
            file: self.file,
            range,
            message,
            code,
        });
    }

    /// Validate every direct-child `RefArg` of a `Call`/`FnLiteral`'s
    /// argument list — covers ordinary calls, `bind(f, …)`/`call(f, …)`
    /// (both lower through `Expr::Call`, docs/t1c-spec.md §3), and
    /// `#fn(…)` uniformly. Claims each one's range so the generic
    /// `enter_expr` arm above doesn't also flag it as standalone.
    fn claim_ref_args(&mut self, args: &[Expr]) {
        for arg in args {
            if let Expr::RefArg(ra) = arg {
                self.claimed.insert(range_key(ra.ptr.text_range()));
                self.check_durable_root(ra);
            }
        }
    }

    fn standalone(&mut self, ra: &RefArgExpr) {
        let msg = format!(
            "{}: `ref` projections exist only in ref-argument position — a direct \
             argument of a call, `#fn(…)`, or `bind(…)` — never a standalone value or a \
             nested subexpression (docs/t1e-spec.md §2: \"projections exist only where \
             `ref` already exists: argument binding\"); first-class standalone projection \
             values are a deliberate future round, tracked as icebox #825",
            DiagnosticCode::E097.title(),
        );
        self.push(ra.ptr.text_range(), msg, DiagnosticCode::E097);
    }

    /// `E080` — the projection's root must be a durable global `VAR`
    /// (`#@local` flow-locals included), same rule T1c's unmarked
    /// ref-argument discipline (`fn_values::check_ref_arg`) already
    /// enforces, extended to the `ref lvalue-path` grammar (t1e-spec §2:
    /// "the root must be a durable cell … the T1c rule unchanged").
    fn check_durable_root(&mut self, ra: &RefArgExpr) {
        let reject = |cause: &str| {
            format!(
                "{}: a `ref` projection's root must be a durable cell (a VAR, including \
                 `#@local` flow-locals) — {cause} (docs/t1e-spec.md §2)",
                DiagnosticCode::E080.title(),
            )
        };
        let Some((root, _segments)) = decompose(&ra.operand) else {
            self.push(
                ra.ptr.text_range(),
                reject("this argument is not an lvalue"),
                DiagnosticCode::E080,
            );
            return;
        };
        let Some(&root_def) = self.by_range.get(&range_key(root.range)) else {
            // Unresolved reference — resolution's own E025 already covers it.
            return;
        };
        match self.index.symbols.get(&root_def) {
            Some(info) => match info.kind {
                SymbolKind::Variable => {}
                SymbolKind::Constant => {
                    self.push(
                        root.range,
                        reject("a CONST is not a mutable cell"),
                        DiagnosticCode::E080,
                    );
                }
                SymbolKind::Param | SymbolKind::Temp => {
                    self.push(
                        root.range,
                        reject("a temp/param dies with its frame (value-model §11)"),
                        DiagnosticCode::E080,
                    );
                }
                _ => {
                    self.push(
                        root.range,
                        reject(&format!("a {} is not a cell", kind_label(info.kind))),
                        DiagnosticCode::E080,
                    );
                }
            },
            None => {
                // Absent from the decls-only index projection: a local
                // temp/param (`fn_values`'s own `DefinitionTag::LocalVar`
                // pattern) — never a durable cell (value-model §11).
                if root_def.tag() == brink_format::DefinitionTag::LocalVar {
                    self.push(
                        root.range,
                        reject("a temp/param dies with its frame (value-model §11)"),
                        DiagnosticCode::E080,
                    );
                }
            }
        }
    }
}

fn kind_label(kind: SymbolKind) -> &'static str {
    match kind {
        SymbolKind::Knot => "knot",
        SymbolKind::Stitch => "stitch",
        SymbolKind::Variable => "variable",
        SymbolKind::Constant => "constant",
        SymbolKind::List => "LIST",
        SymbolKind::ListItem => "list item",
        SymbolKind::External => "external function",
        SymbolKind::Label => "label",
        SymbolKind::Param => "parameter",
        SymbolKind::Temp => "temp",
        SymbolKind::Struct => "STRUCT",
    }
}

/// One path-projection segment.
enum Segment<'a> {
    /// `.field` — a dotted field-access segment. Carries the field `Name`
    /// for both diagnostics (range) and lookup (text).
    Field(&'a Name),
    /// `[index]` — an indexing segment. The index subexpression is opaque
    /// here (T1e-1 doesn't statically check index *values*, only that the
    /// base being indexed is a collection — `E098`).
    Index(TextRange),
}

/// Decompose an lvalue-shaped expression into its root `Path` (the cell the
/// projection reads/writes through the runtime machinery) and its ordered
/// segment chain. `None` if `expr` isn't lvalue-shaped at all (a rvalue —
/// call, literal, infix, …).
///
/// A multi-segment `Path` (`npc.hp`, a bare dotted-identifier chain) is
/// exactly the TM-4b resolution-fallback shape: `brink-analyzer` resolves
/// the *whole* path's range to the head variable's `DefinitionId`, so the
/// root is the path itself (looked up by its full range) and every segment
/// past the first is a field-access segment — same convention
/// `fn_values::check_ref_arg` already documents for the unmarked-`ref` form.
fn decompose(expr: &Expr) -> Option<(&Path, Vec<Segment<'_>>)> {
    match expr {
        Expr::Path(p) => {
            let segments = p.segments[1..].iter().map(Segment::Field).collect();
            Some((p, segments))
        }
        Expr::FieldAccess(fa) => {
            let (root, mut segments) = decompose(&fa.base)?;
            segments.push(Segment::Field(&fa.field));
            Some((root, segments))
        }
        Expr::Index(idx) => {
            let (root, mut segments) = decompose(&idx.base)?;
            segments.push(Segment::Index(idx.ptr.text_range()));
            Some((root, segments))
        }
        _ => None,
    }
}

// ─── E098 (strict-mode segment-shape check) ───────────────────────────

/// Strict-mode-only: every `ref lvalue-path` projection's segments checked
/// against the root's statically-known declared shape (`VAR name: Shape =
/// …`, TM-2). Callers only reach this once `strict::config_error` has
/// confirmed `types = strict` + `dialect = brink` (mirrors
/// `structs::check`'s own entry condition) — wired into `strict::check`.
#[must_use]
pub fn check_strict(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
    resolutions: &ResolutionMap,
) -> Vec<Diagnostic> {
    let shapes = declared_shapes(files, index);
    let var_seeds = declared_var_shapes(files, index);
    let mut out = Vec::new();
    for &(file, hir) in files {
        let by_range: BTreeMap<(u32, u32), brink_format::DefinitionId> = resolutions
            .iter()
            .filter(|r| r.file == file)
            .map(|r| (range_key(r.range), r.target))
            .collect();
        let mut v = SegmentShapeVisitor {
            file,
            by_range,
            shapes: &shapes,
            var_seeds: &var_seeds,
            index,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
    }
    out
}

/// Every global `VAR`'s own declared shape name, by `(declaring file, VAR
/// name)` — the seed a projection's root type-walk starts from. Only a
/// `VAR name: Shape = …` annotation that resolves to `Ty::Struct(_)` seeds
/// anything; everything else (no annotation, a scalar/collection
/// annotation, an unresolved shape name) seeds nothing, matching
/// `structs::declared_shapes`' own "unresolved -> silent" contract.
fn declared_var_shapes(
    files: &[(FileId, &HirFile)],
    index: &SymbolIndex,
) -> BTreeMap<(FileId, String), Ty> {
    let names = annotations::TypeNames::new(index, None);
    let mut out = BTreeMap::new();
    for &(file, hir) in files {
        for var in &hir.variables {
            let Some(ann) = &var.annotation else {
                continue;
            };
            if let Some(ty @ Ty::Struct(_)) = annotations::resolve(ann, &names) {
                out.insert((file, var.name.text.clone()), ty);
            }
        }
    }
    out
}

struct SegmentShapeVisitor<'a> {
    file: FileId,
    by_range: BTreeMap<(u32, u32), brink_format::DefinitionId>,
    shapes: &'a BTreeMap<String, ShapeInfo>,
    var_seeds: &'a BTreeMap<(FileId, String), Ty>,
    index: &'a SymbolIndex,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for SegmentShapeVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::RefArg(ra) = expr {
            self.check(ra);
        }
    }
}

impl SegmentShapeVisitor<'_> {
    fn check(&mut self, ra: &RefArgExpr) {
        let Some((root, segments)) = decompose(&ra.operand) else {
            return; // not an lvalue at all — E080's job, not this check's.
        };
        // The root resolves (TM-4b fallback) by its *whole* range to the
        // head variable's `DefinitionId` — same convention `check_durable_root`
        // uses. `info.file`/`info.name` name that declaration directly, the
        // key `declared_var_shapes` seeded its map with.
        let Some(&root_def) = self.by_range.get(&range_key(root.range)) else {
            return;
        };
        let Some(info) = self.index.symbols.get(&root_def) else {
            return; // a local temp/param — never seeded (VAR-only seeding).
        };
        let Some(mut current) = self.var_seeds.get(&(info.file, info.name.clone())).cloned() else {
            return; // no statically-known shape to check against.
        };
        for segment in segments {
            match segment {
                Segment::Field(name) => match &current {
                    Ty::Struct(shape_name) => {
                        let Some(shape) = self.shapes.get(shape_name) else {
                            return; // unresolved shape — E068 already covers it.
                        };
                        if !shape.has_field(&name.text) {
                            self.push_e098(
                                name.range,
                                &format!(
                                    "`{shape_name}` has no field `{}` — the projection's \
                                     statically-known shape doesn't declare it",
                                    name.text,
                                ),
                            );
                            return;
                        }
                        current = shape.field_ty(&name.text).cloned().unwrap_or(Ty::Unknown);
                    }
                    Ty::Unknown => return, // Unknown never disagrees.
                    other => {
                        self.push_e098(
                            name.range,
                            &format!(
                                "`.{}` is a field-access segment, but the statically-known \
                                 type here is {} — not a STRUCT",
                                name.text,
                                ty_label(other),
                            ),
                        );
                        return;
                    }
                },
                Segment::Index(range) => match &current {
                    Ty::Array(elem) => current = elem.as_ref().clone(),
                    Ty::Map(_, val) => current = val.as_ref().clone(),
                    Ty::Unknown => return,
                    other => {
                        self.push_e098(
                            range,
                            &format!(
                                "`[…]` is an indexing segment, but the statically-known type \
                                 here is {} — not an array or map",
                                ty_label(other),
                            ),
                        );
                        return;
                    }
                },
            }
        }
    }

    fn push_e098(&mut self, range: TextRange, cause: &str) {
        self.diagnostics.push(Diagnostic {
            file: self.file,
            range,
            message: format!(
                "{}: {cause} (docs/t1e-spec.md §6)",
                DiagnosticCode::E098.title()
            ),
            code: DiagnosticCode::E098,
        });
    }
}

fn ty_label(ty: &Ty) -> String {
    match ty {
        Ty::Int => "int".to_string(),
        Ty::Float => "float".to_string(),
        Ty::Bool => "bool".to_string(),
        Ty::String => "string".to_string(),
        Ty::Divert => "divert".to_string(),
        Ty::List(name) => format!("List<{name}>"),
        Ty::Array(_) => "Array".to_string(),
        Ty::Map(_, _) => "Map".to_string(),
        Ty::Struct(name) => name.clone(),
        Ty::Fn(_, _) => "fn".to_string(),
        Ty::Unknown => "Unknown".to_string(),
        _ => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::hir::lower;

    /// Parse → HIR lower → index → resolve — the real per-file pipeline
    /// shape `per_file_diagnostics` drives (same helper style as
    /// `fn_values::tests::build`).
    fn build(src: &str) -> (HirFile, SymbolIndex, ResolutionMap) {
        let parsed = brink_syntax::parse(src);
        let (hir, manifest, _diag) = lower(FileId(0), &parsed.tree());
        let (index, _diag) = crate::symbol_index(&[(FileId(0), &manifest)]);
        let (resolutions, _diag) =
            crate::resolve(FileId(0), &manifest, &index, &crate::ImportScope::default());
        (hir, (*index).clone(), (*resolutions).clone())
    }

    fn check_src(src: &str) -> Vec<Diagnostic> {
        let (hir, index, res) = build(src);
        check(&[(FileId(0), &hir)], &res, &index)
    }

    // ── E080: durable-root discipline ───────────────────────────────

    #[test]
    fn ref_bare_var_arg_is_clean() {
        let src = "VAR gold = 5\n\
                   === function alter(ref x, k) ===\n~ x = x + k\n\n\
                   === main ===\n~ alter(ref gold, 7)\n-> DONE\n";
        let diags = check_src(src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn ref_dotted_field_projection_to_durable_var_is_clean() {
        let src = "VAR npc = 5\n\
                   === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
                   === main ===\n~ heal(ref npc.hp, 5)\n-> DONE\n";
        let diags = check_src(src);
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E080),
            "{diags:?}"
        );
    }

    #[test]
    fn ref_temp_root_is_e080() {
        let src = "=== function alter(ref x, k) ===\n~ x = x + k\n\n\
                   === main ===\n~ temp t = 1\n~ alter(ref t, 7)\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E080);
        assert!(diags[0].message.contains("frame"), "{diags:?}");
    }

    #[test]
    fn ref_const_root_is_e080() {
        let src = "CONST LIMIT = 100\n\
                   === function alter(ref x, k) ===\n~ x = x + k\n\n\
                   === main ===\n~ alter(ref LIMIT, 7)\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E080);
        assert!(diags[0].message.contains("CONST"), "{diags:?}");
    }

    #[test]
    fn ref_rvalue_is_e080() {
        let src = "=== function alter(ref x, k) ===\n~ x = x + k\n\n\
                   === main ===\n~ alter(ref (1 + 1), 7)\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E080);
    }

    // ── E097: standalone position ─────────────────────────────────────

    #[test]
    fn standalone_ref_in_temp_decl_is_e097() {
        let src = "VAR gold = 5\n=== main ===\n~ temp r = ref gold\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E097);
    }

    #[test]
    fn ref_nested_inside_an_infix_is_e097() {
        let src = "VAR gold = 5\n\
                   === function alter(ref x, k) ===\n~ x = x + k\n\n\
                   === main ===\n~ alter(ref gold + 0, 7)\n-> DONE\n";
        // `ref gold + 0` parses `ref` at `Prec::Prefix` tightest, so this is
        // `(ref gold) + 0` — the `RefArg` is nested inside an `Infix`, not a
        // direct call argument.
        let diags = check_src(src);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E097),
            "{diags:?}"
        );
    }

    #[test]
    fn ref_in_fn_literal_arg_position_is_not_standalone() {
        let src = "VAR gold = 5\n\
                   === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
                   === main ===\n~ temp f = #fn(heal, ref gold)\n-> DONE\n";
        let diags = check_src(src);
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E097),
            "{diags:?}"
        );
    }

    #[test]
    fn ref_in_bind_arg_position_is_not_standalone() {
        let src = "VAR gold = 5\n\
                   === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
                   === main ===\n~ temp f = #fn(heal)\n~ temp g = bind(f, ref gold)\n-> DONE\n";
        let diags = check_src(src);
        assert!(
            diags.iter().all(|d| d.code != DiagnosticCode::E097),
            "{diags:?}"
        );
    }

    // ── E098: strict-mode segment-shape check ───────────────────────────

    const NPC: &str = "STRUCT NPC = #{hp: int, name: string}\n";

    fn check_strict_src(src: &str) -> Vec<Diagnostic> {
        let (hir, index, res) = build(src);
        check_strict(&[(FileId(0), &hir)], &index, &res)
    }

    #[test]
    fn known_field_segment_against_annotated_shape_is_clean() {
        let src = format!(
            "{NPC}VAR npc: NPC = NPC#{{hp: 10, name: \"x\"}}\n\
             === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
             === main ===\n~ heal(ref npc.hp, 5)\n-> DONE\n"
        );
        let diags = check_strict_src(&src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unknown_field_segment_against_annotated_shape_is_e098() {
        let src = format!(
            "{NPC}VAR npc: NPC = NPC#{{hp: 10, name: \"x\"}}\n\
             === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
             === main ===\n~ heal(ref npc.mana, 5)\n-> DONE\n"
        );
        let diags = check_strict_src(&src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E098);
        assert!(diags[0].message.contains("mana"), "{diags:?}");
    }

    #[test]
    fn index_segment_on_a_struct_typed_root_is_e098() {
        let src = format!(
            "{NPC}VAR npc: NPC = NPC#{{hp: 10, name: \"x\"}}\n\
             === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
             === main ===\n~ heal(ref npc[0], 5)\n-> DONE\n"
        );
        let diags = check_strict_src(&src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E098);
    }

    #[test]
    fn unannotated_root_is_silently_unchecked() {
        // No `VAR npc: NPC` annotation — the shape isn't statically known,
        // so "Unknown never disagrees" and this stays silent (a runtime
        // fault concern, T1e-2's territory, not this check's).
        let src = "VAR npc = 5\n\
                   === function heal(ref hp, k) ===\n~ hp = hp + k\n\n\
                   === main ===\n~ heal(ref npc.anything, 5)\n-> DONE\n";
        let diags = check_strict_src(src);
        assert!(diags.is_empty(), "{diags:?}");
    }
}
