//! B0.1 exit-criterion tests for the opaque-provenance seam (issue #1148,
//! `docs/hir-admission-contract.md` Q1(b)/D1/§4.3).
//!
//! Two guarantees are pinned here:
//!
//! 1. **Resolver round-trip** — ink node → `Provenance` → live node: every
//!    provenance the ink lowering stamps resolves back (through
//!    `InkProvenanceResolver`, by value, including a provenance
//!    reconstructed from serialized parts) to the exact node it was
//!    stamped from.
//! 2. **Headless compiles never resolve provenance** — garbling every
//!    frontend-private half of every provenance in a lowered `HirFile`
//!    (the `file` and the raw kind — everything a resolver would need)
//!    while keeping the pipeline-visible halves (class + range) leaves
//!    analysis, LIR, and codegen output byte-identical. This is contract
//!    §4.3's "a headless compile never resolves ptrs", proven rather than
//!    assumed — and it is exactly the property that lets a native frontend
//!    ship codegen before native IDE support.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::items_after_statements,
    clippy::match_same_arms
)]
#![allow(
    clippy::disallowed_types,
    reason = "always-empty file_paths map handed to lower_to_program, no order to leak"
)]

use brink_ir::hir::{InkProvenanceResolver, lower, normalize_file};
use brink_ir::provenance::{KindToken, NodeClass, Provenance, ProvenanceResolver};
use brink_ir::{FileId, HirFile, SymbolKind, SymbolManifest, hir};
use brink_syntax::ast::AstNode;

// ─── Fixture ────────────────────────────────────────────────────────

/// A fixture exercising a wide spread of provenance-stamped node kinds:
/// a promoted top-level stitch, declarations, a knot with params, a
/// nested stitch, choices, a conditional, a sequence, diverts, a tunnel,
/// a thread, tags, and logic lines.
const FIXTURE: &str = "\
INCLUDE other.ink
VAR gold = 10
CONST LIMIT = 3
LIST moods = happy, (sad)
= floating
Floating stitch content.
-> DONE
== start(x) ==
Hello there. # first_tag
~ temp y = x + 1
~ gold = gold - 1
{gold > 5:
  Rich.
- else:
  Poor.
}
{&one|two|three}
* [Option A] -> mid
* [Option B] -> start.inner
= inner
Inner content.
-> mid ->
<- side
-> DONE
== mid ==
Mid.
->->
== side ==
Side thread.
-> DONE
== function double(n) ==
~ return n * 2
";

fn lower_fixture() -> (brink_syntax::SyntaxNode, HirFile, SymbolManifest) {
    let parsed = brink_syntax::parse(FIXTURE);
    let tree = parsed.tree();
    let root = tree.syntax().clone();
    let (hir, manifest, _diags) = lower(FileId(0), &tree);
    (root, hir, manifest)
}

// ─── 1. Resolver round-trip ─────────────────────────────────────────

#[test]
fn every_container_and_decl_provenance_round_trips() {
    let (root, hir, _) = lower_fixture();
    let resolver = InkProvenanceResolver::new(FileId(0), &root);

    let mut checked = 0usize;
    let mut check = |p: Provenance, what: &str| {
        let node = resolver
            .resolve(p)
            .unwrap_or_else(|| panic!("{what}: provenance {p:?} did not resolve"));
        assert_eq!(node.text_range(), p.range, "{what}: resolved wrong node");
        checked += 1;
    };

    for knot in &hir.knots {
        check(knot.ptr, "knot");
        for stitch in &knot.stitches {
            check(stitch.ptr, "stitch");
        }
    }
    for v in &hir.variables {
        check(v.ptr, "var");
    }
    for c in &hir.constants {
        check(c.ptr, "const");
    }
    for l in &hir.lists {
        check(l.ptr, "list");
    }
    for inc in &hir.includes {
        check(inc.ptr, "include");
    }
    assert!(
        checked >= 8,
        "fixture should exercise many nodes: {checked}"
    );
}

#[test]
fn body_statement_provenance_round_trips() {
    let (root, hir, _) = lower_fixture();
    let resolver = InkProvenanceResolver::new(FileId(0), &root);

    // Walk every statement in every body and round-trip each provenance
    // the walk can see. Every stamped provenance must resolve to a node
    // with the identical range.
    let mut seen = Vec::new();
    for knot in &hir.knots {
        collect_block(&knot.body, &mut seen);
        for stitch in &knot.stitches {
            collect_block(&stitch.body, &mut seen);
        }
    }
    collect_block(&hir.root_content, &mut seen);

    assert!(
        seen.len() >= 10,
        "fixture should stamp many statement provenances: {}",
        seen.len()
    );
    for p in seen {
        let node = resolver
            .resolve(p)
            .unwrap_or_else(|| panic!("statement provenance {p:?} did not resolve"));
        assert_eq!(node.text_range(), p.range);
    }
}

