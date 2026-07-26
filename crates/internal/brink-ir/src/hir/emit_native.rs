//! `HirFile` → `.brink` source: a faithful pretty-printer over lowered HIR
//! (issue #1178, `docs/b0-sequencing.md` §B0.8b).
//!
//! This is deliberately **shared machinery**, not a one-off converter
//! script: the future `.brink` formatter and printer-based IDE rewrites
//! (folding, refactors) are the other planned consumers of "take an
//! `HirFile` and produce source text for it" — the 2026-07-19 evening
//! ruling (NF-5) that spawned this issue explicitly named that as the
//! reason the old charter §8.5 converter deferral no longer holds.
//!
//! # Scope — only currently-supported constructs
//!
//! The native surface (`brink_syntax_native` + [`super::lower_native`]) does
//! not yet lower every `Stmt`/`Expr` shape the shared HIR types can carry
//! (see `lower_native`'s own module docs for the current judgment calls and
//! gaps). This emitter is **conservative by construction**: every
//! `emit_*` function returns [`EmitError::Unsupported`] the moment it meets
//! a shape it cannot faithfully respell, rather than guessing or emitting
//! partial/lossy text. [`emit_file`] is all-or-nothing — a single
//! unsupported node anywhere in the tree fails the whole call. This is the
//! "never emit invalid syntax" rule from the issue: an `Err` is always
//! preferable to a best-effort guess that might not round-trip.
//!
//! Supported today: `var`/`const`/`flags`/`struct`/`extern` top-level
//! declarations; `flow`/`fn` knots with one level of nested `flow`
//! stitches (native's own Q4(b) two-level fence — `Stitch` has no further
//! nesting field, so this is not a self-imposed limit); params (bare,
//! `ref`, type-annotated); content lines (text, glue, `{expr}`
//! interpolation, trailing tags); diverts (`-> target`, `-> END`,
//! `-> DONE`, with call args); tunnel calls (`-> target ->`); `return` /
//! `return -> target`; `{?}` choice points (sticky/once, guards, labels,
//! the `text[bracket]inner` display split, `else {}` fallback, the
//! dissolved-gather continuation — including a **labeled** continuation,
//! and a mid-flow `Stmt::LabeledBlock` wherever it occurs, both via G-1's
//! `(name)` content-line-label spelling, see the "Labeled lines" section
//! above); `{if cond {} else {}}` conditionals (`CondKind::InitialCondition`
//! only — native's `if`/`else` grammar has no `else if` chain, so
//! `CondKind::IfElse` has no native spelling, see `lower_native::cond`'s
//! own finding); `{match subj {}}` (`CondKind::Switch`).
//!
//! Explicitly unsupported (each a real gap, not an oversight — see
//! `docs/b0-sequencing.md` §3 and `tests/tier1-brink-respell/README.md`'s
//! own gap findings for the native-grammar context): `Stmt::TempDecl`/
//! `Assignment`/`ExprStmt`/`LogicBlock`/`Await` (code-dialect ground, a
//! different keyword set this slice didn't need);
//! `Stmt::Sequence`/`ContentPart::InlineSequence`/`InlineConditional`
//! (alternations `~`/`&`/`!`/`|` — not exercised by this slice's target
//! corpus, deferred rather than risked untested); `Stmt::ThreadStart`
//! (a splice `<- flow(args)` has a ruled spelling *inside* a `{?}` choice
//! point, but only as a sibling of the choice lines around it — the HIR
//! flattens it into an ordinary preceding/trailing statement with no
//! marker of that original nesting, so re-nesting it correctly would need
//! more than this slice's scope; still refused, not guessed); most `Expr`
//! variants beyond literals/paths/operators/calls (collections, structs,
//! refs, `#fn`); any knot/stitch/decl directive channel (`is_local`,
//! `#@effects`, `#@was`, visibility, doc comments); `IncludeSite`,
//! `ModuleDecl`, `Import`, file-level
//! `VisibilityDirective`/`was_directives`.
//!
//! Type annotations are **supported** in every position the native grammar
//! spells them (NG-A/B/C, issues #1487/#1488/#1489): parameters
//! (`fn f(g: Guest)`), `var`/`const` bindings, and a `flow`/`fn` header's
//! `: type` return clause. They used to be listed above as an unsupported
//! channel — correct while native had no annotation grammar at all, but a
//! `.brink` file can now *contain* them, so refusing to spell them back
//! would make legal native source un-round-trippable.

use std::fmt::Write as _;

use crate::{
    Block, Choice, ChoiceSet, CondBranch, CondKind, Conditional, ConstDecl, Content, ContentPart,
    DivertPath, DivertTarget, Expr, ExternalDecl, HirFile, InfixOp, Knot, ListDecl, Name, Param,
    Path, PostfixOp, PrefixOp, Return, Stitch, Stmt, StringPart, StructDecl, Tag, TypeExpr,
    VarDecl,
};

// ─── Labeled lines (G-1) ─────────────────────────────────────────────
//
// `tests/tier1-brink-respell/README.md`'s G-1 finding ("no ruled spelling
// for a labeled mid-flow re-entry point") was ruled 2026-07-20 ("label
// any content line", landed in `brink-syntax-native`'s
// `content::at_content_label`/`label`): every content line now takes an
// optional leading `(name)` — the native spelling for both a mid-flow
// labeled re-entry point
// (`Stmt::LabeledBlock`, `lower_native::body::lower_items`'s
// label-absorption) and a labeled dissolved-gather continuation
// (`ChoiceSet.continuation.label`, `lower_native::body::lower_continuation`).
// Neither construct is a distinct `Stmt` shape on the way back out — the
// label decorates whichever line the absorbed stream's *first* item
// happens to render as, exactly mirroring how `lower_content_line_body`
// built it going in. [`emit_labeled_stmt_stream`] is the shared reverse:
// it recognizes the same handful of leading shapes
// `lower_content_line_body`/`lower_content_run` can produce for a single
// content line (content alone, content sharing its line with a divert or
// single-hop tunnel call) and refuses — never guesses — anything else.

