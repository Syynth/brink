//! T1c creation-site checks for `#fn(name, args…)` function-value literals
//! (docs/t1c-spec.md §2/§8, issue #699).
//!
//! "Every static obligation lands at this one marked site" — the creation
//! site is where the target name becomes a value, where `ref` params bind,
//! and (T2) where the effect row freezes. Three diagnostics enforce the
//! ruled discipline:
//!
//! - **E079** — the target must resolve to a statically-named *function
//!   definition* (`=== function name ===`). A variable, list, external,
//!   label, non-function knot/stitch, or a builtin/stdlib intrinsic name is
//!   not a definition a fn token can be taken of. A target that failed
//!   resolution entirely already carries resolution's own `E025` — not
//!   double-reported here — *except* the builtin/stdlib names, which
//!   `resolve::resolve_function` deliberately skips without a diagnostic
//!   (they're valid *calls*), so this pass is where those become errors as
//!   `#fn` targets.
//! - **E080** — every `ref` param of the target must be bound in the
//!   creation-site prefix, and each `ref`-position argument must be an
//!   lvalue naming a durable cell: a global `VAR` (`#@local` flow-local
//!   VARs included — same `SymbolKind::Variable` at this layer). `temp`s
//!   and params die with the frame (value-model §11), `CONST` is not a
//!   mutable cell, and rvalues/field projections are not cells at all —
//!   "a durable cell, never a heap location".
//! - **E081** — the bound args are a *prefix* of the declared param row;
//!   binding more than the target declares is a compile error.
//!
//! Runs only under `dialect = brink` (`per_file_diagnostics` gates the
//! call): under `strict-ink` the whole literal is already rejected as
//! extension syntax (E051), and content diagnostics on rejected syntax are
//! noise (the TM-2 annotation-content precedent, ruling 2026-07-13).
//! Dialect-level, not type-policy-level: these obligations hold under
//! `types = gradual` too.

use std::collections::BTreeMap;

use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Diagnostic, DiagnosticCode, Expr, FileId, FnLiteral, HirFile, ResolutionMap, SymbolIndex,
    SymbolKind,
};
use rowan::TextRange;

/// `(start, end)` key for range-indexed lookups (`TextRange` has no `Ord`).
fn range_key(range: TextRange) -> (u32, u32) {
    (range.start().into(), range.end().into())
}

/// Walk one file's HIR and check every `#fn` creation site.
///
/// `file_resolutions` is this file's own slice of the resolution map (the
/// same shape [`crate::per_file_diagnostics`] hands `dialect_gate::check`);
/// `index` supplies the resolved target's kind/params and each
/// `ref`-argument's resolved kind.
#[must_use]
pub fn check(
    files: &[(FileId, &HirFile)],
    file_resolutions: &ResolutionMap,
    index: &SymbolIndex,
) -> Vec<Diagnostic> {
    let mut out = Vec::new();
    for &(file, hir) in files {
        let by_range: BTreeMap<(u32, u32), DefinitionId> = file_resolutions
            .iter()
            .filter(|r| r.file == file)
            .map(|r| (range_key(r.range), r.target))
            .collect();
        let mut v = FnValueVisitor {
            file,
            by_range: &by_range,
            index,
            diagnostics: &mut out,
        };
        visit::visit(hir, &mut v);
    }
    out
}

struct FnValueVisitor<'a> {
    file: FileId,
    by_range: &'a BTreeMap<(u32, u32), DefinitionId>,
    index: &'a SymbolIndex,
    diagnostics: &'a mut Vec<Diagnostic>,
}

impl HirVisitor for FnValueVisitor<'_> {
    fn visit_exprs(&self) -> bool {
        true
    }

    fn enter_expr(&mut self, expr: &Expr) {
        if let Expr::FnLiteral(fl) = expr {
            self.check_fn_literal(fl);
        }
    }
}