/// Collect the statement-level provenance a body walk can reach (enough
/// breadth for the round-trip guarantee; the exhaustive per-field walk
/// lives in the garbling test below).
fn collect_block(block: &hir::Block, out: &mut Vec<Provenance>) {
    for stmt in &block.stmts {
        match stmt {
            hir::Stmt::Content(c) => {
                out.extend(c.ptr);
                for tag in &c.tags {
                    out.push(tag.ptr);
                }
            }
            hir::Stmt::Divert(d) => out.extend(d.ptr),
            hir::Stmt::TunnelCall(t) => out.push(t.ptr),
            hir::Stmt::ThreadStart(t) => out.push(t.ptr),
            hir::Stmt::TempDecl(t) => out.push(t.ptr),
            hir::Stmt::Assignment(a) => out.push(a.ptr),
            hir::Stmt::Return(r) => out.extend(r.ptr),
            hir::Stmt::ChoiceSet(cs) => {
                for choice in &cs.choices {
                    out.push(choice.ptr);
                    collect_block(&choice.body, out);
                }
                collect_block(&cs.continuation, out);
            }
            hir::Stmt::LabeledBlock(b) => collect_block(b, out),
            hir::Stmt::Conditional(c) => {
                out.push(c.ptr);
                for b in &c.branches {
                    out.push(b.ptr);
                    collect_block(&b.body, out);
                }
            }
            hir::Stmt::Sequence(s) => {
                out.push(s.ptr);
                for b in &s.branches {
                    out.push(b.ptr);
                    collect_block(&b.body, out);
                }
            }
            hir::Stmt::LogicBlock(lb) => out.push(lb.ptr),
            hir::Stmt::ExprStmt(_) | hir::Stmt::EndOfLine | hir::Stmt::Await(_) => {}
        }
    }
}

#[test]
fn promoted_floating_stitch_keeps_stitch_class() {
    let (root, hir, _) = lower_fixture();
    let resolver = InkProvenanceResolver::new(FileId(0), &root);

    // The `= floating` top-level stitch is promoted to a Knot node but must
    // keep NodeClass::Stitch (the former ContainerPtr::Stitch role, F-I#5 /
    // the #626 floating-stitch trap) and index as SymbolKind::Stitch.
    let floating = hir
        .knots
        .iter()
        .find(|k| k.name.text == "floating")
        .expect("promoted floating stitch");
    assert_eq!(floating.ptr.class(), NodeClass::Stitch);
    assert_eq!(floating.symbol_kind(), SymbolKind::Stitch);
    // And it still resolves to the live stitch-def node.
    assert!(resolver.resolve(floating.ptr).is_some());

    // A real knot carries NodeClass::Knot and indexes as SymbolKind::Knot.
    let start = hir.knots.iter().find(|k| k.name.text == "start").unwrap();
    assert_eq!(start.ptr.class(), NodeClass::Knot);
    assert_eq!(start.symbol_kind(), SymbolKind::Knot);
}

#[test]
fn typed_resolution_finds_the_include_statement() {
    let (root, hir, _) = lower_fixture();
    let resolver = InkProvenanceResolver::new(FileId(0), &root);

    let inc = &hir.includes[0];
    let stmt: brink_syntax::ast::IncludeStmt = resolver
        .resolve_ast(inc.ptr)
        .expect("typed include resolution");
    assert!(stmt.syntax().text().to_string().contains("other.ink"));
}

#[test]
fn resolution_is_keyed_by_value_not_by_minting_session() {
    // A resolver must accept any well-formed Provenance value — including
    // one reconstructed from serialized parts it never minted (the future
    // debugger path: bytecode offset → stored parts → resolver).
    let (root, hir, _) = lower_fixture();
    let resolver = InkProvenanceResolver::new(FileId(0), &root);

    let original = hir.knots[1].ptr;
    let rebuilt = Provenance::new(
        FileId(original.file.0),
        original.range,
        KindToken::from_u32(original.kind.as_u32()).unwrap(),
    );
    assert_eq!(original, rebuilt);
    assert_eq!(
        resolver.resolve(original).map(|n| n.text_range()),
        resolver.resolve(rebuilt).map(|n| n.text_range()),
        "reconstructed provenance must resolve identically to the original"
    );
    assert!(resolver.resolve(rebuilt).is_some());
}