/// Why [`emit_file`] refused to produce source text. Always fatal to the
/// whole call — see the module doc's "never emit invalid syntax" rule.
#[derive(Debug, Clone, thiserror::Error)]
pub enum EmitError {
    /// A construct with no known (or no faithful) native spelling.
    /// `what` names the construct; `context` is a short breadcrumb (e.g.
    /// the enclosing knot's name) since HIR nodes carry no stable text
    /// location an emit-time error can point at cheaply.
    #[error("unsupported for native emission: {what} ({context})")]
    Unsupported { what: &'static str, context: String },
    /// `root_content` is non-empty and needs wrapping in a synthesized
    /// `flow main() { … }` (the native story-entry convention,
    /// `lower_native::entry_root_content`'s doc), but a top-level `flow`/
    /// `fn` named `main` already exists — emitting both would either
    /// collide or silently drop one. Not fabricated a rename; surfaced.
    #[error(
        "root content needs a synthesized `flow main()`, but a top-level `main` already exists"
    )]
    RootMainCollision,
}

fn unsupported(what: &'static str, context: impl Into<String>) -> EmitError {
    EmitError::Unsupported {
        what,
        context: context.into(),
    }
}

/// Recognize `lower_native::entry_root_content`'s synthesized shape: a
/// `root_content` consisting of exactly one bare `-> main` divert, matching
/// a real top-level, zero-param, non-`fn` `main` knot. See this function's
/// call site for why that shape must not be re-wrapped.
fn is_synthetic_main_entry(hir: &HirFile) -> bool {
    let [Stmt::Divert(d)] = hir.root_content.stmts.as_slice() else {
        return false;
    };
    let DivertPath::Path(p) = &d.target.path else {
        return false;
    };
    if !d.target.args.is_empty() {
        return false;
    }
    let [seg] = p.segments.as_slice() else {
        return false;
    };
    if seg.text != "main" {
        return false;
    }
    hir.knots
        .iter()
        .any(|k| k.name.text == "main" && k.params.is_empty() && !k.is_function)
}

/// Emit a complete `.brink` source file from `hir`.
///
/// All-or-nothing: the first unsupported construct anywhere in the tree
/// fails the whole call (see the module doc). Ordering is canonical, not
/// source-position-preserving — `variables`, `constants`, `lists`
/// (`flags`), `structs`, `externals`, then `knots` (each knot's own body
/// content precedes its nested stitches, since `Knot.body`/`Knot.stitches`
/// are separate fields with no shared interleaving order to reconstruct).
/// A non-empty `root_content` is wrapped in a synthesized top-level
/// `flow main() { … }` (see [`EmitError::RootMainCollision`]).
pub fn emit_file(hir: &HirFile) -> Result<String, EmitError> {
    if !hir.includes.is_empty() {
        return Err(unsupported("INCLUDE sites", "file"));
    }
    if hir.module.is_some() {
        return Err(unsupported("#@module directive", "file"));
    }
    if !hir.imports.is_empty() {
        return Err(unsupported("import/use statements", "file"));
    }
    if !hir.visibility.is_empty() || !hir.was_directives.is_empty() {
        return Err(unsupported(
            "file-level visibility/#@was directives",
            "file",
        ));
    }

    let mut out = String::new();
    let mut wrote_any = false;

    for v in &hir.variables {
        emit_var_decl(&mut out, v)?;
        wrote_any = true;
    }
    if wrote_any {
        out.push('\n');
    }

    let before = out.len();
    for c in &hir.constants {
        emit_const_decl(&mut out, c)?;
    }
    if out.len() != before {
        out.push('\n');
        wrote_any = true;
    }

    let before = out.len();
    for l in &hir.lists {
        emit_flags_decl(&mut out, l)?;
    }
    if out.len() != before {
        out.push('\n');
        wrote_any = true;
    }

    let before = out.len();
    for s in &hir.structs {
        emit_struct_decl(&mut out, s)?;
        out.push('\n');
    }
    if out.len() != before {
        wrote_any = true;
    }

    let before = out.len();
    for e in &hir.externals {
        emit_external_decl(&mut out, e)?;
    }
    if out.len() != before {
        out.push('\n');
        wrote_any = true;
    }

    let mut knots: Vec<&Knot> = hir.knots.iter().collect();
    let synthetic_main;
    // `lower_native::entry_root_content` synthesizes `root_content` as a
    // single `-> main` divert whenever a top-level, zero-param, non-`fn`
    // `main` knot exists (the native story-entry convention, charter §15)
    // — that shape is *already* representable by the real `main` knot
    // alone once re-lowered, so it must not be re-wrapped (a native
    // fixture round-tripping through this emitter would otherwise always
    // hit `RootMainCollision`, since `main` legitimately exists both as a
    // knot and as this synthesized entry). Only a *genuine* non-empty
    // root content (ink's literal pre-first-knot weave) needs wrapping.
    if !hir.root_content.stmts.is_empty() && !is_synthetic_main_entry(hir) {
        if knots
            .iter()
            .any(|k| k.name.text == "main" && k.params.is_empty())
        {
            return Err(EmitError::RootMainCollision);
        }
        let empty_range = rowan::TextRange::empty(rowan::TextSize::from(0));
        synthetic_main = Knot {
            ptr: crate::provenance::Provenance::synthetic(
                crate::provenance::NodeClass::Knot,
                empty_range,
            ),
            name: Name {
                text: "main".to_string(),
                range: empty_range,
            },
            is_function: false,
            params: Vec::new(),
            body: hir.root_content.clone(),
            stitches: Vec::new(),
            is_local: false,
            effects_assertion: None,
            return_type: None,
            doc: None,
            visibility: None,
            was: None,
        };
        knots.insert(0, &synthetic_main);
    }

    for (i, knot) in knots.iter().enumerate() {
        if i > 0 {
            out.push('\n');
        }
        emit_knot(&mut out, knot)?;
    }
    let _ = wrote_any;

    Ok(out)
}

