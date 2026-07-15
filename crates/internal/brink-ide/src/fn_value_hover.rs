//! T1c-4 IDE polish (issue #702, docs/t1c-spec.md §11): hover on a fn-value
//! slot shows the bound signature display form.
//!
//! The runtime's authoritative display shape (docs/t1c-spec.md §5) is
//! signature-like — `fn heal(ref hp = player_hp, amount)` — with bound `val`
//! args rendered as their value and bound `ref` args rendered as the
//! captured cell's name (`brink_runtime`'s `display_fn_value`). There is no
//! compiled `Program` or runtime `Value` at hover time, so this builds the
//! *same shape* statically from the HIR: the target's declared param row
//! (name + ref/val-ness, from the resolved definition's `SymbolInfo`) laid
//! out against the creation site's bound-argument *source text*
//! ([`brink_ir::display_expr`]) rather than an evaluated value. The two
//! agree for the common case of a bare literal or cell-name argument.
//!
//! Deliberately scoped to a **direct** `#fn(target, args…)` literal only —
//! the one "marked site" `brink_analyzer::fn_values` already treats as
//! authoritative for every other T1c static obligation (E079/E080/E081).
//! Two source shapes bind a slot directly this way: the declaration's own
//! initializer (`~ temp f = #fn(heal, hp)`, `VAR f = #fn(heal, hp)`) and a
//! later plain assignment (`VAR healer = 0` … `~ healer = #fn(heal, hp)`,
//! the shape the T1c-2/T1c-3 corpus fixtures themselves use). `bind()`
//! chains and copy-of-another-variable assignment are not traced; tracing
//! them would mean re-deriving general dataflow, out of scope for this
//! pass. When a slot is assigned a `#fn(...)` literal more than once in the
//! file, the *last* one found by the HIR walk (source order) wins — a
//! static best guess, not a control-flow-aware "current value".

use brink_analyzer::AnalysisResult;
use brink_format::DefinitionId;
use brink_ir::hir::visit::{self, HirVisitor};
use brink_ir::{
    Assignment, BlockStmt, Expr, FileId, FnLiteral, HirFile, IfStmt, Stmt, SymbolInfo, SymbolKind,
};
use rowan::TextRange;

/// The bound-signature display text for a fn-value slot, if `info` is ever
/// directly bound to a `#fn(target, args…)` literal (declaration initializer
/// or a later plain assignment). `None` for every other symbol kind, and
/// when no direct `#fn(...)` binding is found (a `bind()` result, a copy of
/// another variable, an ordinary value, …).
#[must_use]
pub fn fn_value_slot_signature(
    analysis: &AnalysisResult,
    hir: &HirFile,
    info: &SymbolInfo,
) -> Option<String> {
    if !matches!(
        info.kind,
        SymbolKind::Variable | SymbolKind::Constant | SymbolKind::Temp
    ) {
        return None;
    }

    let declared = match info.kind {
        SymbolKind::Variable => hir
            .variables
            .iter()
            .find(|v| v.name.range == info.range)
            .map(|v| v.value.clone()),
        SymbolKind::Constant => hir
            .constants
            .iter()
            .find(|c| c.name.range == info.range)
            .map(|c| c.value.clone()),
        SymbolKind::Temp => find_temp_declared_init(hir, info.range),
        _ => None,
    };
    let assigned = find_last_assigned_fn_literal(hir, analysis, info.id, info.file);

    // A later assignment (if any) is the more current static picture; fall
    // back to the declaration's own initializer otherwise.
    let init = assigned.or(declared)?;
    let Expr::FnLiteral(fl) = init else {
        return None;
    };
    render_fn_literal(analysis, &fl, info.file)
}

