//! Per-scope LIR chunks with chunk-local (symbolic) name references (FG-4c,
//! #817 — `docs/fine-grained-salsa-proposal.md` §5 + the
//! three-resolution-moments appendix).
//!
//! Lowering no longer threads a single shared
//! [`NameTable`](super::context::NameTable) through the whole container tree.
//! Instead each top-level scope (one file's root-level content, or a whole
//! knot subtree) is lowered against its **own** fresh `NameTable`, producing
//! a self-contained
//! [`ScopeChunk`]: a slice of the container tree whose `NameId`s index the
//! chunk's *own* owned `local_names` table rather than a project-wide one.
//!
//! A chunk is therefore the LIR analogue of FG-4b's `ContainerChunk`: its
//! name references are symbolic (the strings are owned by the chunk, the
//! content *is* the address) and it depends on no shared mutable state, so it
//! is memoizable in isolation (the FG-4d win this representation exists to
//! unlock). The whole-project **assembly** step ([`assemble_scopes`]) is the
//! LIR analogue of FG-4b's link phase: it merges every chunk's local names
//! into the project name table in deterministic walk order and *relocates*
//! each chunk's local `NameId`s to their assembled positions
//! ([`remap_container`] / [`remap_stmts`]), yielding today's whole-project
//! [`lir::Container`](super::lir::Container). The relocation erases itself —
//! a merged name table built in the fixed decls-then-walk order is
//! byte-identical to the one the old shared-table walk produced.
//!
//! History-independence (the FG-4d gate): a chunk's local table lists the
//! strings it references in first-occurrence walk order, and the merge is a
//! pure dedup over that order — both content-derived, never allocation-history
//! derived. Assembling the chunks in the same order the tree is walked
//! reproduces the project name table exactly whether a chunk was freshly
//! lowered or (in a future incremental world) reused from cache.

use brink_format::NameId;

use super::context::NameTable;
use super::lir;

/// One lowered top-level scope, self-contained w.r.t. names.
///
/// `body`/`children` carry `NameId`s that index this chunk's own
/// [`local_names`](Self::local_names), *not* the assembled project name
/// table. [`assemble_scopes`] relocates them.
///
/// A **root-content** chunk (one file's top-level content) carries that
/// file's top-level `body` statements and its inline `children`
/// (sequence/choice/gather containers created while lowering that body). A
/// **knot** chunk carries an empty `body` and a single-element `children`
/// holding the knot container (with its stitches and inline children nested
/// inside). Both shapes flatten into the root container by the assembler in
/// exactly the order the old `lower_root` appended them.
pub(super) struct ScopeChunk {
    pub body: Vec<lir::Stmt>,
    pub children: Vec<lir::Container>,
    /// The chunk's owned name strings, in first-occurrence walk order. Local
    /// `NameId(i)` refers to `local_names[i]`.
    pub local_names: Vec<String>,
}

impl ScopeChunk {
    /// A root-content chunk: a file's top-level `body` + its inline
    /// `children`, plus the local names collected while lowering them.
    pub fn root_content(
        body: Vec<lir::Stmt>,
        children: Vec<lir::Container>,
        local_names: Vec<String>,
    ) -> Self {
        Self {
            body,
            children,
            local_names,
        }
    }

    /// A knot chunk: the lowered knot container (stitches + inline children
    /// nested) plus the names collected while lowering the whole subtree.
    pub fn knot(container: lir::Container, local_names: Vec<String>) -> Self {
        Self {
            body: Vec::new(),
            children: vec![container],
            local_names,
        }
    }
}

/// Merge every chunk's `local_names` into `names` (the project table, already
/// seeded with declaration/struct names) in walk order, relocate each chunk's
/// local `NameId`s to their assembled positions, and flatten the chunks into
/// the root container's `body`/`children`.
///
/// The merge is a pure first-occurrence dedup, so a string a chunk references
/// that was already interned (by decls, structs, or an earlier chunk) keeps
/// its earlier assembled id — exactly what the old single shared-table walk
/// did, hence byte-identical.
pub(super) fn assemble_scopes(
    chunks: Vec<ScopeChunk>,
    names: &mut NameTable,
) -> (Vec<lir::Stmt>, Vec<lir::Container>) {
    let mut body = Vec::new();
    let mut children = Vec::new();

    for mut chunk in chunks {
        // Relocation table: local NameId -> assembled NameId.
        let map: Vec<NameId> = chunk.local_names.iter().map(|s| names.intern(s)).collect();

        remap_stmts(&mut chunk.body, &map);
        for c in &mut chunk.children {
            remap_container(c, &map);
        }

        body.extend(chunk.body);
        children.append(&mut chunk.children);
    }

    (body, children)
}