// ─── Top-level declarations ─────────────────────────────────────────

fn emit_var_decl(out: &mut String, v: &VarDecl) -> Result<(), EmitError> {
    if v.is_local || v.doc.is_some() || v.visibility.is_some() || v.was.is_some() {
        return Err(unsupported("var directive channel", &v.name.text));
    }
    let ty = emit_binding_annotation(v.annotation.as_ref(), &v.name.text)?;
    let value = emit_expr(&v.value, &v.name.text)?;
    let _ = writeln!(out, "var {}{ty} = {value}", v.name.text);
    Ok(())
}

fn emit_const_decl(out: &mut String, c: &ConstDecl) -> Result<(), EmitError> {
    if c.doc.is_some() || c.visibility.is_some() || c.was.is_some() {
        return Err(unsupported("const directive channel", &c.name.text));
    }
    let ty = emit_binding_annotation(c.annotation.as_ref(), &c.name.text)?;
    let value = emit_expr(&c.value, &c.name.text)?;
    let _ = writeln!(out, "const {}{ty} = {value}", c.name.text);
    Ok(())
}

/// `": T"` for an annotated binding, `""` for an unannotated one — the
/// exact text that goes between a `var`/`const` name and its `=` (NG-B,
/// issue #1488).
fn emit_binding_annotation(
    annotation: Option<&TypeExpr>,
    context: &str,
) -> Result<String, EmitError> {
    annotation.map_or_else(
        || Ok(String::new()),
        |ty| Ok(format!(": {}", emit_type(ty, context)?)),
    )
}

fn emit_flags_decl(out: &mut String, l: &ListDecl) -> Result<(), EmitError> {
    if l.doc.is_some() || l.visibility.is_some() || l.was.is_some() {
        return Err(unsupported("flags directive channel", &l.name.text));
    }
    let members: Vec<String> = l
        .members
        .iter()
        .map(|m| {
            let mut s = m.name.text.clone();
            if let Some(v) = m.value {
                let _ = write!(s, " = {v}");
            }
            if m.is_active { format!("({s})") } else { s }
        })
        .collect();
    let _ = writeln!(out, "flags {} = {}", l.name.text, members.join(", "));
    Ok(())
}

fn emit_struct_decl(out: &mut String, s: &StructDecl) -> Result<(), EmitError> {
    if s.doc.is_some() || s.visibility.is_some() {
        return Err(unsupported("struct directive channel", &s.name.text));
    }
    let _ = writeln!(out, "struct {} {{", s.name.text);
    for f in &s.fields {
        let ty = emit_type(&f.ty, &s.name.text)?;
        let _ = writeln!(out, "  {}: {ty}", f.name.text);
    }
    let _ = writeln!(out, "}}");
    Ok(())
}

fn emit_external_decl(out: &mut String, e: &ExternalDecl) -> Result<(), EmitError> {
    if e.doc.is_some() || e.visibility.is_some() || e.was.is_some() {
        return Err(unsupported("extern directive channel", &e.name.text));
    }
    let params: Vec<&str> = e.params.iter().map(|p| p.name.as_str()).collect();
    let _ = writeln!(out, "extern {}({})", e.name.text, params.join(", "));
    Ok(())
}

fn emit_type(ty: &TypeExpr, context: &str) -> Result<String, EmitError> {
    match ty {
        TypeExpr::Named { name, .. } => Ok(name.clone()),
        TypeExpr::Generic { name, args, .. } => {
            let rendered: Result<Vec<String>, EmitError> =
                args.iter().map(|a| emit_type(a, context)).collect();
            Ok(format!("{name}<{}>", rendered?.join(", ")))
        }
        TypeExpr::Fn { .. } => Err(unsupported("fn(...) type expression", context)),
    }
}

fn emit_param(p: &Param, context: &str) -> Result<String, EmitError> {
    if p.is_divert {
        return Err(unsupported("divert-typed parameter", context));
    }
    let mut s = String::new();
    if p.is_ref {
        s.push_str("ref ");
    }
    s.push_str(&p.name.text);
    if let Some(ty) = &p.annotation {
        let _ = write!(s, ": {}", emit_type(ty, context)?);
    }
    Ok(s)
}

fn emit_params(params: &[Param], context: &str) -> Result<String, EmitError> {
    let rendered: Result<Vec<String>, EmitError> =
        params.iter().map(|p| emit_param(p, context)).collect();
    Ok(rendered?.join(", "))
}

// ─── Containers ──────────────────────────────────────────────────────

fn emit_knot(out: &mut String, k: &Knot) -> Result<(), EmitError> {
    if k.is_local
        || k.effects_assertion.is_some()
        || k.doc.is_some()
        || k.visibility.is_some()
        || k.was.is_some()
    {
        return Err(unsupported("knot directive/doc channel", &k.name.text));
    }
    let keyword = if k.is_function { "fn" } else { "flow" };
    let params = emit_params(&k.params, &k.name.text)?;
    // The ruled `: type` return clause goes after the parameter list
    // (NG-C, issue #1489) — never before the `(`, and never as an arrow.
    let ret = emit_binding_annotation(k.return_type.as_ref(), &k.name.text)?;
    let _ = writeln!(out, "{keyword} {}({params}){ret} {{", k.name.text);
    emit_block_stmts(out, &k.body, 1, &k.name.text)?;
    for s in &k.stitches {
        emit_stitch(out, s, 1)?;
    }
    let _ = writeln!(out, "}}");
    Ok(())
}