/// Find the initializer of the `~ temp name = …` declaration whose *name*
/// range is `range` — mirrors how `info.range` identifies every other
/// symbol kind's declaration site. Covers both weave-side `Stmt::TempDecl`
/// (any nesting depth reachable by the shared HIR walk) and T1b `~ { … }`
/// block-scoped `BlockStmt::TempDecl` (the walk's closed `BlockStmt` set,
/// which the shared visitor doesn't call back into — see
/// `brink_ir::hir::visit`'s module doc — so it's re-walked here directly).
fn find_temp_declared_init(hir: &HirFile, range: TextRange) -> Option<Expr> {
    struct Finder {
        range: TextRange,
        found: Option<Expr>,
    }
    impl HirVisitor for Finder {
        fn enter_stmt(&mut self, stmt: &Stmt) {
            if self.found.is_some() {
                return;
            }
            match stmt {
                Stmt::TempDecl(t) if t.name.range == self.range => {
                    self.found.clone_from(&t.value);
                }
                Stmt::LogicBlock(lb) => {
                    self.found = find_temp_in_block_stmts(&lb.stmts, self.range);
                }
                _ => {}
            }
        }

        fn visit_exprs(&self) -> bool {
            false
        }
    }

    let mut finder = Finder { range, found: None };
    visit::visit(hir, &mut finder);
    finder.found
}

/// Walk every plain assignment in the file (`Stmt::Assignment` at any
/// weave-reachable nesting depth, plus `BlockStmt::Assignment` inside `~ { …
/// }` blocks) and return the value of the *last* one (source order) whose
/// target resolves to `def` and whose RHS is a `#fn(...)` literal.
fn find_last_assigned_fn_literal(
    hir: &HirFile,
    analysis: &AnalysisResult,
    def: DefinitionId,
    file: FileId,
) -> Option<Expr> {
    struct Finder<'a> {
        analysis: &'a AnalysisResult,
        def: DefinitionId,
        file: FileId,
        found: Option<Expr>,
    }
    impl Finder<'_> {
        fn consider(&mut self, assignment: &Assignment) {
            if !matches!(assignment.value, Expr::FnLiteral(_)) {
                return;
            }
            let Expr::Path(p) = &assignment.target else {
                return;
            };
            let targets_def = self
                .analysis
                .resolutions
                .iter()
                .any(|r| r.file == self.file && r.range == p.range && r.target == self.def);
            if targets_def {
                self.found = Some(assignment.value.clone());
            }
        }
    }
    impl HirVisitor for Finder<'_> {
        fn enter_stmt(&mut self, stmt: &Stmt) {
            match stmt {
                Stmt::Assignment(a) => self.consider(a),
                Stmt::LogicBlock(lb) => {
                    walk_block_stmts_for_assignments(&lb.stmts, self);
                }
                _ => {}
            }
        }

        fn visit_exprs(&self) -> bool {
            false
        }
    }

    fn walk_block_stmts_for_assignments(stmts: &[BlockStmt], finder: &mut Finder<'_>) {
        for bs in stmts {
            match bs {
                BlockStmt::Assignment(a) => finder.consider(a),
                BlockStmt::If(i) => walk_if_for_assignments(i, finder),
                BlockStmt::While(w) => walk_block_stmts_for_assignments(&w.body, finder),
                BlockStmt::For(f) => walk_block_stmts_for_assignments(&f.body, finder),
                _ => {}
            }
        }
    }

    fn walk_if_for_assignments(i: &IfStmt, finder: &mut Finder<'_>) {
        walk_block_stmts_for_assignments(&i.body, finder);
        match &i.else_branch {
            Some(brink_ir::ElseBranch::ElseIf(inner)) => walk_if_for_assignments(inner, finder),
            Some(brink_ir::ElseBranch::Else(body)) => {
                walk_block_stmts_for_assignments(body, finder);
            }
            None => {}
        }
    }

    let mut finder = Finder {
        analysis,
        def,
        file,
        found: None,
    };
    visit::visit(hir, &mut finder);
    finder.found
}

/// Recurse through a T1b `~ { … }` block body (and any nested `if`/`else
/// if`/`else`/`while`/`for` bodies within it) looking for a `TempDecl` whose
/// name range matches.
fn find_temp_in_block_stmts(stmts: &[BlockStmt], range: TextRange) -> Option<Expr> {
    for bs in stmts {
        match bs {
            BlockStmt::TempDecl(t) if t.name.range == range => return t.value.clone(),
            BlockStmt::If(i) => {
                if let Some(found) = find_temp_in_if_stmt(i, range) {
                    return Some(found);
                }
            }
            BlockStmt::While(w) => {
                if let Some(found) = find_temp_in_block_stmts(&w.body, range) {
                    return Some(found);
                }
            }
            BlockStmt::For(f) => {
                if let Some(found) = find_temp_in_block_stmts(&f.body, range) {
                    return Some(found);
                }
            }
            _ => {}
        }
    }
    None
}

