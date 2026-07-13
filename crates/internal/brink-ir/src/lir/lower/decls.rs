use brink_format::{DefinitionId, DefinitionTag};

use crate::symbols::{SymbolIndex, SymbolKind};
use crate::{Diagnostic, DiagnosticCode, FileId, hir};

use super::context::{NameTable, ResolutionLookup};
use super::expr::const_value_to_map_key;
use super::lir;

/// Collect global variable/constant definitions from HIR files.
///
/// Evaluates constants first so that variable initializers like `VAR x = c`
/// can resolve constant references to their values.
pub fn collect_globals(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    names: &mut NameTable,
    resolutions: &ResolutionLookup,
    diagnostics: &mut Vec<Diagnostic>,
) -> Vec<lir::GlobalDef> {
    use std::collections::HashMap;

    // Pass 1: evaluate all constants and build a value lookup.
    let mut const_values: HashMap<DefinitionId, lir::ConstValue> = HashMap::new();
    let mut globals = Vec::new();

    for &(file_id, hir_file) in files {
        for cst in &hir_file.constants {
            if let Some(id) = lookup_global(index, &cst.name.text, SymbolKind::Constant) {
                let name = names.intern(&cst.name.text);
                let default = eval_const_expr(
                    &cst.value,
                    index,
                    resolutions,
                    file_id,
                    &const_values,
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
        for var in &hir_file.variables {
            if let Some(id) = lookup_global(index, &var.name.text, SymbolKind::Variable) {
                let name = names.intern(&var.name.text);
                let default = eval_const_expr(
                    &var.value,
                    index,
                    resolutions,
                    file_id,
                    &const_values,
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

    for &(_file_id, hir_file) in files {
        for list_decl in &hir_file.lists {
            let Some(list_id) = lookup_global(index, &list_decl.name.text, SymbolKind::List) else {
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

                if let Some(item_id) = lookup_global(index, &qualified, SymbolKind::ListItem) {
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
pub fn collect_externals(
    files: &[(FileId, &hir::HirFile)],
    index: &SymbolIndex,
    names: &mut NameTable,
) -> Vec<lir::ExternalDef> {
    let mut externals = Vec::new();

    for &(_file_id, hir_file) in files {
        for ext in &hir_file.externals {
            if let Some(id) = lookup_global(index, &ext.name.text, SymbolKind::External) {
                let name = names.intern(&ext.name.text);
                // Look for an ink-defined function with the same name to use as fallback.
                let fallback = lookup_global(index, &ext.name.text, SymbolKind::Knot);
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

pub(super) fn lookup_global(
    index: &SymbolIndex,
    name: &str,
    kind: SymbolKind,
) -> Option<DefinitionId> {
    index.by_name.get(name).and_then(|ids| {
        ids.iter()
            .find(|&&id| index.symbols.get(&id).is_some_and(|info| info.kind == kind))
            .copied()
    })
}

/// Evaluate a compile-time constant expression.
#[expect(
    clippy::cast_possible_truncation,
    reason = "f64→f32 is intentional per ink spec"
)]
pub fn eval_const_expr(
    expr: &hir::Expr,
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    file: FileId,
    const_values: &std::collections::HashMap<DefinitionId, lir::ConstValue>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    match expr {
        hir::Expr::Int(n) => lir::ConstValue::Int(*n),
        hir::Expr::Float(bits) => lir::ConstValue::Float(bits.to_f64() as f32),
        hir::Expr::Bool(b) => lir::ConstValue::Bool(*b),
        hir::Expr::String(s) => eval_const_string(s, file, diagnostics),
        hir::Expr::Prefix(hir::PrefixOp::Negate, inner) => {
            match eval_const_expr(inner, index, resolutions, file, const_values, diagnostics) {
                lir::ConstValue::Int(n) => lir::ConstValue::Int(-n),
                lir::ConstValue::Float(f) => lir::ConstValue::Float(-f),
                _ => lir::ConstValue::Null,
            }
        }
        hir::Expr::Prefix(hir::PrefixOp::Not, inner) => {
            match eval_const_expr(inner, index, resolutions, file, const_values, diagnostics) {
                lir::ConstValue::Bool(b) => lir::ConstValue::Bool(!b),
                lir::ConstValue::Int(n) => lir::ConstValue::Bool(n == 0),
                lir::ConstValue::Float(f) => lir::ConstValue::Bool(f == 0.0),
                lir::ConstValue::Null => lir::ConstValue::Bool(true),
                _ => lir::ConstValue::Null,
            }
        }
        hir::Expr::Infix(lhs, op, rhs) => {
            let l = eval_const_expr(lhs, index, resolutions, file, const_values, diagnostics);
            let r = eval_const_expr(rhs, index, resolutions, file, const_values, diagnostics);
            eval_const_infix(&l, *op, &r)
        }
        hir::Expr::Path(path) => {
            if let Some(id) = resolutions.resolve(file, path.range) {
                if let Some(info) = index.symbols.get(&id) {
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
                } else {
                    lir::ConstValue::Null
                }
            } else {
                lir::ConstValue::Null
            }
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
        hir::Expr::ArrayLiteral(arr) => {
            eval_const_array_literal(arr, index, resolutions, file, const_values, diagnostics)
        }
        // #673: `VAR`/`CONST m = #{…}` — see `eval_const_map_literal`'s doc.
        hir::Expr::MapLiteral(map) => {
            eval_const_map_literal(map, index, resolutions, file, const_values, diagnostics)
        }
        // #673: `VAR`/`CONST p = Name#{…}` — see `eval_const_struct_literal`'s
        // doc.
        hir::Expr::StructLiteral(sl) => eval_const_struct_literal(sl, file, diagnostics),
        // T1c-1: `VAR f = #fn(…)` — see `eval_const_fn_literal`'s doc.
        hir::Expr::FnLiteral(fl) => eval_const_fn_literal(fl, file, diagnostics),
        _ => lir::ConstValue::Null,
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
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    file: FileId,
    const_values: &std::collections::HashMap<DefinitionId, lir::ConstValue>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let mut items = Vec::with_capacity(arr.elements.len());
    for e in &arr.elements {
        if !is_const_foldable_kind(e) {
            diagnostics.push(Diagnostic {
                file,
                range: arr.ptr.text_range(),
                message: DiagnosticCode::E077.title().to_string(),
                code: DiagnosticCode::E077,
            });
        }
        items.push(eval_const_expr(
            e,
            index,
            resolutions,
            file,
            const_values,
            diagnostics,
        ));
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
    index: &SymbolIndex,
    resolutions: &ResolutionLookup,
    file: FileId,
    const_values: &std::collections::HashMap<DefinitionId, lir::ConstValue>,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    let mut entries = Vec::with_capacity(map.entries.len());
    for (k, v) in &map.entries {
        if !is_const_foldable_kind(v) {
            diagnostics.push(Diagnostic {
                file,
                range: map.ptr.text_range(),
                message: DiagnosticCode::E077.title().to_string(),
                code: DiagnosticCode::E077,
            });
        }
        let key_val = eval_const_expr(k, index, resolutions, file, const_values, diagnostics);
        let value = eval_const_expr(v, index, resolutions, file, const_values, diagnostics);
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

/// #673: `ConstValue` has no record-carrying variant (adding one is a format
/// question outside this fix's fence, per the issue), and unlike
/// arrays/maps there is no existing codegen path to reuse: a global's
/// default is baked into `StoryData` at compile time, with no `RecordNew`
/// runtime construction step for a declaration default to defer to the way
/// a mid-story `p = Point#{…}` assignment has. A real, non-suppressible
/// compile error (`E075`) replaces the silent `Null` fallthrough — the
/// minimum-acceptable fix direction the issue names for exactly this case.
fn eval_const_struct_literal(
    sl: &hir::StructLiteral,
    file: FileId,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    diagnostics.push(Diagnostic {
        file,
        range: sl.ptr.text_range(),
        message: DiagnosticCode::E075.title().to_string(),
        code: DiagnosticCode::E075,
    });
    lir::ConstValue::Null
}

/// T1c-1: `VAR f = #fn(…)` — function values don't lower anywhere yet
/// (T1c-2, docs/t1c-spec.md §11), and a declaration default has no runtime
/// construction step to defer to regardless. A real compile error (E052,
/// same "not yet implemented" class as the expression-position rejection in
/// `expr::reject_fn_literal`), never a silent `Null` default.
fn eval_const_fn_literal(
    fl: &hir::FnLiteral,
    file: FileId,
    diagnostics: &mut Vec<Diagnostic>,
) -> lir::ConstValue {
    diagnostics.push(Diagnostic {
        file,
        range: fl.ptr.text_range(),
        message: "`#fn(…)` function values are not yet implemented in this slice — \
                  creation type-checks (T1c-1) but lowering/execution lands in T1c-2"
            .to_owned(),
        code: DiagnosticCode::E052,
    });
    lir::ConstValue::Null
}

/// #679 review: can this source expression *kind* ever constant-fold in a
/// declaration default? `false` means `eval_const_expr` is guaranteed to
/// land in a `Null` fallthrough — a function call, postfix indexing, field
/// access, or `++`/`--` has no compile-time evaluation and no runtime
/// construction step left to defer to, so an array element / map value of
/// that kind is a real compile error (`E077`), #673's silent-`Null` bug one
/// level down inside the literal.
///
/// Deliberately keyed off the expression kind, never the folded result:
///
/// - `Expr::Null` is HIR error recovery (or a missing initializer), not
///   author-writable source — folding it to `Null` is correct and already
///   diagnosed upstream, so it must not double-report here.
/// - `Expr::Path` constness depends on what the path resolves to; a bare
///   reference to another `VAR` folding to `Null` is the pre-existing,
///   separately-tracked gap #679's scope notes name, and its behavior is
///   deliberately unchanged.
/// - Collection/struct literals recurse through their own `eval_const_*`
///   arms, which do their own per-element checking (`E075`/`E076`/`E077`).
///
/// Exhaustive on purpose: a new `hir::Expr` variant must decide its
/// declaration-default story here instead of silently inheriting `true`.
fn is_const_foldable_kind(expr: &hir::Expr) -> bool {
    match expr {
        hir::Expr::Int(_)
        | hir::Expr::Float(_)
        | hir::Expr::Bool(_)
        | hir::Expr::String(_)
        | hir::Expr::Null
        | hir::Expr::Path(_)
        | hir::Expr::DivertTarget(_)
        | hir::Expr::ListLiteral(_)
        | hir::Expr::ArrayLiteral(_)
        | hir::Expr::MapLiteral(_)
        | hir::Expr::StructLiteral(_) => true,
        hir::Expr::Prefix(_, inner) => is_const_foldable_kind(inner),
        hir::Expr::Infix(lhs, _, rhs) => is_const_foldable_kind(lhs) && is_const_foldable_kind(rhs),
        hir::Expr::Postfix(..)
        | hir::Expr::Call(..)
        | hir::Expr::Index(_)
        | hir::Expr::FieldAccess(_)
        // T1c-1: `#fn(…)` never constant-folds — as a declaration default it
        // is already a targeted E052 at `eval_const_expr`'s own arm; as an
        // array/map element it reports the standard E077.
        | hir::Expr::FnLiteral(_) => false,
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