fn emit_stitch(out: &mut String, s: &Stitch, depth: usize) -> Result<(), EmitError> {
    if s.is_local
        || s.effects_assertion.is_some()
        || s.doc.is_some()
        || s.visibility.is_some()
        || s.was.is_some()
    {
        return Err(unsupported("stitch directive/doc channel", &s.name.text));
    }
    let indent = "  ".repeat(depth);
    let params = emit_params(&s.params, &s.name.text)?;
    let _ = writeln!(out, "{indent}flow {}({params}) {{", s.name.text);
    emit_block_stmts(out, &s.body, depth + 1, &s.name.text)?;
    let _ = writeln!(out, "{indent}}}");
    Ok(())
}

// ─── Statements ──────────────────────────────────────────────────────

/// Emit a block's statement stream at `depth` levels of two-space indent.
///
/// `Stmt::ChoiceSet` is the one shape that isn't a simple one-line-per-stmt
/// mapping: per the dissolved-gather model (`lower_native::body`'s module
/// doc), a `{?}` choice point always absorbs everything lexically after it
/// into its own `continuation` — so on the way back out, the continuation's
/// statements are flattened back into the *same* line stream immediately
/// after the closing `}`, not nested inside another block.
fn emit_block_stmts(
    out: &mut String,
    block: &Block,
    depth: usize,
    context: &str,
) -> Result<(), EmitError> {
    if block.label.is_some() {
        return Err(unsupported("labeled block", context));
    }
    emit_stmt_stream(out, &block.stmts, depth, context)
}

fn emit_stmt_stream(
    out: &mut String,
    stmts: &[Stmt],
    depth: usize,
    context: &str,
) -> Result<(), EmitError> {
    let indent = "  ".repeat(depth);
    let mut i = 0;
    while i < stmts.len() {
        let stmt = &stmts[i];
        match stmt {
            Stmt::EndOfLine => {}
            // A `Content` with no `Stmt::EndOfLine` immediately after it
            // (checked by peeking ahead, not by anything on `Content`
            // itself — the marker is structural, sitting in the stream)
            // means the source had a divert/tunnel-call on the *same*
            // physical line right after the text
            // (`lower_content_run`'s interior `flush_content(..., false)`
            // before an embedded `DIVERT_STMT`/`TUNNEL_CALL`, e.g. `Bye.
            // -> b`). Emitting them as two separate output lines would
            // silently insert a real line break the source never had —
            // re-lowering would then see a genuine trailing
            // `Stmt::EndOfLine` after the content that the original
            // never carried, an observable behavior change (an extra
            // newline in play output), not just a formatting choice.
            Stmt::Content(c) => {
                let same_line_divert = match stmts.get(i + 1) {
                    Some(Stmt::Divert(d)) => Some(emit_divert_target(&d.target, context)?),
                    Some(Stmt::TunnelCall(t)) if t.targets.len() == 1 => Some(format!(
                        "{} ->",
                        emit_divert_target(&t.targets[0], context)?
                    )),
                    _ => None,
                };
                if let Some(divert_text) = same_line_divert {
                    let text = emit_content_parts(&c.parts, context)?;
                    if !c.tags.is_empty() {
                        return Err(unsupported(
                            "tags on a content line sharing its line with a divert",
                            context,
                        ));
                    }
                    let _ = writeln!(out, "{indent}{text}-> {divert_text}");
                    i += 2;
                    continue;
                }
                emit_content_line(out, &indent, c, context)?;
            }
            Stmt::Divert(d) => {
                let target = emit_divert_target(&d.target, context)?;
                let _ = writeln!(out, "{indent}-> {target}");
            }
            Stmt::TunnelCall(t) => {
                if t.targets.len() != 1 {
                    return Err(unsupported("multi-hop tunnel chain", context));
                }
                let target = emit_divert_target(&t.targets[0], context)?;
                let _ = writeln!(out, "{indent}-> {target} ->");
            }
            Stmt::Return(r) => {
                let line = emit_return(r, context)?;
                let _ = writeln!(out, "{indent}{line}");
            }
            Stmt::ChoiceSet(cs) => {
                emit_choice_set(out, &indent, depth, cs, context)?;
                // The continuation's statements are the rest of *this*
                // stream, flattened in place — see this function's doc. A
                // labeled continuation (a gather `(name)` immediately after
                // the `{?}`, per `lower_native::body::lower_continuation`)
                // spells with G-1's `(name)` content-line-label prefix on
                // the continuation's own first line — see
                // `emit_labeled_stmt_stream`.
                match &cs.continuation.label {
                    Some(label) => {
                        emit_labeled_stmt_stream(
                            out,
                            label,
                            &cs.continuation.stmts,
                            depth,
                            context,
                        )?;
                    }
                    None => {
                        emit_stmt_stream(out, &cs.continuation.stmts, depth, context)?;
                    }
                }
                return Ok(());
            }
            Stmt::Conditional(cond) => emit_conditional(out, &indent, depth, cond, context)?,
            Stmt::LabeledBlock(b) => {
                // G-1's other absorption shape: a `(name)` label on an
                // ordinary mid-flow content line (`lower_items`'s own
                // label-absorption arm) wraps everything after it in a
                // `LabeledBlock` that is *always* the last statement of
                // whatever stream produced it (`lower_items` returns
                // immediately after pushing it) — never nested, always
                // flattened back into the surrounding stream. Same
                // `(name)`-prefix-on-the-first-line spelling as a labeled
                // continuation, via the same shared helper.
                let Some(label) = &b.label else {
                    // Not reachable from native lowering (a `LabeledBlock`
                    // is only ever constructed with a label) — refuse
                    // rather than silently emitting an unlabeled block as
                    // if it were an ordinary flattened stream.
                    return Err(unsupported("labeled block without a label", context));
                };
                emit_labeled_stmt_stream(out, label, &b.stmts, depth, context)?;
                return Ok(());
            }
            Stmt::Sequence(_) => return Err(unsupported("alternation sequence", context)),
            Stmt::TempDecl(_) => return Err(unsupported("temp declaration", context)),
            Stmt::Assignment(_) => return Err(unsupported("assignment", context)),
            Stmt::ExprStmt(_) => return Err(unsupported("expression statement", context)),
            Stmt::ThreadStart(_) => return Err(unsupported("thread-start splice", context)),
            Stmt::LogicBlock(_) => return Err(unsupported("`~ { }` logic block", context)),
            Stmt::Await(_) => return Err(unsupported("await statement", context)),
        }
        i += 1;
    }
    Ok(())
}