fn find_temp_in_if_stmt(i: &IfStmt, range: TextRange) -> Option<Expr> {
    if let Some(found) = find_temp_in_block_stmts(&i.body, range) {
        return Some(found);
    }
    match &i.else_branch {
        Some(brink_ir::ElseBranch::ElseIf(inner)) => find_temp_in_if_stmt(inner, range),
        Some(brink_ir::ElseBranch::Else(body)) => find_temp_in_block_stmts(body, range),
        None => None,
    }
}

/// Render an already-parsed `#fn(target, args…)` literal in the runtime's
/// authoritative display shape, using the target's *resolved* declared
/// param row (so a renamed/reordered param — including a stale rehydrated
/// value's shape mismatch — is a non-issue here: hover always reads the
/// live signature, unlike a saved runtime closure).
///
/// `file` scopes the resolution lookup to the slot's own file: `resolutions`
/// is a project-wide `Vec<ResolvedRef>` and `range` is only a per-file byte
/// offset, so an unscoped range-only match can hit a same-range reference in
/// another file in a multi-file `INCLUDE` project (matching every sibling
/// lookup in this crate — see `navigation.rs::find_def_at_offset`,
/// `hover.rs`).
fn render_fn_literal(analysis: &AnalysisResult, fl: &FnLiteral, file: FileId) -> Option<String> {
    let target_def = analysis
        .resolutions
        .iter()
        .find(|r| r.file == file && r.range == fl.target.range)
        .map(|r| r.target)?;
    let target_info = analysis.index.symbols.get(&target_def)?;
    let target_name = fl
        .target
        .segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".");

    let mut parts = Vec::with_capacity(target_info.params.len());
    for (i, param) in target_info.params.iter().enumerate() {
        if let Some(arg) = fl.args.get(i) {
            if param.is_ref {
                // T1e (docs/t1e-spec.md §4 PROPOSED, issue #850): the bound
                // arg is either a bare cell (`Expr::Path`, unmarked —
                // vanilla ink's implicit-ref convention) or an explicit
                // `ref lvalue-path` (`Expr::RefArg`, a real path
                // projection — `ref npc.hp`). `display_expr`'s own
                // `Expr::RefArg` arm prepends its own `ref ` (it's also
                // used to render a bare `ref` expression standing alone,
                // where that prefix is the whole point) — call it on the
                // *operand* here instead of the outer node, so the `ref
                // {param.name} = ` this arm already renders isn't doubled
                // into `ref hp = ref npc.hp`. This mirrors the runtime's
                // own `display_fn_value` fix (`brink_runtime::value_ops`)
                // for the identical shape at the *evaluated* level.
                let path_text = match arg {
                    Expr::RefArg(ra) => brink_ir::display_expr(&ra.operand),
                    _ => brink_ir::display_expr(arg),
                };
                parts.push(format!("ref {} = {path_text}", param.name));
            } else {
                let arg_text = brink_ir::display_expr(arg);
                parts.push(format!("{} = {arg_text}", param.name));
            }
        } else {
            parts.push(param.name.clone());
        }
    }
    Some(format!("fn {target_name}({})", parts.join(", ")))
}

#[cfg(test)]
mod tests {
    use rowan::TextSize;

    use super::fn_value_slot_signature;
    use crate::navigation::find_def_at_offset;
    use crate::session::IdeSession;

    fn signature_at(src: &str, needle: &str) -> Option<String> {
        let mut session = IdeSession::new();
        let file_id = session.update_and_analyze("test.ink", src.to_string());
        let analysis = session.analysis().expect("analysis");
        let hir = session.hir(file_id).expect("hir");
        let pos = u32::try_from(src.find(needle).expect("needle present")).expect("offset");
        let info = find_def_at_offset(analysis, file_id, TextSize::from(pos))?;
        fn_value_slot_signature(analysis, hir, info)
    }