// ─── Name-id relocation visitor ─────────────────────────────────────
//
// Every `NameId` reachable from a chunk's subtree is a chunk-local index and
// must be relocated to its assembled position via `map`. The matches below
// are deliberately **exhaustive** (no `_ =>` arms on the name-bearing enums):
// a new `NameId`-carrying LIR variant will fail to compile here until it is
// handled, so this pass can never silently drop a name reference (the
// silent-data-drop hazard the project guards against).

fn relocate(id: &mut NameId, map: &[NameId]) {
    if let Some(assembled) = map.get(id.0 as usize) {
        *id = *assembled;
    }
}

fn remap_container(c: &mut lir::Container, map: &[NameId]) {
    for p in &mut c.params {
        relocate(&mut p.name, map);
    }
    remap_stmts(&mut c.body, map);
    for child in &mut c.children {
        remap_container(child, map);
    }
}

fn remap_stmts(stmts: &mut [lir::Stmt], map: &[NameId]) {
    for s in stmts {
        remap_stmt(s, map);
    }
}

fn remap_stmt(stmt: &mut lir::Stmt, map: &[NameId]) {
    use lir::Stmt;
    match stmt {
        Stmt::EmitContent(content) => remap_content(content, map),
        Stmt::EmitLine(emission) | Stmt::EvalLine(emission) => remap_emission(emission, map),
        Stmt::ChoiceOutput { content, emission } => {
            remap_content(content, map);
            if let Some(e) = emission {
                remap_emission(e, map);
            }
        }
        Stmt::Divert(d) => remap_divert(d, map),
        Stmt::TunnelCall(t) => {
            for target in &mut t.targets {
                remap_divert_target(&mut target.target, map);
                remap_call_args(&mut target.args, map);
            }
        }
        Stmt::ThreadStart(t) => {
            remap_divert_target(&mut t.target, map);
            remap_call_args(&mut t.args, map);
        }
        Stmt::DeclareTemp {
            slot: _,
            name,
            value,
        } => {
            relocate(name, map);
            if let Some(v) = value {
                remap_expr(v, map);
            }
        }
        Stmt::Assign {
            target,
            op: _,
            value,
        } => {
            remap_assign_target(target, map);
            remap_expr(value, map);
        }
        Stmt::Return {
            value,
            is_tunnel: _,
            args,
        } => {
            if let Some(v) = value {
                remap_expr(v, map);
            }
            remap_call_args(args, map);
        }
        Stmt::ChoiceSet(cs) => {
            for choice in &mut cs.choices {
                remap_choice(choice, map);
            }
        }
        Stmt::Conditional(cond) => remap_conditional(cond, map),
        Stmt::Sequence(seq) => remap_sequence(seq, map),
        Stmt::ExprStmt(e) => remap_expr(e, map),
        Stmt::LogicWhile(w) => {
            remap_expr(&mut w.condition, map);
            remap_stmts(&mut w.body, map);
            remap_stmts(&mut w.post, map);
        }
        // No name references.
        Stmt::EnterContainer(_) | Stmt::EndOfLine | Stmt::LogicBreak | Stmt::LogicContinue => {}
    }
}

fn remap_divert(d: &mut lir::Divert, map: &[NameId]) {
    remap_divert_target(&mut d.target, map);
    remap_call_args(&mut d.args, map);
}

fn remap_divert_target(t: &mut lir::DivertTarget, map: &[NameId]) {
    use lir::DivertTarget;
    match t {
        DivertTarget::VariableTemp(_, name) => relocate(name, map),
        DivertTarget::Address(_)
        | DivertTarget::Variable(_)
        | DivertTarget::Done
        | DivertTarget::End => {}
    }
}

fn remap_call_args(args: &mut [lir::CallArg], map: &[NameId]) {
    use lir::CallArg;
    for arg in args {
        match arg {
            CallArg::Value(e) => remap_expr(e, map),
            CallArg::RefTemp(_, name) => relocate(name, map),
            CallArg::RefGlobal(_) => {}
        }
    }
}