/// Emit a statement stream whose *first* line carries an implicit
/// `(label)` prefix — the G-1 shape shared by [`Stmt::LabeledBlock`] and a
/// labeled `ChoiceSet` continuation (see this module's "Labeled lines"
/// section doc). `label` is not itself a statement; it decorates whatever
/// `lower_content_line_body`/`lower_content_run` produced for the labeled
/// line, so this recognizes exactly the leading shapes those functions can
/// produce for a single content line: content alone, or content sharing
/// its line with a divert/single-hop tunnel call (mirroring
/// `emit_stmt_stream`'s own `same_line_divert` peek), plus the empty-`stmts`
/// case (a bare label with no content on its own line and nothing after
/// it — `flush_content`'s empty-flush short-circuit means the labeled
/// line contributes no `Stmt` of its own when it has no text). A shape
/// this function doesn't recognize is refused, not guessed, per the
/// module's "never emit invalid/lossy syntax" rule.
fn emit_labeled_stmt_stream(
    out: &mut String,
    label: &Name,
    stmts: &[Stmt],
    depth: usize,
    context: &str,
) -> Result<(), EmitError> {
    let indent = "  ".repeat(depth);
    let mut head = format!("{indent}({})", label.text);

    match stmts {
        [Stmt::Content(c), Stmt::Divert(d), rest @ ..] => {
            if !c.tags.is_empty() {
                return Err(unsupported(
                    "tags on a labeled content line sharing its line with a divert",
                    context,
                ));
            }
            let text = emit_content_parts(&c.parts, context)?;
            if !text.is_empty() {
                let _ = write!(head, " {text}");
            }
            let target = emit_divert_target(&d.target, context)?;
            let _ = writeln!(out, "{head}-> {target}");
            emit_stmt_stream(out, rest, depth, context)
        }
        [Stmt::Content(c), Stmt::TunnelCall(t), rest @ ..] if t.targets.len() == 1 => {
            if !c.tags.is_empty() {
                return Err(unsupported(
                    "tags on a labeled content line sharing its line with a tunnel call",
                    context,
                ));
            }
            let text = emit_content_parts(&c.parts, context)?;
            if !text.is_empty() {
                let _ = write!(head, " {text}");
            }
            let target = emit_divert_target(&t.targets[0], context)?;
            let _ = writeln!(out, "{head}-> {target} ->");
            emit_stmt_stream(out, rest, depth, context)
        }
        [Stmt::Content(c), Stmt::EndOfLine, rest @ ..] => {
            let text = emit_content_parts(&c.parts, context)?;
            if !text.is_empty() {
                let _ = write!(head, " {text}");
            }
            for tag in &c.tags {
                let _ = write!(head, " #{}", emit_tag(tag, context)?);
            }
            let _ = writeln!(out, "{head}");
            emit_stmt_stream(out, rest, depth, context)
        }
        // The labeled line's own content was empty (`flush_content`'s
        // empty-flush short-circuit, `lower_native::body`: a bare `(name)`
        // with nothing else on its line produces no `Content`/`EndOfLine`
        // of its own) *and* nothing follows it at all — a label on the
        // very last line of its enclosing stream. A bare label line with
        // no body is still a faithful, if unusual, respelling.
        [] => {
            let _ = writeln!(out, "{head}");
            Ok(())
        }
        _ => Err(unsupported(
            "labeled line with an unsupported leading shape",
            context,
        )),
    }
}

fn emit_return(r: &Return, context: &str) -> Result<String, EmitError> {
    if !r.onwards_args.is_empty() {
        return Err(unsupported("tunnel-return onwards args", context));
    }
    match &r.value {
        None => Ok("return".to_string()),
        Some(Expr::DivertTarget(p)) => Ok(format!("return -> {}", emit_path(p))),
        Some(_) => Err(unsupported("return with a value expression", context)),
    }
}

fn emit_divert_target(t: &DivertTarget, context: &str) -> Result<String, EmitError> {
    let head = match &t.path {
        DivertPath::Path(p) => emit_path(p),
        DivertPath::Done => "DONE".to_string(),
        DivertPath::End => "END".to_string(),
    };
    if t.args.is_empty() {
        Ok(head)
    } else {
        let rendered: Result<Vec<String>, EmitError> =
            t.args.iter().map(|a| emit_expr(a, context)).collect();
        Ok(format!("{head}({})", rendered?.join(", ")))
    }
}