    /// Like [`signature_at`], but loads a multi-file project (`files`, each
    /// `(path, source)`, loaded in order) and queries the *last* file for
    /// `needle`. Used to prove the file-scoped resolution lookups in
    /// [`render_fn_literal`](super::render_fn_literal) and
    /// [`find_last_assigned_fn_literal`](super::find_last_assigned_fn_literal)
    /// aren't fooled by a same-`TextRange` resolution living in a different
    /// file of the project-wide `resolutions` vec.
    fn signature_at_multi(files: &[(&str, &str)], needle: &str) -> Option<String> {
        let mut session = IdeSession::new();
        let mut file_id = None;
        for (path, src) in files {
            file_id = Some(session.update_and_analyze(path, (*src).to_string()));
        }
        let file_id = file_id.expect("at least one file");
        let (_, last_src) = files.last().expect("at least one file");
        let analysis = session.analysis().expect("analysis");
        let hir = session.hir(file_id).expect("hir");
        let pos = u32::try_from(last_src.find(needle).expect("needle present")).expect("offset");
        let info = find_def_at_offset(analysis, file_id, TextSize::from(pos))?;
        fn_value_slot_signature(analysis, hir, info)
    }

    #[test]
    fn var_slot_assigned_later_shows_the_bound_signature() {
        // The corpus fixtures' own idiom: `VAR healer = 0`, bound later via
        // a plain assignment rather than the declaration's own initializer.
        let src = "\
VAR player_hp = 10
VAR healer = 0

~ healer = #fn(heal, player_hp)
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
";
        let sig = signature_at(src, "healer = 0").expect("fn-value slot signature");
        assert_eq!(sig, "fn heal(ref hp = player_hp, amount)");
    }

    #[test]
    fn var_slot_bound_at_declaration_shows_the_bound_signature() {
        let src = "\
VAR player_hp = 10
VAR healer = #fn(heal, player_hp)
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
";
        let sig = signature_at(src, "healer = #fn").expect("fn-value slot signature");
        assert_eq!(sig, "fn heal(ref hp = player_hp, amount)");
    }

    #[test]
    fn temp_slot_shows_the_bound_signature() {
        let src = "\
VAR player_hp = 10

~ temp healer = #fn(heal, player_hp)
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
";
        let sig = signature_at(src, "healer = #fn").expect("fn-value slot signature");
        assert_eq!(sig, "fn heal(ref hp = player_hp, amount)");
    }

    #[test]
    fn temp_slot_inside_a_logic_block_shows_the_bound_signature() {
        let src = "\
VAR player_hp = 10

~ {
    temp healer = #fn(heal, player_hp)
}
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
";
        let sig = signature_at(src, "healer = #fn").expect("fn-value slot signature");
        assert_eq!(sig, "fn heal(ref hp = player_hp, amount)");
    }

    #[test]
    fn unbound_params_render_bare() {
        let src = "\
~ temp d = #fn(double)
-> END

=== function double(x) ===
~ return x + x
";
        let sig = signature_at(src, "d = #fn").expect("fn-value slot signature");
        assert_eq!(sig, "fn double(x)");
    }

    /// T1e (docs/t1e-spec.md §4 PROPOSED, issue #850): a path-projection
    /// `ref` argument (`ref npc.hp`, not a bare cell name) renders its path
    /// exactly once — `ref hp = npc.hp`, not the doubled `ref hp = ref
    /// npc.hp` that would result from re-prepending `display_expr`'s own
    /// `ref `-prefixed `Expr::RefArg` rendering.
    #[test]
    fn projection_ref_arg_shows_the_path_not_a_doubled_ref_prefix() {
        let src = "\
STRUCT NPC = #{hp: int, name: string}
VAR npc = 0

~ temp healer = #fn(heal, ref npc.hp)
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
";
        let sig = signature_at(src, "healer = #fn").expect("fn-value slot signature");
        assert_eq!(sig, "fn heal(ref hp = npc.hp, amount)");
    }

    /// Same shape, an index segment (`docs/t1e-spec.md` §4's own
    /// `ref npc.inventory[3]` example) rather than a field — the display
    /// form chains both segment kinds the same way `display_expr`'s
    /// `Expr::Index`/`Expr::FieldAccess` arms already compose.
    #[test]
    fn projection_ref_arg_with_index_segment_shows_the_full_path() {
        let src = "\
STRUCT NPC = #{inventory: int, name: string}
VAR npc = 0

~ temp granter = #fn(grant, ref npc.inventory[3])
-> END

=== function grant(ref slot, amount) ===
~ slot = slot + amount
~ return slot
";
        let sig = signature_at(src, "granter = #fn").expect("fn-value slot signature");
        assert_eq!(sig, "fn grant(ref slot = npc.inventory[3], amount)");
    }