fn remap_assign_target(t: &mut lir::AssignTarget, map: &[NameId]) {
    use lir::AssignTarget;
    match t {
        AssignTarget::Temp(_, name) => relocate(name, map),
        AssignTarget::Global(_) => {}
    }
}

fn remap_choice(choice: &mut lir::Choice, map: &[NameId]) {
    if let Some(c) = &mut choice.condition {
        remap_expr(c, map);
    }
    for content in [
        &mut choice.start_content,
        &mut choice.choice_only_content,
        &mut choice.inner_content,
    ]
    .into_iter()
    .flatten()
    {
        remap_content(content, map);
    }
    if let Some(e) = &mut choice.display_emission {
        remap_emission(e, map);
    }
    if let Some(e) = &mut choice.output_emission {
        remap_emission(e, map);
    }
    remap_tags(&mut choice.tags, map);
}

fn remap_conditional(cond: &mut lir::Conditional, map: &[NameId]) {
    if let lir::CondKind::Switch(e) = &mut cond.kind {
        remap_expr(e, map);
    }
    for branch in &mut cond.branches {
        if let Some(c) = &mut branch.condition {
            remap_expr(c, map);
        }
        remap_stmts(&mut branch.body, map);
    }
}

fn remap_sequence(seq: &mut lir::Sequence, map: &[NameId]) {
    for branch in &mut seq.branches {
        remap_stmts(branch, map);
    }
}

fn remap_emission(emission: &mut lir::ContentEmission, map: &[NameId]) {
    if let lir::RecognizedLine::Template { slot_exprs, .. } = &mut emission.line {
        for e in slot_exprs {
            remap_expr(e, map);
        }
    }
    remap_tags(&mut emission.tags, map);
}

fn remap_content(content: &mut lir::Content, map: &[NameId]) {
    remap_content_parts(&mut content.parts, map);
    remap_tags(&mut content.tags, map);
}

fn remap_tags(tags: &mut [Vec<lir::ContentPart>], map: &[NameId]) {
    for tag in tags {
        remap_content_parts(tag, map);
    }
}

fn remap_content_parts(parts: &mut [lir::ContentPart], map: &[NameId]) {
    use lir::ContentPart;
    for part in parts {
        match part {
            ContentPart::Interpolation(e) => remap_expr(e, map),
            ContentPart::InlineConditional(cond) => remap_conditional(cond, map),
            ContentPart::InlineSequence(seq) => remap_sequence(seq, map),
            ContentPart::Text(_)
            | ContentPart::Glue
            | ContentPart::Spring
            | ContentPart::EnterSequence(_) => {}
        }
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "exhaustive per-variant Expr walk — one arm per variant is the point"
)]
fn remap_expr(expr: &mut lir::Expr, map: &[NameId]) {
    use lir::Expr;
    match expr {
        Expr::GetTemp(_, name) | Expr::TakeTemp(_, name) => relocate(name, map),
        Expr::String(s) => remap_string(s, map),
        Expr::Prefix(_, e) | Expr::Postfix(e, _) => remap_expr(e, map),
        Expr::Infix(l, _, r) => {
            remap_expr(l, map);
            remap_expr(r, map);
        }
        Expr::Call { target: _, args }
        | Expr::CallVariable { target: _, args }
        | Expr::CallExternal {
            target: _, args, ..
        } => remap_call_args(args, map),
        Expr::CallVariableTemp {
            slot: _,
            name,
            args,
        } => {
            relocate(name, map);
            remap_call_args(args, map);
        }
        Expr::CallBuiltin { builtin: _, args } => {
            for e in args {
                remap_expr(e, map);
            }
        }
        Expr::MakeFnValue { target: _, bound } => remap_call_args(bound, map),
        Expr::CallValue { callee, args } | Expr::BindValue { callee, args } => {
            remap_expr(callee, map);
            for e in args {
                remap_expr(e, map);
            }
        }
        Expr::ArrayNew(elems) => {
            for e in elems {
                remap_expr(e, map);
            }
        }
        Expr::MapNew(pairs) => {
            for (k, v) in pairs {
                remap_expr(k, map);
                remap_expr(v, map);
            }
        }
        Expr::Index { base, index } => {
            remap_expr(base, map);
            remap_expr(index, map);
        }
        Expr::IndexSet { base, index, value } => {
            remap_expr(base, map);
            remap_expr(index, map);
            remap_expr(value, map);
        }
        Expr::CollectionLen(e)
        | Expr::CollectionKeys(e)
        | Expr::CollectionValues(e)
        | Expr::ConvertInt(e)
        | Expr::ConvertFloat(e)
        | Expr::ConvertString(e) => {
            remap_expr(e, map);
        }
        Expr::CollectionContains { container, needle } => {
            remap_expr(container, map);
            remap_expr(needle, map);
        }
        Expr::CollectionInsert { base, key, value } => {
            remap_expr(base, map);
            remap_expr(key, map);
            remap_expr(value, map);
        }
        Expr::CollectionRemove { base, key } => {
            remap_expr(base, map);
            remap_expr(key, map);
        }
        Expr::RecordNew {
            shape_id: _,
            fields,
            prelude,
        } => {
            for e in fields {
                remap_expr(e, map);
            }
            for (_slot, name, e) in prelude {
                relocate(name, map);
                remap_expr(e, map);
            }
        }
        Expr::RecordGet {
            base,
            field,
            static_offset: _,
        } => {
            remap_expr(base, map);
            relocate(field, map);
        }
        Expr::RecordSet {
            base,
            field,
            static_offset: _,
            value,
        } => {
            remap_expr(base, map);
            relocate(field, map);
            remap_expr(value, map);
        }
        // No name references.
        Expr::Int(_)
        | Expr::Float(_)
        | Expr::Bool(_)
        | Expr::Null
        | Expr::GetGlobal(_)
        | Expr::TakeGlobal(_)
        | Expr::VisitCount(_)
        | Expr::DivertTarget(_)
        | Expr::ListLiteral { .. }
        | Expr::ConstLiteral(_) => {}
    }
}