fn emit_path(p: &Path) -> String {
    p.segments
        .iter()
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(".")
}

fn emit_content_line(
    out: &mut String,
    indent: &str,
    c: &Content,
    context: &str,
) -> Result<(), EmitError> {
    let text = emit_content_parts(&c.parts, context)?;
    let mut line = format!("{indent}{text}");
    for tag in &c.tags {
        let _ = write!(line, " #{}", emit_tag(tag, context)?);
    }
    let _ = writeln!(out, "{line}");
    Ok(())
}

fn emit_tag(tag: &Tag, context: &str) -> Result<String, EmitError> {
    emit_content_parts(&tag.parts, context)
}

fn emit_content_parts(parts: &[ContentPart], context: &str) -> Result<String, EmitError> {
    let mut s = String::new();
    for part in parts {
        match part {
            ContentPart::Text(t) => s.push_str(t),
            ContentPart::Glue => s.push_str("<>"),
            ContentPart::Interpolation(e) => {
                let _ = write!(s, "{{{}}}", emit_expr(e, context)?);
            }
            ContentPart::Spring => return Err(unsupported("word-break spring", context)),
            ContentPart::InlineConditional(_) => {
                return Err(unsupported("inline conditional in content", context));
            }
            ContentPart::InlineSequence(_) => {
                return Err(unsupported("inline sequence in content", context));
            }
        }
    }
    Ok(s)
}

// ─── Choices ─────────────────────────────────────────────────────────

fn emit_choice_set(
    out: &mut String,
    indent: &str,
    depth: usize,
    cs: &ChoiceSet,
    context: &str,
) -> Result<(), EmitError> {
    let _ = writeln!(out, "{indent}{{?");
    for choice in &cs.choices {
        emit_choice(out, depth + 1, choice, context)?;
    }
    let _ = writeln!(out, "{indent}}}");
    Ok(())
}

fn emit_choice(out: &mut String, depth: usize, c: &Choice, context: &str) -> Result<(), EmitError> {
    let indent = "  ".repeat(depth);
    if !c.tags.is_empty() {
        return Err(unsupported("choice-line trailing tags", context));
    }

    if c.is_fallback {
        let _ = write!(out, "{indent}else ");
        emit_choice_body(out, depth, &c.body, context)?;
        return Ok(());
    }

    let marker = if c.is_sticky { "+" } else { "*" };
    let mut head = format!("{indent}{marker}");
    if let Some(cond) = &c.condition {
        let _ = write!(head, " {{if {}}}", emit_expr(cond, context)?);
    }
    if let Some(label) = &c.label {
        let _ = write!(head, " ({})", label.text);
    }
    if let Some(start) = &c.start_content {
        let text = emit_content_parts(&start.parts, context)?;
        if !text.is_empty() {
            let _ = write!(head, " {text}");
        }
    }
    let needs_brackets = c.bracket_content.is_some() || c.inner_content.is_some();
    if needs_brackets {
        let bracket = match &c.bracket_content {
            Some(b) => emit_content_parts(&b.parts, context)?,
            None => String::new(),
        };
        let _ = write!(head, "[{bracket}]");
        if let Some(inner) = &c.inner_content {
            let text = emit_content_parts(&inner.parts, context)?;
            head.push_str(&text);
        }
    }
    out.push_str(&head);

    // A labeled choice body has no faithful native spelling — same guard as
    // `emit_choice_body`'s fallback (`else {}`) path, applied here for
    // consistency on the non-fallback path.
    if c.body.label.is_some() {
        return Err(unsupported("labeled choice body", context));
    }

    // The choice body's own stmts always start with a structural
    // `Stmt::EndOfLine` marker (`lower_native::choice`'s own doc: "the
    // list-display/echoed-text boundary marker"), optionally preceded by a
    // `Divert`/`TunnelCall` pulled out of the bracket/inner text regions
    // (N-1: a divert immediately following `]` with no further text). See
    // this function's three-way dispatch below.
    let stmts = c.body.stmts.as_slice();
    match stmts {
        [] => {
            return Err(unsupported(
                "malformed choice body (no EndOfLine marker)",
                context,
            ));
        }
        [Stmt::EndOfLine] => {
            out.push('\n');
        }
        [Stmt::Divert(d), Stmt::EndOfLine] => {
            let target = emit_divert_target(&d.target, context)?;
            let _ = writeln!(out, " -> {target}");
        }
        [Stmt::TunnelCall(t), Stmt::EndOfLine] if t.targets.len() == 1 => {
            let target = emit_divert_target(&t.targets[0], context)?;
            let _ = writeln!(out, " -> {target} ->");
        }
        [Stmt::EndOfLine, rest @ ..] => {
            out.push(' ');
            emit_choice_body_stmts(out, depth, rest, context)?;
        }
        _ => {
            return Err(unsupported(
                "malformed choice body (no leading EndOfLine)",
                context,
            ));
        }
    }
    Ok(())
}

fn emit_choice_body(
    out: &mut String,
    depth: usize,
    body: &Block,
    context: &str,
) -> Result<(), EmitError> {
    if body.label.is_some() {
        return Err(unsupported("labeled choice/else body", context));
    }
    match body.stmts.as_slice() {
        [] => Err(unsupported(
            "malformed else body (no EndOfLine marker)",
            context,
        )),
        [Stmt::EndOfLine] => {
            out.push('\n');
            Ok(())
        }
        [Stmt::EndOfLine, rest @ ..] => emit_choice_body_stmts(out, depth, rest, context),
        _ => Err(unsupported(
            "malformed else body (no leading EndOfLine)",
            context,
        )),
    }
}