#[test]
fn foreign_synthetic_and_stale_provenance_resolve_to_none() {
    let (root, hir, _) = lower_fixture();
    let resolver = InkProvenanceResolver::new(FileId(0), &root);
    let real = hir.knots[1].ptr;

    // Another file's provenance: None.
    let foreign = Provenance::new(FileId(7), real.range, real.kind);
    assert!(resolver.resolve(foreign).is_none());

    // Synthetic provenance: None (no frontend claims the synthetic raw).
    let synthetic = Provenance::synthetic(real.class(), real.range);
    assert!(resolver.resolve(synthetic).is_none());

    // Stale provenance (tree changed → no node at that kind+range): None,
    // never a panic — resolution failure is a normal answer.
    let reparsed = brink_syntax::parse("Completely different.\n");
    let new_root = reparsed.tree().syntax().clone();
    let stale_resolver = InkProvenanceResolver::new(FileId(0), &new_root);
    assert!(stale_resolver.resolve(real).is_none());
}

// ─── 2. Headless compile never resolves provenance ──────────────────

/// Garble the frontend-private halves (`file`, raw kind) of a provenance,
/// keeping the pipeline-visible halves (class, range) intact.
fn garble(p: &mut Provenance) {
    *p = Provenance {
        file: FileId(u32::MAX),
        range: p.range,
        kind: KindToken::synthetic(p.kind.class),
    };
}

fn garble_opt(p: &mut Option<Provenance>) {
    if let Some(p) = p {
        garble(p);
    }
}

/// Exhaustive provenance-garbling walk over a `HirFile`. Every match is
/// exhaustive on purpose: a new node kind or field must show up here (as a
/// compile error) so it gets a garbling rule and stays inside the §4.3
/// guarantee.
fn garble_file(hir: &mut HirFile) {
    let HirFile {
        root_content,
        knots,
        variables,
        constants,
        lists,
        structs,
        externals,
        includes,
        module: _,
        imports: _,
        visibility: _,
        was_directives: _,
    } = hir;
    garble_block(root_content);
    for knot in knots {
        garble(&mut knot.ptr);
        garble_block(&mut knot.body);
        for stitch in &mut knot.stitches {
            garble(&mut stitch.ptr);
            garble_block(&mut stitch.body);
        }
    }
    for v in variables {
        garble(&mut v.ptr);
        garble_expr(&mut v.value);
    }
    for c in constants {
        garble(&mut c.ptr);
        garble_expr(&mut c.value);
    }
    for l in lists {
        garble(&mut l.ptr);
    }
    for s in structs {
        garble(&mut s.ptr);
    }
    for e in externals {
        garble(&mut e.ptr);
    }
    for i in includes {
        garble(&mut i.ptr);
    }
}

fn garble_divert_target(target: &mut hir::DivertTarget) {
    for arg in &mut target.args {
        garble_expr(arg);
    }
}

fn garble_block(block: &mut hir::Block) {
    for stmt in &mut block.stmts {
        garble_stmt(stmt);
    }
}