fn remap_string(s: &mut lir::StringExpr, map: &[NameId]) {
    for part in &mut s.parts {
        if let lir::StringPart::Interpolation(e) = part {
            remap_expr(e, map);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_format::NameId;

    #[test]
    fn relocate_maps_local_to_assembled() {
        let map = vec![NameId(5), NameId(2), NameId(9)];
        let mut id = NameId(1);
        relocate(&mut id, &map);
        assert_eq!(id, NameId(2));
    }

    #[test]
    fn remap_rewrites_nested_name_ids() {
        // Local names [greeting, field] -> assembled [7, 3].
        let map = vec![NameId(7), NameId(3)];
        let mut expr = lir::Expr::Infix(
            Box::new(lir::Expr::GetTemp(0, NameId(0))),
            crate::InfixOp::Add,
            Box::new(lir::Expr::RecordGet {
                base: Box::new(lir::Expr::GetTemp(1, NameId(0))),
                field: NameId(1),
                static_offset: None,
            }),
        );
        remap_expr(&mut expr, &map);
        let (l, r) = match &expr {
            lir::Expr::Infix(l, _, r) => Some((l, r)),
            _ => None,
        }
        .expect("expected infix");
        assert!(matches!(**l, lir::Expr::GetTemp(0, NameId(7))));
        let (base, field) = match &**r {
            lir::Expr::RecordGet { base, field, .. } => Some((base, field)),
            _ => None,
        }
        .expect("expected record get");
        assert!(matches!(**base, lir::Expr::GetTemp(1, NameId(7))));
        assert_eq!(*field, NameId(3));
    }

    #[test]
    fn assemble_dedups_against_existing_table() {
        let mut names = NameTable::new();
        let pre = names.intern("existing"); // NameId(0)

        let chunk = ScopeChunk::root_content(
            vec![lir::Stmt::DeclareTemp {
                slot: 0,
                name: NameId(1),                               // local -> "fresh"
                value: Some(lir::Expr::GetTemp(0, NameId(0))), // local -> "existing"
            }],
            Vec::new(),
            vec!["existing".to_string(), "fresh".to_string()],
        );

        let (mut body, children) = assemble_scopes(vec![chunk], &mut names);
        assert!(children.is_empty());

        // "existing" deduped to its prior id; "fresh" got a new one after it.
        let entries = names.into_entries();
        assert_eq!(entries, vec!["existing".to_string(), "fresh".to_string()]);
        assert_eq!(pre, NameId(0));

        let (name, value) = match body.remove(0) {
            lir::Stmt::DeclareTemp { name, value, .. } => Some((name, value)),
            _ => None,
        }
        .expect("expected declare temp");
        assert_eq!(name, NameId(1)); // "fresh"
        assert!(matches!(value, Some(lir::Expr::GetTemp(0, NameId(0))))); // "existing"
    }
}