fn emit_choice_body_stmts(
    out: &mut String,
    depth: usize,
    rest: &[Stmt],
    context: &str,
) -> Result<(), EmitError> {
    let indent = "  ".repeat(depth);
    let _ = writeln!(out, "{{");
    emit_stmt_stream(out, rest, depth + 1, context)?;
    let _ = writeln!(out, "{indent}}}");
    Ok(())
}

// ─── Conditionals ────────────────────────────────────────────────────

fn emit_conditional(
    out: &mut String,
    indent: &str,
    depth: usize,
    cond: &Conditional,
    context: &str,
) -> Result<(), EmitError> {
    match &cond.kind {
        CondKind::InitialCondition => {
            if cond.branches.is_empty() || cond.branches.len() > 2 {
                return Err(unsupported("multi-branch conditional shape", context));
            }
            let first = &cond.branches[0];
            let Some(if_cond) = &first.condition else {
                return Err(unsupported(
                    "conditional with no leading condition",
                    context,
                ));
            };
            let _ = writeln!(out, "{indent}{{if {} {{", emit_expr(if_cond, context)?);
            emit_block_stmts(out, &first.body, depth + 1, context)?;
            if let Some(second) = cond.branches.get(1) {
                if second.condition.is_some() {
                    return Err(unsupported("`else if` chain", context));
                }
                let _ = writeln!(out, "{indent}}} else {{");
                emit_block_stmts(out, &second.body, depth + 1, context)?;
            }
            let _ = writeln!(out, "{indent}}}}}");
            Ok(())
        }
        CondKind::Switch(subject) => {
            let _ = writeln!(out, "{indent}{{match {} {{", emit_expr(subject, context)?);
            for branch in &cond.branches {
                emit_match_arm(out, depth + 1, branch, context)?;
            }
            let _ = writeln!(out, "{indent}}}}}");
            Ok(())
        }
        CondKind::IfElse => Err(unsupported(
            "IfElse conditional (no native `else if` chain)",
            context,
        )),
    }
}

fn emit_match_arm(
    out: &mut String,
    depth: usize,
    branch: &CondBranch,
    context: &str,
) -> Result<(), EmitError> {
    let Some(pattern) = &branch.condition else {
        return Err(unsupported(
            "match arm with no pattern (default arm)",
            context,
        ));
    };
    let indent = "  ".repeat(depth);
    let _ = writeln!(out, "{indent}{} => {{", emit_expr(pattern, context)?);
    emit_block_stmts(out, &branch.body, depth + 1, context)?;
    let _ = writeln!(out, "{indent}}}");
    Ok(())
}

// ─── Expressions ─────────────────────────────────────────────────────

fn emit_expr(e: &Expr, context: &str) -> Result<String, EmitError> {
    match e {
        Expr::Int(n) => Ok(n.to_string()),
        Expr::Float(f) => Ok(f.to_f64().to_string()),
        Expr::Bool(b) => Ok(b.to_string()),
        Expr::Path(p) => Ok(emit_path(p)),
        Expr::String(s) => {
            let mut out = String::from("\"");
            for part in &s.parts {
                match part {
                    StringPart::Literal(t) => out.push_str(t),
                    StringPart::Interpolation(inner) => {
                        let _ = write!(out, "{{{}}}", emit_expr(inner, context)?);
                    }
                }
            }
            out.push('"');
            Ok(out)
        }
        Expr::Prefix(op, inner) => {
            let op_str = match op {
                PrefixOp::Negate => "-",
                PrefixOp::Not => "not ",
            };
            Ok(format!("{op_str}{}", emit_expr(inner, context)?))
        }
        Expr::Infix(lhs, op, rhs) => {
            let op_str = infix_op_str(*op);
            Ok(format!(
                "{} {op_str} {}",
                emit_expr(lhs, context)?,
                emit_expr(rhs, context)?
            ))
        }
        Expr::Postfix(inner, op) => {
            let op_str = match op {
                PostfixOp::Increment => "++",
                PostfixOp::Decrement => "--",
            };
            Ok(format!("{}{op_str}", emit_expr(inner, context)?))
        }
        Expr::Call(path, args) => {
            let rendered: Result<Vec<String>, EmitError> =
                args.iter().map(|a| emit_expr(a, context)).collect();
            Ok(format!("{}({})", emit_path(path), rendered?.join(", ")))
        }
        Expr::Null => Err(unsupported("`null` literal", context)),
        Expr::DivertTarget(_) => Err(unsupported("divert-target-as-value expression", context)),
        Expr::ListLiteral(_) => Err(unsupported("list literal expression", context)),
        Expr::ArrayLiteral(_) => Err(unsupported("array sigil literal", context)),
        Expr::MapLiteral(_) => Err(unsupported("map sigil literal", context)),
        Expr::Index(_) => Err(unsupported("index expression", context)),
        Expr::Range(_) => Err(unsupported("range literal", context)),
        Expr::StructLiteral(_) => Err(unsupported("struct construction literal", context)),
        Expr::FieldAccess(_) => Err(unsupported("field access expression", context)),
        Expr::FnLiteral(_) => Err(unsupported("`#fn` literal", context)),
        Expr::RefArg(_) => Err(unsupported("`ref` argument expression", context)),
    }
}

fn infix_op_str(op: InfixOp) -> &'static str {
    match op {
        InfixOp::Add => "+",
        InfixOp::Sub => "-",
        InfixOp::Mul => "*",
        InfixOp::Div => "/",
        InfixOp::Mod => "%",
        InfixOp::Intersect => "^",
        InfixOp::Eq => "==",
        InfixOp::NotEq => "!=",
        InfixOp::Lt => "<",
        InfixOp::Gt => ">",
        InfixOp::LtEq => "<=",
        InfixOp::GtEq => ">=",
        InfixOp::And => "&&",
        InfixOp::Or => "||",
        InfixOp::Has => "?",
        InfixOp::HasNot => "!?",
        InfixOp::Coalesce => "or",
    }
}