fn garble_stmt(stmt: &mut hir::Stmt) {
    match stmt {
        hir::Stmt::Content(c) => garble_content(c),
        hir::Stmt::Divert(d) => {
            garble_opt(&mut d.ptr);
            garble_divert_target(&mut d.target);
        }
        hir::Stmt::TunnelCall(t) => {
            garble(&mut t.ptr);
            for target in &mut t.targets {
                garble_divert_target(target);
            }
        }
        hir::Stmt::ThreadStart(t) => {
            garble(&mut t.ptr);
            garble_divert_target(&mut t.target);
        }
        hir::Stmt::TempDecl(t) => {
            garble(&mut t.ptr);
            if let Some(v) = &mut t.value {
                garble_expr(v);
            }
        }
        hir::Stmt::Assignment(a) => {
            garble(&mut a.ptr);
            garble_expr(&mut a.target);
            garble_expr(&mut a.value);
        }
        hir::Stmt::Return(r) => {
            garble_opt(&mut r.ptr);
            if let Some(v) = &mut r.value {
                garble_expr(v);
            }
            for e in &mut r.onwards_args {
                garble_expr(e);
            }
        }
        hir::Stmt::ChoiceSet(cs) => {
            for choice in &mut cs.choices {
                garble(&mut choice.ptr);
                if let Some(cond) = &mut choice.condition {
                    garble_expr(cond);
                }
                for content in [
                    &mut choice.start_content,
                    &mut choice.bracket_content,
                    &mut choice.inner_content,
                ]
                .into_iter()
                .flatten()
                {
                    garble_content(content);
                }
                for tag in &mut choice.tags {
                    garble(&mut tag.ptr);
                }
                garble_block(&mut choice.body);
            }
            garble_block(&mut cs.continuation);
        }
        hir::Stmt::LabeledBlock(b) => garble_block(b),
        hir::Stmt::Conditional(c) => garble_conditional(c),
        hir::Stmt::Sequence(s) => garble_sequence(s),
        hir::Stmt::ExprStmt(e) => garble_expr(e),
        hir::Stmt::EndOfLine => {}
        hir::Stmt::LogicBlock(lb) => {
            garble(&mut lb.ptr);
            for bs in &mut lb.stmts {
                garble_block_stmt(bs);
            }
        }
        hir::Stmt::Await(a) => {
            garble(&mut a.ptr);
            if let Some(cond) = &mut a.condition {
                garble_expr(cond);
            }
        }
    }
}

fn garble_content(c: &mut hir::Content) {
    garble_opt(&mut c.ptr);
    for part in &mut c.parts {
        garble_content_part(part);
    }
    for tag in &mut c.tags {
        garble(&mut tag.ptr);
        for part in &mut tag.parts {
            garble_content_part(part);
        }
    }
}

fn garble_content_part(part: &mut hir::ContentPart) {
    match part {
        hir::ContentPart::Text(_) | hir::ContentPart::Glue | hir::ContentPart::Spring => {}
        hir::ContentPart::Interpolation(e) => garble_expr(e),
        hir::ContentPart::InlineConditional(c) => garble_conditional(c),
        hir::ContentPart::InlineSequence(s) => garble_sequence(s),
    }
}

fn garble_conditional(c: &mut hir::Conditional) {
    garble(&mut c.ptr);
    if let hir::CondKind::Switch(e) = &mut c.kind {
        garble_expr(e);
    }
    for branch in &mut c.branches {
        garble(&mut branch.ptr);
        if let Some(cond) = &mut branch.condition {
            garble_expr(cond);
        }
        garble_block(&mut branch.body);
    }
}

fn garble_sequence(s: &mut hir::Sequence) {
    garble(&mut s.ptr);
    for branch in &mut s.branches {
        garble(&mut branch.ptr);
        garble_block(&mut branch.body);
    }
}

fn garble_block_stmt(bs: &mut hir::BlockStmt) {
    match bs {
        hir::BlockStmt::TempDecl(t) => {
            garble(&mut t.ptr);
            if let Some(v) = &mut t.value {
                garble_expr(v);
            }
        }
        hir::BlockStmt::Assignment(a) => {
            garble(&mut a.ptr);
            garble_expr(&mut a.target);
            garble_expr(&mut a.value);
        }
        hir::BlockStmt::Return(r) => {
            garble_opt(&mut r.ptr);
            if let Some(v) = &mut r.value {
                garble_expr(v);
            }
            for e in &mut r.onwards_args {
                garble_expr(e);
            }
        }
        hir::BlockStmt::If(i) => garble_if(i),
        hir::BlockStmt::While(w) => {
            garble(&mut w.ptr);
            garble_expr(&mut w.condition);
            for bs in &mut w.body {
                garble_block_stmt(bs);
            }
        }
        hir::BlockStmt::For(f) => {
            garble(&mut f.ptr);
            garble_expr(&mut f.iterable);
            for bs in &mut f.body {
                garble_block_stmt(bs);
            }
        }
        hir::BlockStmt::Break(p) | hir::BlockStmt::Continue(p) => garble(p),
        hir::BlockStmt::ExprStmt(e) => garble_expr(e),
        hir::BlockStmt::Await(a) => {
            garble(&mut a.ptr);
            if let Some(cond) = &mut a.condition {
                garble_expr(cond);
            }
        }
    }
}

