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
//! `Assignment`/`ExprStmt`/`LogicBlock`/`Await` at prose-body position
//! (code-dialect ground — `lower_native::body` never constructs any of
//! these outside a `~{ }` logic block, so this is a native-**grammar**
//! gap, not just an emission one: there is no bare `~ x = expr`-style
//! prose-body statement to round-trip yet, issue #1335's B0.8b sweep);
//! `Stmt::Sequence`/`ContentPart::InlineSequence`/`InlineConditional`
//! (alternations `~`/`&`/`!`/`|` — **unlike the code-dialect-ground gaps
//! above, this one is emitter-only, not native-grammar**: native's
//! `ALTERNATION_BLOCK` parses and `lower_native::body`/`expr` already
//! lower it to real `Sequence`/`InlineSequence`/`InlineConditional` HIR
//! today, per a #1335 B0.8b re-check — this emitter has simply never
//! grown the `emit_*` arm for it, a real, closeable, still-open follow-up
//! this slice didn't take on); `Stmt::ThreadStart`
//! (a splice `<- flow(args)` has a ruled spelling *inside* a `{?}` choice
//! point, but only as a sibling of the choice lines around it — the HIR
//! flattens it into an ordinary preceding/trailing statement with no
//! marker of that original nesting, so re-nesting it correctly would need
//! more than this slice's scope; still refused, not guessed);
//! `ContentPart::Spring` (the deferred word-break marker
//! `lower::choice::replace_trailing_ws_with_spring` produces — no native
//! token forces that same runtime-deferred-whitespace behavior, and
//! respelling it as a literal space would silently change what renders,
//! so it stays refused); `CondKind::IfElse` and a prose-body `return`
//! with a value expression (both native-grammar gaps too — `if`/`else`
//! has no `else if` chain and `RETURN_STMT` never carries a value at body
//! position, see `lower_native::cond`'s and `lower_native::body`'s own
//! findings); most `Expr` variants beyond literals/paths/operators/calls
//! (collections, structs, refs, `#fn`, a divert target used as a value);
//! any knot/stitch/decl directive channel (`is_local`, `#@effects`,
//! `#@was`, visibility, doc comments); `IncludeSite`, `ModuleDecl`,
//! file-level `VisibilityDirective`/`was_directives`; `@[allow(…)]`
//! suppression scopes (`HirFile::allow_scopes`, issue #1614/#1161 — the
//! scope carries only a `(range, codes)` fact with no pointer back to the
//! declaration it decorates, so there is no way to re-place the
//! annotation line from here; refused loudly rather than silently
//! dropping the suppression).
//!
//! Type annotations are **supported** in every position the native grammar
//! spells them (NG-A/B/C, issues #1487/#1488/#1489): parameters
//! (`fn f(g: Guest)`), `var`/`const` bindings, and a `flow`/`fn` header's
//! `: type` return clause. They used to be listed above as an unsupported
//! channel — correct while native had no annotation grammar at all, but a
//! `.brink` file can now *contain* them, so refusing to spell them back
//! would make legal native source un-round-trippable.
//!
//! `Import` (`use`/`import` declarations, M-2) is likewise **supported**
//! now: issues #1581/#1590 fixed `Import.module` to be the real
//! `::`-joined module name (not the ink-era `.`-joined, leaf-inclusive
//! string) and made an item alias actually take effect, so the shape this
//! emitter produces is exactly what `hir::lower_native::import` needs to
//! reconstruct it. It used to be listed above as an unsupported channel —
//! correct while `Import.module` couldn't match a real module name even
//! on a clean round-trip, but that is fixed upstream of this emitter now.

use std::fmt::Write as _;