#[cfg(test)]
#[expect(
    clippy::panic,
    reason = "test-only `let-else { panic!(...) }` assertions for concise failure messages"
)]
mod tests {
    use super::*;

    /// Parse + lower native `src`, then run it through [`emit_file`].
    fn lower_and_emit(src: &str) -> Result<String, EmitError> {
        let parse = brink_syntax_native::parse(src);
        let tree = parse.tree();
        let (hir, _manifest, diags) = crate::hir::lower_native::lower(crate::FileId(0), &tree);
        assert!(diags.is_empty(), "unexpected diagnostics: {diags:?}");
        emit_file(&hir)
    }

    /// Reparse + relower `src` and return the resulting `HirFile` (asserting
    /// a clean parse and lowering, the same discipline `lower_and_emit`
    /// uses). Used to check a round-trip lands on an equivalent HIR shape,
    /// not just "parses without error".
    fn reparse_and_lower(src: &str) -> crate::HirFile {
        let parse = brink_syntax_native::parse(src);
        assert!(
            parse.errors().is_empty(),
            "emitted source has parse errors: {:?}\n--- source ---\n{src}",
            parse.errors()
        );
        let tree = parse.tree();
        let (hir, _manifest, diags) = crate::hir::lower_native::lower(crate::FileId(0), &tree);
        assert!(
            diags.is_empty(),
            "emitted source has lowering diagnostics: {diags:?}\n--- source ---\n{src}"
        );
        hir
    }

    /// A `{?}` choice point followed by a labeled gather `(again)` attaches
    /// the label directly to `ChoiceSet::continuation.label`
    /// (`lower_native::body::lower_continuation`'s dissolved-gather
    /// convention — see `labeled_gather_after_choices_attaches_label_to_continuation`
    /// in `lower_native::tests`). G-1's `(name)` content-line-label spelling
    /// (RULED 2026-07-20) gives this a faithful native respelling: the
    /// emitter must round-trip it, not refuse it.
    #[test]
    fn labeled_gather_continuation_round_trips() {
        let src = "flow a() {\n  {?\n    * A.\n  }\n  (again)\n  Loop point.\n}\n";
        let emitted = lower_and_emit(src).expect("labeled gather continuation must now emit");
        assert!(
            emitted.contains("(again)"),
            "emitted source dropped the continuation label:\n{emitted}"
        );

        let hir = reparse_and_lower(&emitted);
        let body = &hir.knots[0].body;
        let Stmt::ChoiceSet(cs) = &body.stmts[0] else {
            panic!("expected ChoiceSet as the re-lowered body's first statement: {body:?}");
        };
        assert_eq!(
            cs.continuation.label.as_ref().map(|n| n.text.as_str()),
            Some("again"),
            "re-lowered continuation lost its label"
        );
    }

    /// A standalone labeled content line mid-flow (`(mid) Middle.`) lowers
    /// to `Stmt::LabeledBlock` (`lower_native::tests::
    /// standalone_labeled_content_line_becomes_labeled_block`). Same G-1
    /// spelling, same round-trip obligation.
    #[test]
    fn labeled_mid_flow_block_round_trips() {
        let src = "flow a() {\n  Intro.\n  (mid) Middle.\n  End.\n}\n";
        let emitted = lower_and_emit(src).expect("labeled mid-flow block must now emit");
        assert!(
            emitted.contains("(mid)"),
            "emitted source dropped the mid-flow label:\n{emitted}"
        );

        let hir = reparse_and_lower(&emitted);
        let body = &hir.knots[0].body;
        let labeled = body
            .stmts
            .iter()
            .find_map(|s| match s {
                Stmt::LabeledBlock(b) => Some(b),
                _ => None,
            })
            .expect("expected a LabeledBlock among the re-lowered body's statements");
        assert_eq!(labeled.label.as_ref().map(|n| n.text.as_str()), Some("mid"));
    }

    /// A gather label with no content at all on its own line, and nothing
    /// following it in its enclosing flow, has no `Stmt` to attach to
    /// (`flush_content`'s empty-flush short-circuit): the emitter must
    /// print a bare `(again)` line, with no dangling trailing space and no
    /// spurious refusal.
    #[test]
    fn bare_label_line_at_end_of_flow_has_no_trailing_space() {
        let src = "flow a() {\n  {?\n    * A.\n  }\n  (again)\n}\n";
        let emitted = lower_and_emit(src).expect("bare trailing label must now emit");
        assert!(
            emitted.lines().any(|l| l.trim() == "(again)"),
            "expected a bare `(again)` line with no trailing text:\n{emitted}"
        );
        reparse_and_lower(&emitted);
    }

    /// When the labeled line's own content is empty but real content
    /// follows on later lines (`(again)` alone, then `Loop point.` on the
    /// next line), `flush_content`'s empty-flush short-circuit means the
    /// label attaches directly onto that *following* content statement —
    /// there is no separate empty `Content`/`EndOfLine` pair to hold it.
    /// This collapses the label and the following text onto one output
    /// line, which is a faithful respelling (the original label line
    /// contributed no text of its own either way).
    #[test]
    fn label_with_empty_own_line_attaches_to_following_content() {
        let src = "flow a() {\n  {?\n    * A.\n  }\n  (again)\n  Loop point.\n}\n";
        let emitted = lower_and_emit(src).expect("labeled gather continuation must now emit");
        assert!(
            emitted.lines().any(|l| l.trim() == "(again) Loop point."),
            "expected the label to attach to the following content line:\n{emitted}"
        );
    }
}