    #[test]
    fn ordinary_var_has_no_fn_value_signature() {
        let src = "VAR health = 100\n-> END\n";
        assert!(signature_at(src, "health = 100").is_none());
    }

    #[test]
    fn a_bind_result_has_no_static_signature() {
        // Out of scope for this pass (module doc): `bind()` chains aren't
        // traced, so a slot holding a `bind(...)` result shows nothing here
        // rather than a wrong/partial signature.
        let src = "\
~ temp f = #fn(double)
~ temp g = bind(f, 1)
-> END

=== function double(x) ===
~ return x + x
";
        assert!(signature_at(src, "g = bind").is_none());
    }

    #[test]
    fn cross_file_range_collision_does_not_mismatch_the_target_function() {
        // `lib.ink` binds an unrelated `fake(a, b, c)` at the *exact* byte
        // range (within its own file — same start AND same length, so the
        // `TextRange`s are literally equal) that `main.ink`'s `#fn(heal, …)`
        // target token occupies within *its* file. `ResolvedRef::range` is
        // only a per-file byte offset, so an unscoped
        // `resolutions.iter().find(|r| r.range == fl.target.range)` can
        // match `lib.ink`'s entry instead of `main.ink`'s own — rendering
        // the wrong function's signature. Regression for the review finding
        // on `render_fn_literal` (must filter `r.file == info.file`, same
        // convention as `navigation.rs::find_def_at_offset` / `hover.rs`).
        let lib_ink = "\
VAR x = 0

// xxxxxxxxxxxxxxxxxxxx
~ temp t = #fn(fake, x)
-> END

=== function fake(a, b, c) ===
~ return a + b + c
";
        let main_ink = "\
VAR player_hp = 10
VAR healer = 0

~ healer = #fn(heal, player_hp)
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp
";
        let sig = signature_at_multi(
            &[("lib.ink", lib_ink), ("main.ink", main_ink)],
            "healer = #fn",
        )
        .expect("fn-value slot signature");
        assert_eq!(
            sig, "fn heal(ref hp = player_hp, amount)",
            "must resolve against main.ink's own target, not lib.ink's same-range fake decoy"
        );
    }

    #[test]
    fn cross_file_shared_global_assignment_does_not_leak_across_files() {
        // `main.ink` declares the shared global `healer` and correctly
        // assigns it `#fn(heal, …)`. It also has an unrelated `target`
        // variable's assignment whose *target path range* is engineered to
        // coincide exactly (same start AND same length as `healer`, 6
        // bytes) with `lib.ink`'s own — separate — assignment of the *same*
        // shared `healer` global. Without a `r.file == info.file` filter,
        // `resolutions.iter().any(|r| r.range == p.range && r.target ==
        // self.def)` can be satisfied by `lib.ink`'s entry when checking
        // `main.ink`'s `target` statement, wrongly treating it as a (later,
        // so "most current") assignment to `healer` and returning
        // `wrong_fn`'s signature instead of the real `heal` binding.
        // Regression for the review finding on
        // `find_last_assigned_fn_literal`.
        let lib_ink = "\
// xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
~ healer = #fn(heal_lib, player_hp)
-> END

=== function heal_lib(x, y, z) ===
~ return x + y + z
";
        let main_ink = "\
INCLUDE lib.ink
VAR player_hp = 10
VAR healer = 0
VAR target = 0

~ healer = #fn(heal, player_hp)
~ target = #fn(wrong_fn, player_hp)
-> END

=== function heal(ref hp, amount) ===
~ hp = hp + amount
~ return hp

=== function wrong_fn(a, b, c) ===
~ return a + b + c
";
        let sig = signature_at_multi(
            &[("lib.ink", lib_ink), ("main.ink", main_ink)],
            "healer = #fn",
        )
        .expect("fn-value slot signature");
        assert_eq!(
            sig, "fn heal(ref hp = player_hp, amount)",
            "the genuine main.ink assignment to healer must win, not the coincidental \
             same-range target statement"
        );
    }
}