use crate::{
    Block, Choice, ChoiceSet, CondBranch, CondKind, Conditional, ConstDecl, Content, ContentPart,
    DivertPath, DivertTarget, Expr, ExternalDecl, HirFile, Import, InfixOp, Knot, LambdaBody,
    ListDecl, Name, Param, Path, PostfixOp, PrefixOp, Return, Stitch, Stmt, StringPart, StructDecl,
    Tag, TypeExpr, VarDecl,
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

/// Refuse the file-level channels [`emit_file`] has no spelling for at all
/// (as opposed to the per-declaration channels each `emit_*` function
/// checks itself). Split out of [`emit_file`] purely to keep that function
/// under clippy's line-count lint — these checks have no data to
/// contribute to the emitted text either way.
fn refuse_unsupported_file_channels(hir: &HirFile) -> Result<(), EmitError> {
    if !hir.includes.is_empty() {
        return Err(unsupported("INCLUDE sites", "file"));
    }
    if hir.module.is_some() {
        return Err(unsupported("#@module directive", "file"));
    }
    if !hir.visibility.is_empty() || !hir.was_directives.is_empty() {
        return Err(unsupported(
            "file-level visibility/#@was directives",
            "file",
        ));
    }
    // `@[allow(Exxx, …)]` scopes (issue #1614/#1161) carry only a
    // `(range, codes)` fact — no pointer back to the declaration they
    // decorate — so there is no way to re-place the annotation line above
    // the right node from here. Refusing loudly beats the alternative of
    // silently dropping the suppression (a `.brink` round-trip that
    // quietly stops silencing a diagnostic the author explicitly allowed
    // is a correctness regression, not a formatting nit).
    if !hir.allow_scopes.is_empty() {
        return Err(unsupported("@[allow(…)] suppression scopes", "file"));
    }
    Ok(())
}

/// Emit a complete `.brink` source file from `hir`.
///
/// All-or-nothing: the first unsupported construct anywhere in the tree
/// fails the whole call (see the module doc). Ordering is canonical, not
/// source-position-preserving — `imports`, `variables`, `constants`,
/// `lists` (`flags`), `structs`, `externals`, then `knots` (each knot's
/// own body content precedes its nested stitches, since `Knot.body`/
/// `Knot.stitches` are separate fields with no shared interleaving order
/// to reconstruct). A non-empty `root_content` is wrapped in a
/// synthesized top-level `flow main() { … }` (see
/// [`EmitError::RootMainCollision`]).
pub fn emit_file(hir: &HirFile) -> Result<String, EmitError> {
    refuse_unsupported_file_channels(hir)?;

    let mut out = String::new();
    let mut wrote_any = false;

    let before = out.len();
    for import in &hir.imports {
        emit_import(&mut out, import);
    }
    if out.len() != before {
        out.push('\n');
        wrote_any = true;
    }

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
            element_annotation: None,
            style_annotation: None,
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
    let ty = emit_annotation_suffix(v.annotation.as_ref());
    let value = emit_expr(&v.value, &v.name.text)?;
    let _ = writeln!(out, "var {}{ty} = {value}", v.name.text);
    Ok(())
}

fn emit_const_decl(out: &mut String, c: &ConstDecl) -> Result<(), EmitError> {
    if c.doc.is_some() || c.visibility.is_some() || c.was.is_some() {
        return Err(unsupported("const directive channel", &c.name.text));
    }
    let ty = emit_annotation_suffix(c.annotation.as_ref());
    let value = emit_expr(&c.value, &c.name.text)?;
    let _ = writeln!(out, "const {}{ty} = {value}", c.name.text);
    Ok(())
}

/// `": T"` for an annotated binding or return clause, `""` when absent.
/// Used both for the text between a `var`/`const` name and its `=` (NG-B,
/// issue #1488) and, in [`emit_knot`], for a `fn`/`flow` header's `: type`
/// return clause after the parameter list (NG-C, issue #1489) — the
/// rendering is identical, only what follows the annotation differs.
fn emit_annotation_suffix(annotation: Option<&TypeExpr>) -> String {
    annotation.map_or_else(String::new, |ty| format!(": {}", emit_type(ty)))
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
        let ty = emit_type(&f.ty);
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

/// `Import.module`/`items`/`bare` now round-trip faithfully on their own
/// (issues #1581/#1590 fixed the `::`-joining and honored aliases, both
/// upstream of this emitter — the whole `Import` shape it produces is
/// exactly what `hir::lower_native::import` needs to reconstruct it), so
/// the emitter's own blanket refusal predates that fix, not a still-live
/// gap. `bare: false` is the qualified form (`import module`, no leaf
/// item — the same shape a single-segment `use module;` also produces, so
/// `import` is the canonical spelling for either origin, not a guess).
/// `bare: true` always uses the `use module::{items};` brace form, even
/// for a single item — `use module::item;`'s shorthand is a *parse*
/// convenience the grammar also accepts, but re-lowering the brace form
/// yields an identical `Import`, so there is no faithfulness reason to
/// prefer the shorthand here. Like [`emit_type`], every `Import` shape has
/// a faithful spelling, so this is infallible too.
///
/// `import`'s own grammar (`parser::decl::import_decl`) never consumes a
/// trailing `;` — unlike every other native declaration, and unlike `use`
/// (whose `;` is optional but recognized, `parser::decl::use_decl`'s own
/// doc) — so a `;` after `import module` is not part of the statement at
/// all; it would parse as a *second*, out-of-position top-level construct
/// and diagnose `E129`. Only `use`'s brace form gets one here.
fn emit_import(out: &mut String, import: &Import) {
    if import.bare {
        let items: Vec<String> = import
            .items
            .iter()
            .map(|item| match &item.alias {
                Some(alias) => format!("{} as {alias}", item.name),
                None => item.name.clone(),
            })
            .collect();
        let _ = writeln!(out, "use {}::{{{}}};", import.module, items.join(", "));
    } else {
        let _ = writeln!(out, "import {}", import.module);
    }
}

/// Every `TypeExpr` shape has a faithful native spelling (NG-A/B/C,
/// issues #1487/#1488/#1489), so unlike the rest of this module's
/// `emit_*` functions, this one is infallible — there is no
/// `EmitError::Unsupported` arm to grow here.
fn emit_type(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Named { name, .. } => name.clone(),
        TypeExpr::Generic { name, args, .. } => {
            let rendered: Vec<String> = args.iter().map(emit_type).collect();
            format!("{name}<{}>", rendered.join(", "))
        }
        TypeExpr::Fn { params, ret, .. } => {
            let rendered: Vec<String> = params.iter().map(emit_type).collect();
            format!("fn({}): {}", rendered.join(", "), emit_type(ret))
        }
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
        let _ = write!(s, ": {}", emit_type(ty));
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
        || k.element_annotation.is_some()
        || k.style_annotation.is_some()
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
    let ret = emit_annotation_suffix(k.return_type.as_ref());
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
        || s.element_annotation.is_some()
        || s.style_annotation.is_some()
        || s.doc.is_some()
        || s.visibility.is_some()
        || s.was.is_some()
    {
        return Err(unsupported("stitch directive/doc channel", &s.name.text));
    }
    let indent = "  ".repeat(depth);
    let params = emit_params(&s.params, &s.name.text)?;
    // The ruled `: type` return clause (NG-C, issue #1489, widened to
    // stitches by #1509) — same position as a top-level `flow`/`fn`'s, see
    // `emit_knot`.
    let ret = emit_annotation_suffix(s.return_type.as_ref());
    let _ = writeln!(out, "{indent}flow {}({params}){ret} {{", s.name.text);
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
        // A `Content`-leading shape none of the three arms above matched
        // (e.g. tags or trailing statements this function doesn't yet
        // recognize) — a real gap, refused rather than guessed.
        [Stmt::Content(_), ..] => Err(unsupported(
            "labeled line with an unsupported leading shape",
            context,
        )),
        // The labeled line's own content was empty (`flush_content`'s
        // empty-flush short-circuit, `lower_native::body`: a bare `(name)`
        // with nothing else on its line produces no `Content` of its own)
        // and whatever follows isn't a `Content` line either — most often a
        // `{?}` choice point sitting directly under the label with nothing
        // between them (`tests/tier1/choices/I093-default-choices`'s
        // `- (start)` immediately followed by choice lines), but this
        // covers any non-`Content` leading shape uniformly: a bare label
        // line, then the rest of the stream emitted normally at the same
        // depth. Subsumes the old all-consumed `[]` case (`emit_stmt_stream`
        // on an empty slice is a no-op), so it is folded in here rather
        // than kept as a separate arm.
        rest => {
            let _ = writeln!(out, "{head}");
            emit_stmt_stream(out, rest, depth, context)
        }
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
            ContentPart::Text(t) => s.push_str(&escape_content_text(t)),
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
            ContentPart::Span(span) => s.push_str(&emit_span(span, context)?),
        }
    }
    Ok(s)
}