impl FnValueVisitor<'_> {
    fn push(&mut self, range: TextRange, message: String, code: DiagnosticCode) {
        self.diagnostics.push(Diagnostic {
            file: self.file,
            range,
            message,
            code,
        });
    }

    fn check_fn_literal(&mut self, fl: &FnLiteral) {
        let target_name = fl
            .target
            .segments
            .iter()
            .map(|s| s.text.as_str())
            .collect::<Vec<_>>()
            .join(".");

        let Some(&def) = self.by_range.get(&range_key(fl.target.range)) else {
            // No resolution entry. For an ordinary unknown name, resolution
            // already reported `E025` — nothing to add. The builtin
            // (`RANDOM`, `INT`, …) and stdlib-intrinsic (`len`, `push`, …)
            // names are skipped silently by `resolve_function` because they
            // are valid *calls* — but they are not definitions, so as `#fn`
            // targets they are this pass's E079.
            let is_intrinsic = fl.target.segments.len() == 1
                && (crate::resolve::is_builtin_function(&target_name)
                    || crate::resolve::is_t1b_stdlib_name(&target_name));
            if is_intrinsic {
                self.push(
                    fl.target.range,
                    format!(
                        "`#fn` target `{target_name}` is a builtin, not a function \
                         definition — only a statically-named `=== function ===` can \
                         become a function value (docs/t1c-spec.md §2)"
                    ),
                    DiagnosticCode::E079,
                );
            }
            return;
        };

        let Some(info) = self.index.symbols.get(&def) else {
            // Not in the index. Under `brink-db`'s salsa pipeline the index
            // handed to per-file passes is the decls-only cutoff projection
            // (`resolution_index_query` strips `Param`/`Temp` symbols), so a
            // target that resolved to a *local* arrives here with a
            // resolution entry but no index info — classify by the id's own
            // `DefinitionTag` instead (locals are `LocalVar` by
            // construction, `SymbolKind::definition_tag`).
            if def.tag() == brink_format::DefinitionTag::LocalVar {
                self.push(
                    fl.target.range,
                    format!(
                        "`#fn` target `{target_name}` does not resolve to a \
                         statically-named function definition (resolved to a local \
                         temp/param) — declare the target as `=== function \
                         {target_name} ===` (docs/t1c-spec.md §2)"
                    ),
                    DiagnosticCode::E079,
                );
            }
            return;
        };

        let is_function_def = matches!(info.kind, SymbolKind::Knot | SymbolKind::Stitch)
            && info.detail.as_deref() == Some("function");
        if !is_function_def {
            self.push(
                fl.target.range,
                format!(
                    "`#fn` target `{target_name}` does not resolve to a statically-named \
                     function definition (resolved to a {}) — declare the target as \
                     `=== function {target_name} ===` (docs/t1c-spec.md §2)",
                    kind_label(info.kind),
                ),
                DiagnosticCode::E079,
            );
            return;
        }

        // E081 — over-binding: the bound args are a prefix, never longer
        // than the declared row.
        if fl.args.len() > info.params.len() {
            self.push(
                fl.ptr.text_range(),
                format!(
                    "`#fn` binds {} argument(s) but `{target_name}` declares only {} \
                     parameter(s) — bound args are a prefix of the declared row \
                     (docs/t1c-spec.md §2)",
                    fl.args.len(),
                    info.params.len(),
                ),
                DiagnosticCode::E081,
            );
        }

        // E080 — every `ref` param must be bound at creation, to a durable
        // cell.
        for (i, param) in info.params.iter().enumerate() {
            if !param.is_ref {
                continue;
            }
            match fl.args.get(i) {
                None => {
                    self.push(
                        fl.ptr.text_range(),
                        format!(
                            "ref parameter `{}` of `{target_name}` must be bound at \
                             creation — all ref params bind in the `#fn` prefix \
                             (docs/t1c-spec.md §2)",
                            param.name,
                        ),
                        DiagnosticCode::E080,
                    );
                }
                Some(arg) => self.check_ref_arg(fl, &target_name, &param.name, arg),
            }
        }
    }

    /// A `ref`-position argument must be an lvalue naming a durable cell:
    /// a single-segment path resolving to a global `VAR`. Everything else —
    /// rvalues, `temp`s/params, `CONST`s, dotted field projections — is
    /// E080 with a cause-specific message.
    fn check_ref_arg(&mut self, fl: &FnLiteral, target_name: &str, param_name: &str, arg: &Expr) {
        let reject = |cause: &str| {
            format!(
                "ref parameter `{param_name}` of `{target_name}` must capture a durable \
                 cell (a VAR, including `#@local` flow-locals) — {cause} \
                 (docs/t1c-spec.md §2)"
            )
        };
        let Expr::Path(p) = arg else {
            let msg = reject("this argument is not an lvalue");
            self.push(fl.ptr.text_range(), msg, DiagnosticCode::E080);
            return;
        };
        let Some(&arg_def) = self.by_range.get(&range_key(p.range)) else {
            // Unresolved reference — resolution's own E025 already covers it.
            return;
        };
        let Some(info) = self.index.symbols.get(&arg_def) else {
            // Resolved but absent from the decls-only index projection (see
            // `check_fn_literal`'s target arm): a local temp/param — never a
            // durable cell (value-model §11).
            if arg_def.tag() == brink_format::DefinitionTag::LocalVar {
                let msg = reject("a temp/param dies with its frame (value-model §11)");
                self.push(p.range, msg, DiagnosticCode::E080);
            }
            return;
        };
        // The TM-4b resolution fallback resolves a dotted `p.x` to its head
        // variable — that's a field projection (a heap location), never a
        // cell, regardless of what the head is.
        if p.segments.len() > 1 {
            let msg = reject("a field projection is a heap location, not a cell");
            self.push(p.range, msg, DiagnosticCode::E080);
            return;
        }
        match info.kind {
            SymbolKind::Variable => {}
            SymbolKind::Constant => {
                let msg = reject("a CONST is not a mutable cell");
                self.push(p.range, msg, DiagnosticCode::E080);
            }
            SymbolKind::Param | SymbolKind::Temp => {
                let msg = reject("a temp/param dies with its frame (value-model §11)");
                self.push(p.range, msg, DiagnosticCode::E080);
            }
            _ => {
                let msg = reject(&format!("a {} is not a cell", kind_label(info.kind)));
                self.push(p.range, msg, DiagnosticCode::E080);
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

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::hir::lower;

    /// Parse → HIR lower → index → resolve — the real per-file pipeline
    /// shape `per_file_diagnostics` drives (same helper style as
    /// `strict::tests::build`).
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

    const HEAL: &str = "=== function heal(ref hp, amount) ===\n~ hp = hp + amount\n~ return hp\n\n";
    const PURE: &str = "=== function double(x) ===\n~ return x + x\n\n";

    // ── E079: target must be a function definition ───────────────────

    #[test]
    fn function_knot_target_is_clean() {
        let src = format!("{PURE}VAR v = 0\n=== main ===\n~ temp f = #fn(double, 1)\n-> DONE\n");
        let diags = check_src(&src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn variable_target_is_e079() {
        let src = "VAR gold = 5\n=== main ===\n~ temp f = #fn(gold)\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E079);
        assert!(diags[0].message.contains("variable"), "{diags:?}");
    }

    #[test]
    fn non_function_knot_target_is_e079() {
        let src = "=== plain_knot ===\nHello.\n-> DONE\n=== main ===\n~ temp f = #fn(plain_knot)\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E079);
        assert!(diags[0].message.contains("knot"), "{diags:?}");
    }

    #[test]
    fn stdlib_intrinsic_target_is_e079() {
        // `len` never resolves (it's the builtin) — resolution skips it
        // silently as a *call*, so this pass is where it errors as a
        // target.
        let src = "=== main ===\n~ temp f = #fn(len)\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E079);
        assert!(diags[0].message.contains("builtin"), "{diags:?}");
    }

    #[test]
    fn uppercase_builtin_target_is_e079() {
        let src = "=== main ===\n~ temp f = #fn(RANDOM)\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E079);
    }

    #[test]
    fn unknown_target_is_left_to_resolutions_e025_not_double_reported() {
        let src = "=== main ===\n~ temp f = #fn(nowhere)\n-> DONE\n";
        let (hir, index, res) = build(src);
        let diags = check(&[(FileId(0), &hir)], &res, &index);
        assert!(diags.is_empty(), "E025 owns unknown names: {diags:?}");
    }

    #[test]
    fn external_target_is_e079() {
        let src = "EXTERNAL beep(x)\n=== main ===\n~ temp f = #fn(beep)\n-> DONE\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E079);
        assert!(diags[0].message.contains("external"), "{diags:?}");
    }

    // ── E080: ref-binding discipline ──────────────────────────────────

    #[test]
    fn ref_param_bound_to_var_is_clean() {
        let src = format!(
            "{HEAL}VAR player_hp = 10\n=== main ===\n~ temp f = #fn(heal, player_hp)\n-> DONE\n"
        );
        let diags = check_src(&src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn unbound_ref_param_is_e080() {
        let src = format!("{HEAL}=== main ===\n~ temp f = #fn(heal)\n-> DONE\n");
        let diags = check_src(&src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E080);
        assert!(diags[0].message.contains("must be bound"), "{diags:?}");
    }

    #[test]
    fn ref_param_bound_to_temp_is_e080() {
        let src = format!(
            "{HEAL}=== main ===\n~ temp local_hp = 10\n~ temp f = #fn(heal, local_hp)\n-> DONE\n"
        );
        let diags = check_src(&src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E080);
        assert!(diags[0].message.contains("frame"), "{diags:?}");
    }

    #[test]
    fn ref_param_bound_to_rvalue_is_e080() {
        let src = format!("{HEAL}=== main ===\n~ temp f = #fn(heal, 5 + 1)\n-> DONE\n");
        let diags = check_src(&src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E080);
        assert!(diags[0].message.contains("lvalue"), "{diags:?}");
    }

    #[test]
    fn ref_param_bound_to_const_is_e080() {
        let src = format!(
            "CONST LIMIT = 100\n{HEAL}=== main ===\n~ temp f = #fn(heal, LIMIT)\n-> DONE\n"
        );
        let diags = check_src(&src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E080);
        assert!(diags[0].message.contains("CONST"), "{diags:?}");
    }

    #[test]
    fn val_params_never_require_binding() {
        // `amount` (val) stays unbound — perfectly legal; only `hp` (ref)
        // must bind.
        let src = format!(
            "{HEAL}VAR player_hp = 10\n=== main ===\n~ temp f = #fn(heal, player_hp)\n-> DONE\n"
        );
        let diags = check_src(&src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    #[test]
    fn zero_arg_creation_over_a_ref_free_target_is_clean() {
        // `#fn(name)` with zero bound args is legal iff the target has no
        // ref params (docs/t1c-spec.md §2).
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double)\n-> DONE\n");
        let diags = check_src(&src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── E081: over-binding ────────────────────────────────────────────

    #[test]
    fn binding_more_args_than_declared_is_e081() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1, 2)\n-> DONE\n");
        let diags = check_src(&src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E081);
        assert!(diags[0].message.contains("2 argument"), "{diags:?}");
    }

    #[test]
    fn binding_exactly_the_declared_row_is_clean() {
        let src = format!("{PURE}=== main ===\n~ temp f = #fn(double, 1)\n-> DONE\n");
        let diags = check_src(&src);
        assert!(diags.is_empty(), "{diags:?}");
    }

    // ── Nesting ───────────────────────────────────────────────────────

    #[test]
    fn nested_fn_literal_inside_a_call_argument_is_checked() {
        let src = "VAR gold = 5\n=== main ===\n~ temp x = double(#fn(gold))\n-> DONE\n\
                   === function double(x) ===\n~ return x + x\n";
        let diags = check_src(src);
        assert_eq!(diags.len(), 1, "{diags:?}");
        assert_eq!(diags[0].code, DiagnosticCode::E079);
    }
}