fn garble_if(i: &mut hir::IfStmt) {
    garble(&mut i.ptr);
    garble_expr(&mut i.condition);
    for bs in &mut i.body {
        garble_block_stmt(bs);
    }
    match &mut i.else_branch {
        Some(hir::ElseBranch::ElseIf(nested)) => garble_if(nested),
        Some(hir::ElseBranch::Else(stmts)) => {
            for bs in stmts {
                garble_block_stmt(bs);
            }
        }
        None => {}
    }
}

fn garble_expr(e: &mut hir::Expr) {
    match e {
        hir::Expr::Int(_)
        | hir::Expr::Float(_)
        | hir::Expr::Bool(_)
        | hir::Expr::Null
        | hir::Expr::Path(_)
        | hir::Expr::DivertTarget(_)
        | hir::Expr::ListLiteral(_) => {}
        hir::Expr::String(s) => {
            for part in &mut s.parts {
                if let hir::StringPart::Interpolation(inner) = part {
                    garble_expr(inner);
                }
            }
        }
        hir::Expr::Prefix(_, inner) => garble_expr(inner),
        hir::Expr::Infix(ie) => {
            garble(&mut ie.ptr);
            garble_expr(&mut ie.lhs);
            garble_expr(&mut ie.rhs);
        }
        hir::Expr::Postfix(inner, _) => garble_expr(inner),
        hir::Expr::Call(_, args) => {
            for a in args {
                garble_expr(a);
            }
        }
        hir::Expr::ArrayLiteral(al) => {
            garble(&mut al.ptr);
            for el in &mut al.elements {
                garble_expr(el);
            }
        }
        hir::Expr::MapLiteral(ml) => {
            garble(&mut ml.ptr);
            for (k, v) in &mut ml.entries {
                garble_expr(k);
                garble_expr(v);
            }
        }
        hir::Expr::Index(ix) => {
            garble(&mut ix.ptr);
            garble_expr(&mut ix.base);
            garble_expr(&mut ix.index);
        }
        hir::Expr::Range(r) => {
            garble(&mut r.ptr);
            garble_expr(&mut r.start);
            garble_expr(&mut r.end);
        }
        hir::Expr::StructLiteral(sl) => {
            garble(&mut sl.ptr);
            for (_, v) in &mut sl.fields {
                garble_expr(v);
            }
        }
        hir::Expr::FieldAccess(fa) => {
            garble(&mut fa.ptr);
            garble_expr(&mut fa.base);
        }
        hir::Expr::FnLiteral(fl) => {
            garble(&mut fl.ptr);
            for a in &mut fl.args {
                garble_expr(a);
            }
        }
        hir::Expr::RefArg(ra) => {
            garble(&mut ra.ptr);
            garble_expr(&mut ra.operand);
        }
    }
}

fn compile_headless(hir: &HirFile, manifest: &SymbolManifest) -> (Vec<String>, StoryDataRepr) {
    let file_id = FileId(0);
    let files_for_analysis: Vec<(FileId, &HirFile, &SymbolManifest)> =
        vec![(file_id, hir, manifest)];
    let result = brink_analyzer::analyze(&files_for_analysis);

    let files_for_lir: Vec<(FileId, &HirFile)> = vec![(file_id, hir)];
    let (program, lir_diags) = brink_ir::lir::lower_to_program(
        &files_for_lir,
        &result.index,
        &result.resolutions,
        &std::collections::HashMap::new(),
    );
    let story = brink_codegen_inkb::emit(&program.expect("fixture compiles")).expect("codegen");

    let mut diags: Vec<String> = result
        .diagnostics
        .iter()
        .chain(lir_diags.iter())
        .map(|d| format!("{d:?}"))
        .collect();
    diags.sort();
    (diags, story)
}

type StoryDataRepr = brink_format::StoryData;

#[test]
fn headless_compile_is_identical_with_unresolvable_provenance() {
    let parsed = brink_syntax::parse(FIXTURE);
    let tree = parsed.tree();
    let (hir, manifest, _diags) = lower(FileId(0), &tree);

    let mut pristine = hir.clone();
    normalize_file(&mut pristine);

    let mut garbled = hir;
    garble_file(&mut garbled);
    normalize_file(&mut garbled);

    let (diags_a, story_a) = compile_headless(&pristine, &manifest);
    let (diags_b, story_b) = compile_headless(&garbled, &manifest);

    assert_eq!(
        diags_a, diags_b,
        "analysis/LIR diagnostics must not depend on frontend-private provenance"
    );
    assert_eq!(
        story_a, story_b,
        "compiled StoryData must not depend on frontend-private provenance"
    );
}