/// `<name attr="v">…</name>`, or the self-closing `<name attr="v"/>` shape
/// when `children` is empty (the point-marker case, §8b.11) — the exact
/// inverse of `hir::lower_native::body::lower_span`.
fn emit_span(span: &crate::hir::SpanPart, context: &str) -> Result<String, EmitError> {
    let mut s = format!("<{}", span.name);
    for (name, value) in &span.attrs {
        let _ = write!(s, " {name}=\"{}\"", escape_attr_value(value));
    }
    if span.children.is_empty() {
        s.push_str("/>");
        return Ok(s);
    }
    s.push('>');
    s.push_str(&emit_content_parts(&span.children, context)?);
    let _ = write!(s, "</{}>", span.name);
    Ok(s)
}

/// Escape a content-line literal for round-trip through native `.brink`
/// source. `\` `<` `{` `#` are exactly the escape set §8d.6 rules final
/// (`parser::markup::escape`'s `is_escapable`) — each would otherwise
/// reopen a real construct on re-parse (`<b>` a span, `{name}` an
/// interpolation, `#tag` a tag, a bare `\` an invalid escape sequence).
/// This is the emit-side inverse: a HIR `Text` containing e.g. `<b>` used
/// to round-trip as literal text before the markup grammar existed (#1716)
/// — now it must be escaped, or it silently re-parses as a `SPAN`.
fn escape_content_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '\\' | '<' | '{' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

/// Escape a span attribute value for round-trip. `SPAN_ATTR_VALUE` shares
/// the lexer's string-mode token pair (`STRING_TEXT`/`STRING_ESCAPE`) with
/// ordinary string literals, whose only recognized escapes are `\n` `\t`
/// `\\` `\"` (`lexer::lex_string_token`) — and a raw, unescaped newline or
/// carriage return inside a quoted string terminates it early (the
/// lexer's unterminated-string recovery emits `NEWLINE` and closes the
/// string), so this escapes those too, not just the structurally-required
/// `"`/`\`.
fn escape_attr_value(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '"' => out.push_str("\\\""),
            '\n' => out.push_str("\\n"),
            '\t' => out.push_str("\\t"),
            _ => out.push(c),
        }
    }
    out
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
        // The same-line `Divert`/`TunnelCall` shape above (N-1) only covers
        // a choice body that is *exactly* that one statement — a gather-
        // style fold can still absorb further statements onto this same
        // choice after it (e.g. a trailing un-indented `->DONE` line with
        // no gather of its own, folded onto the last choice in a weave
        // that has no separate continuation to hold it — see
        // `tests/tier1/choices/varying-choice`), which the two-element
        // patterns above don't match. Braces are always a faithful
        // respelling of a choice body's full statement stream, divert
        // included, so fall back to the general block form rather than
        // refusing a shape the compact one-liner just can't reach.
        [Stmt::Divert(_) | Stmt::TunnelCall(_), Stmt::EndOfLine, ..] => {
            out.push(' ');
            emit_choice_body_stmts(out, depth, stmts, context)?;
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
        // Unlike a regular choice line, `else` has no inline-content region
        // of its own to embed a same-line divert in — the native grammar's
        // `choice_point` loop only recognizes `else` when immediately
        // followed by `{` (`parser::choice::choice_point`), so an `else`
        // whose body is a bare `-> target` (ink's classic "no visible
        // option" fallback choice, lowered with `is_fallback: true` and no
        // display text — see `tests/tier1/choices/I079-…`) must always
        // spell with braces, never the regular path's compact one-liner.
        [Stmt::Divert(_) | Stmt::TunnelCall(_), ..] => {
            emit_choice_body_stmts(out, depth, body.stmts.as_slice(), context)
        }
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
            // B1b (issue #1475): a template `{if}` can carry an `as`
            // binding — `{if EXPR as n: … else: …}` — and `AS_BINDING` is
            // proven valid *inside* the braced `{if EXPR as n { … }}` form
            // too (`brink-syntax-native::parser::tests::brace_family::
            // conditional_block_carries_an_as_binding_on_the_braced_form`),
            // so respelling it as a suffix on the condition head is a
            // faithful round-trip, not a guess at new grammar. Omitting it
            // would silently change what the respelled source means: the
            // arm's body could reference a binding that no longer exists.
            let binding_suffix = match &first.binding {
                Some(name) => format!(" as {}", name.text),
                None => String::new(),
            };
            let _ = writeln!(
                out,
                "{indent}{{if {}{binding_suffix} {{",
                emit_expr(if_cond, context)?
            );
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
        Expr::Infix(ie) => {
            let op_str = infix_op_str(ie.op);
            Ok(format!(
                "{} {op_str} {}",
                emit_expr(&ie.lhs, context)?,
                emit_expr(&ie.rhs, context)?
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
        // Lambdas (issue #1685): the expression-body form respells exactly
        // — pipes, optional param annotations, the ruled `:` return clause.
        // A *braced* body carries code-ground statements, which this
        // emitter has no printer for at all (`LogicBlock` is unsupported
        // for the same reason), so it refuses loudly rather than emitting a
        // body it cannot round-trip.
        Expr::Lambda(l) => {
            let params = emit_params(&l.params, context)?;
            let ret = emit_annotation_suffix(l.return_type.as_ref());
            match &l.body {
                LambdaBody::Expr(body) => {
                    Ok(format!("|{params}|{ret} {}", emit_expr(body, context)?))
                }
                LambdaBody::Block { .. } => Err(unsupported("lambda with a braced body", context)),
            }
        }
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

    /// A `fn(params…): ret` type annotation (e.g. `f: fn(int): bool`) is
    /// legal native source per
    /// `fn_type_annotation_parses_with_its_colon_return`
    /// (`brink-syntax-native::parser::tests::declaration`), so the emitter
    /// must spell it back rather than refuse — the module doc's "supported"
    /// claim would otherwise be false for this one `TypeExpr` shape.
    #[test]
    fn fn_type_annotation_round_trips() {
        // A `flow` (not `fn`) so the body stays prose-ground — `fn`'s
        // default code-ground body is a separate, pre-existing emission
        // gap (a whole code body lowers to one `Stmt::LogicBlock`, and
        // `LogicBlock` bodies aren't emittable yet); this test is only
        // about the `fn(...)` *type* annotation on the parameter.
        let src = "flow apply(f: fn(int): bool) {\n  Hello.\n}\n";
        let emitted = lower_and_emit(src).expect("fn(...) type annotation must now emit");
        assert!(
            emitted.contains("fn(int): bool"),
            "expected the emitted source to spell the fn(...) type back out:\n{emitted}"
        );

        let hir = reparse_and_lower(&emitted);
        let Some(annotation) = &hir.knots[0].params[0].annotation else {
            panic!("re-lowered param lost its type annotation");
        };
        assert!(
            matches!(annotation, TypeExpr::Fn { .. }),
            "expected a re-lowered TypeExpr::Fn, got {annotation:?}"
        );
    }

    /// #1509: a *nested* flow's `: type` return clause (`hir::Stitch::
    /// return_type`, widening NG-C's `Knot.return_type`) must round-trip
    /// through the emitter exactly like a top-level flow/fn's does
    /// (`emit_knot`'s own `ret` suffix) — `emit_stitch` used to omit it
    /// silently.
    #[test]
    fn stitch_return_type_round_trips() {
        let src = "flow garden() {\n  flow gate(): int {\n    Onward.\n  }\n}\n";
        let emitted = lower_and_emit(src).expect("stitch return type must now emit");
        assert!(
            emitted.contains("gate(): int"),
            "expected the emitted source to spell the stitch's return type back out:\n{emitted}"
        );

        let hir = reparse_and_lower(&emitted);
        match &hir.knots[0].stitches[0].return_type {
            Some(TypeExpr::Named { name, .. }) => assert_eq!(name, "int"),
            other => panic!("re-lowered stitch lost its return type: {other:?}"),
        }
    }

    /// B1b (issue #1475): a template `{if}` conditional's `as` binding must
    /// round-trip, not silently disappear. Before this fix
    /// `CondKind::InitialCondition`'s emission never read
    /// `CondBranch::binding`, so the respelled source dropped the `as l`
    /// suffix — a different-behavior HIR behind a green round-trip check,
    /// since the success arm's body still referenced `l`.
    #[test]
    fn conditional_as_binding_round_trips() {
        let src = "flow a() {\n  {if some(9) as l: number {l} else: nobody}\n}\n";
        let emitted = lower_and_emit(src).expect("`as` binding conditional must now emit");
        assert!(
            emitted.contains("as l"),
            "emitted source dropped the `as` binding:\n{emitted}"
        );

        let hir = reparse_and_lower(&emitted);
        let body = &hir.knots[0].body;
        let Stmt::Conditional(cond) = &body.stmts[0] else {
            panic!("expected Conditional as the re-lowered body's first statement: {body:?}");
        };
        assert_eq!(
            cond.branches[0].binding.as_ref().map(|n| n.text.as_str()),
            Some("l"),
            "re-lowered conditional lost its `as` binding"
        );
    }

    /// Issues #1581/#1590 fixed `Import.module` upstream of this emitter
    /// (real `::`-joined module names, honored aliases) — `emit_import`
    /// must now spell `use`/`import` back rather than refuse the whole
    /// file, covering both the qualified form and the bare form with an
    /// aliased and an unaliased item.
    #[test]
    fn use_and_import_round_trip() {
        let src = "import story::market\n\
                    use story::market::barter::haggle;\n\
                    use story::shop::{gold as g, silver};\n\n\
                    flow a() {\n  Hi.\n}\n";
        let emitted = lower_and_emit(src).expect("use/import must now emit");

        let hir = reparse_and_lower(&emitted);
        assert_eq!(hir.imports.len(), 3, "emitted source:\n{emitted}");

        assert!(!hir.imports[0].bare);
        assert_eq!(hir.imports[0].module, "story::market");
        assert!(hir.imports[0].items.is_empty());

        assert!(hir.imports[1].bare);
        assert_eq!(hir.imports[1].module, "story::market::barter");
        assert_eq!(hir.imports[1].items.len(), 1);
        assert_eq!(hir.imports[1].items[0].name, "haggle");
        assert_eq!(hir.imports[1].items[0].alias, None);

        assert!(hir.imports[2].bare);
        assert_eq!(hir.imports[2].module, "story::shop");
        assert_eq!(hir.imports[2].items.len(), 2);
        assert_eq!(hir.imports[2].items[0].name, "gold");
        assert_eq!(hir.imports[2].items[0].alias.as_deref(), Some("g"));
        assert_eq!(hir.imports[2].items[1].name, "silver");
        assert_eq!(hir.imports[2].items[1].alias, None);
    }

    /// Issue #1685: the `Expr::Lambda` arm added to `emit_expr` is
    /// reachable from a top-level `var` initializer
    /// (`a_lambda_lowers_in_a_top_level_var_initializer_too`,
    /// `crates/internal/brink-ir/tests/native_lambdas.rs`) — every other
    /// emit shape added to this file got a round-trip test right here
    /// (`fn_type_annotation_round_trips`, `stitch_return_type_round_trips`,
    /// `conditional_as_binding_round_trips`, `use_and_import_round_trip`),
    /// so the lambda arm needs one too, pinned on the trickiest spelling:
    /// an *annotated* expression body, where the ruled `:` return clause
    /// sits immediately before the unbraced body with no other separator
    /// (`|x: int|: int x + 1`) — the shape most likely to re-parse wrong.
    #[test]
    fn lambda_with_annotated_expr_body_round_trips() {
        let src = "var add = |x: int|: int x + 1\n";
        let emitted = lower_and_emit(src).expect("annotated lambda expr body must emit");
        assert!(
            emitted.contains("|x: int|: int"),
            "expected the emitted source to spell the param + return annotations back out:\n{emitted}"
        );

        let hir = reparse_and_lower(&emitted);
        let Expr::Lambda(lambda) = &hir.variables.first().expect("one var").value else {
            panic!(
                "re-lowered var initializer lost its lambda: {:?}",
                hir.variables
            );
        };
        assert_eq!(lambda.params.len(), 1);
        assert!(
            matches!(&lambda.params[0].annotation, Some(TypeExpr::Named { name, .. }) if name == "int"),
            "re-lowered lambda lost its param annotation: {:?}",
            lambda.params[0].annotation
        );
        assert!(
            matches!(&lambda.return_type, Some(TypeExpr::Named { name, .. }) if name == "int"),
            "re-lowered lambda lost its return annotation: {:?}",
            lambda.return_type
        );
        assert!(
            matches!(&lambda.body, LambdaBody::Expr(_)),
            "re-lowered lambda lost its expression body: {:?}",
            lambda.body
        );
    }

    /// Issue #1614/#1161: `HirFile::allow_scopes` carries a `(range,
    /// codes)` fact with no pointer back to the declaration it decorates —
    /// this emitter has no way to re-place the `@[allow(…)]` line, so a
    /// file that has one must refuse loudly rather than silently produce
    /// `.brink` text that has quietly stopped suppressing the diagnostic
    /// the original author explicitly allowed.
    #[test]
    fn allow_scope_is_refused_not_silently_dropped() {
        let src = "@[allow(E014)]\nvar gold = 0\n";
        let err = lower_and_emit(src).expect_err("an @[allow(…)] scope must not silently vanish");
        assert!(
            matches!(
                err,
                EmitError::Unsupported {
                    what: "@[allow(…)] suppression scopes",
                    ..
                }
            ),
            "expected the allow-scopes refusal, got {err:?}"
        );
    }

    /// A choice's body's own `Stmt` stream always starts `[divert?,
    /// EndOfLine, …body]` (`lower_native::choice::lower_choice`'s own
    /// preamble, unconditionally). A same-line divert (N-1) followed by a
    /// braced body reproduces the same three-element `[Divert, EndOfLine,
    /// Divert]` leading shape the ink-fed "gather-fold" case does (the one
    /// the old two-element `[Divert, EndOfLine]` pattern alone never
    /// covered) — this only checks the emit arm doesn't refuse it and
    /// produces re-parseable text carrying both diverts; exact re-lowered
    /// `Stmt` positions aren't asserted (the compact same-line divert
    /// becomes an ordinary braced-body line on the way back, which is a
    /// different-but-equally-faithful respelling, not a round-trip
    /// requirement this shape needs to meet byte-for-byte). See
    /// `brink-respell`'s `varying_choice` test for the real
    /// oracle-corpus fixture, proven episode-identical end to end, that
    /// motivated this fix.
    #[test]
    fn choice_body_with_trailing_statement_after_same_line_divert_round_trips() {
        let src = "flow a() {\n  {?\n    * Hop. -> b {\n      -> c\n    }\n  }\n}\n\
             flow b() {\n  Hi.\n}\n\
             flow c() {\n  Hey.\n}\n";
        let emitted =
            lower_and_emit(src).expect("a trailing statement after a same-line divert must emit");
        assert!(emitted.contains("-> b"), "{emitted}");
        assert!(emitted.contains("-> c"), "{emitted}");

        let hir = reparse_and_lower(&emitted);
        let Stmt::ChoiceSet(cs) = &hir.knots[0].body.stmts[0] else {
            panic!(
                "expected ChoiceSet as the re-lowered body's first statement: {:?}",
                hir.knots[0].body
            );
        };
        let target_names: Vec<String> = cs.choices[0]
            .body
            .stmts
            .iter()
            .filter_map(|s| match s {
                Stmt::Divert(d) => match &d.target.path {
                    DivertPath::Path(p) => Some(emit_path(p)),
                    _ => None,
                },
                _ => None,
            })
            .collect();
        assert_eq!(target_names, vec!["b".to_string(), "c".to_string()]);
    }

    /// A bare `(name)` label with nothing else on its own line, immediately
    /// followed by a `{?}` choice point — a `Stmt::LabeledBlock` whose
    /// *first* statement is a `ChoiceSet`, not `Content`
    /// (`tests/tier1/choices/default-choices`'s ` - (start)` is the real
    /// oracle-corpus shape this generalizes from — mechanically respelling
    /// it does emit and reparse cleanly now, but that fixture's ` - (start)`
    /// sits in *root* content, which needs `emit_file`'s synthesized
    /// `flow main() { … }` wrapper; that gives the gather a different
    /// qualified address path than the ink original and fails the
    /// differential on an unrelated addressing mismatch, not this fix —
    /// see the PR's own findings, not a `brink-respell` fixture, since it
    /// can't be made green without touching root-content addressing. This
    /// test isolates the same leading-shape fix inside an ordinary `flow`,
    /// with no root-content synthesis in the way).
    #[test]
    fn labeled_block_immediately_followed_by_choice_point_round_trips() {
        let src = "flow a() {\n  (start)\n  {?\n    *[Choice 1]\n    *[Choice 2]\n  }\n}\n";
        let emitted =
            lower_and_emit(src).expect("a label directly above a choice point must now emit");
        assert!(emitted.contains("(start)"), "{emitted}");

        let hir = reparse_and_lower(&emitted);
        let Stmt::LabeledBlock(b) = &hir.knots[0].body.stmts[0] else {
            panic!(
                "expected LabeledBlock as the re-lowered body's first statement: {:?}",
                hir.knots[0].body
            );
        };
        assert_eq!(b.label.as_ref().map(|n| n.text.as_str()), Some("start"));
        assert!(
            matches!(b.stmts.as_slice(), [Stmt::ChoiceSet(_)]),
            "expected the label's body to be exactly a ChoiceSet: {:?}",
            b.stmts
        );
    }
}
