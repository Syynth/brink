//! CST → HIR lowering for the native `.brink` surface's **declaration and
//! module-skeleton layer** (`docs/b0-sequencing.md` §B0.6, issue #1175).
//!
//! Scope, precisely: `flow`/`fn` declaration heads (→ `Knot`/`Stitch`,
//! params), `var`/`const`/`flags`/`struct`/`extern` (→ their HIR decl
//! nodes), `use`/`import` (→ `Import`). **Bodies are out of scope** — every
//! `Knot`/`Stitch`'s `body` is the empty stub `Block::default()`; prose
//! lowering is B0.7, code-statement lowering is B0.8. The `SymbolManifest`
//! is never hand-built here — [`crate::symbols::project_manifest`] (B0.4)
//! derives it from the `HirFile` this module produces, exactly like the ink
//! frontend's own [`crate::hir::lower::lower`] does. That is the whole
//! point of this slice: a second, independent frontend producing HIR that
//! satisfies the same admission contract (`docs/hir-admission-contract.md`)
//! the ink frontend does, without hand-building a manifest or reaching for
//! ink's `AstPtr`-based provenance.
//!
//! # Judgment calls (flagged for the coordinator / issue #1106)
//!
//! 1. **Crate home + pipeline plug-in shape.** This lowering lives inside
//!    `brink-ir` (a new `brink-syntax-native` dependency, sibling module to
//!    `hir::lower`) rather than a new crate or inside
//!    `brink-syntax-native` itself. Rationale: `brink-ir`'s own doc
//!    ("HIR ... and lowering") already frames it as the home for
//!    *whichever* frontend produces the CST, and it's the crate that owns
//!    [`crate::symbols::project_manifest`] and [`crate::provenance`] this
//!    lowering depends on — putting the lowering here avoids a dependency
//!    cycle (`brink-syntax-native` would otherwise need to depend on
//!    `brink-ir`, which owns nothing native-specific). **Not wired** into
//!    `brink-db`'s `parse_query`/`lowered_query` (F-B's admission point) —
//!    B0.6's gate is "native HIR passes admission," tested by calling this
//!    module directly (mirrors `b03_admission_corpus.rs`'s own direct
//!    `brink_ir::hir::lower` + `validate_admission` calls, bypassing
//!    `brink-db` entirely). Dialect dispatch at the `brink-db` seam,
//!    `.brink` file-extension registration, and project-layer discovery
//!    are explicitly B0.9/B0.10 per `docs/b0-sequencing.md` — wiring them
//!    here would be scope creep this slice didn't need to take on to meet
//!    its own exit criterion.
//! 2. **The `NativeProvenanceResolver` shape** ([`provenance`]) is a
//!    byte-for-byte mirror of [`crate::hir::InkProvenanceResolver`] against
//!    `brink-syntax-native`'s own `SyntaxKind`/`SyntaxNode` types — two
//!    independent implementations of the same trait, no shared resolution
//!    code between frontends (per Q1(b)'s "the ink frontend keeps `AstPtr`
//!    *behind* its resolver; native supplies its own"). The small
//!    duplication (the walk-up-to-matching-range algorithm) was judged
//!    cheaper and more honest than factoring out a generic version that
//!    would need to abstract over two unrelated rowan `Language`s for a
//!    ~25-line function.
//! 3. **Body-deferral strategy**: every container's `body` is
//!    `Block::default()` unconditionally — not a per-content-line
//!    diagnostic. A `flow`/`fn`'s body can contain prose, choices, diverts,
//!    conditionals, tags, annotations — all B0.7/B0.8 territory — and
//!    diagnosing every such line individually would be noise, not signal
//!    (the whole subsystem is deferred, not any one construct within it).
//!    This is distinct from judgment call #4 below: constructs *at the
//!    declaration layer* B0.6 owns but can't yet place get a diagnostic
//!    each; content *inside* a deferred body does not.
//! 4. **Deferred-construct diagnostics** (all E129 unless noted): a `fn`
//!    nested below top level (no HIR container carries `is_function` below
//!    `Knot`); a `flow` nested three levels deep (E130, the Q4(b) fence);
//!    a `module { … }` block (no HIR "module container" node exists —
//!    `HirFile.module` is a single file-identity fact, not a recursive
//!    container; contents are flattened into the enclosing scope and the
//!    diagnostic says so); an `@[…]` annotation line at declaration
//!    position (no directive channel is wired yet — see #5); a lambda
//!    expression in value position (`FnLiteral` is partial application
//!    over a named target, not an anonymous body — charter §8, no lambda
//!    node exists); any other body-line construct reaching top-level
//!    declaration position (content/tags/choices/diverts/conditionals with
//!    no enclosing `flow`/`fn` — a native file has no top-level body ground
//!    the way ink's root weave does; the only top-level "entry" spelling is
//!    the `flow main()` naming convention (`entry_root_content`, ruled
//!    2026-07-21), not literal content or a divert at file-root position —
//!    so such content has nowhere to go and must not vanish silently);
//!    `struct`/`extern`/
//!    `use`/`import` declared below the flattened top-level scope (nested
//!    inside a `flow`/`fn` body) — ink restricts these four kinds to
//!    top-level-only (D6), and the native grammar's shared `item()`
//!    dispatch means the parser *can* produce them at body position even
//!    though nothing downstream can use them there yet.
//! 5. **Directive/annotation channel left unwired.** `is_local`,
//!    `effects_assertion`, `visibility`, `@[was]` are all `None`/`false` on
//!    every decl node this slice produces — B0.5's grammar gives
//!    `@[name(args)]` a fully generic shape but no keyword syntax exists
//!    yet for the specific channels ink's `directive.rs` populates (no
//!    `KW_PUB`/`KW_PRIVATE`/`KW_LOCAL` tokens, no `@[was(…)]` recognition).
//!    Wiring these is real, scoped work (mirroring
//!    `hir/lower/directive.rs`'s sibling-walk-before-a-declaration
//!    pattern) that B0.6 did not need to do to meet its own exit
//!    criterion (admission-clean declaration HIR) — flagged for a
//!    follow-up slice rather than half-built here. `///`/`//!` docs ARE
//!    wired (B0.6b, `docs/decision-log.md` 2026-07-20, `doc_comment`) —
//!    they were judgment call 5 in the B0.6 report but shipped as their
//!    own ruled slice rather than staying deferred alongside the rest.
//! 6. **`import name;`'s semantics** (B0.5's own Finding #3 explicitly left
//!    this to B0.6): lowered as the *qualified* form of ink's `Import`
//!    (`module` = the joined path, `items` empty, `bare: false` — "brings
//!    only the module name into scope"), matching `use path;` with no
//!    `{ … }` list. `use`'s two shapes with no `Import` equivalent
//!    (module-level `as` aliasing; recursive nested groups) get E129
//!    rather than a lossy guess — see [`import`]'s module doc.
//! 7. **Native file-level module identity is never stamped**
//!    (`HirFile.module` stays `None` for every native file this slice
//!    produces). Unlike ink's `#@module(name)` tag (a per-file directive),
//!    native module identity is charter-ruled to be **filesystem-derived**
//!    (NF-3: "path on disk = path in language") — a project-layer fact
//!    B0.10 owns, not something a single-file lowering can determine.  A
//!    nested `module name { … }` *block* is a different concept (charter
//!    §13.2's declared sub-modules) — see judgment call #4.

mod body;
mod choice;
mod cond;
mod container;
mod decl;
mod doc_comment;
mod expr;
pub mod provenance;

use brink_syntax_native::SyntaxKind as N;
use brink_syntax_native::ast::{self, AstNode as _};
use brink_syntax_native::{SyntaxNode, SyntaxToken};

use crate::hir::FileId;
use crate::symbols::{SymbolManifest, project_manifest};
use crate::{
    Diagnostic, DiagnosticCode, Divert, DivertPath, DivertTarget, ExternalDecl, HirFile, Import,
    Knot, Path, StructDecl,
};

mod import;

/// Lower a complete native source file to HIR.
///
/// Produces the same `(HirFile, SymbolManifest, Vec<Diagnostic>)` triple
/// contract §1.1 requires of *any* frontend — the manifest is
/// [`project_manifest`]'s pure projection of the just-built `HirFile`
/// (B0.4), exactly as [`crate::hir::lower::lower`] does for ink. `Knot`/
/// `Stitch` bodies are always the empty stub; see the module doc's judgment
/// call #3. `root_content` is the one exception: see [`entry_root_content`]
/// for the `flow main()` entry convention.
#[must_use]
pub fn lower(
    file_id: FileId,
    file: &ast::SourceFile,
) -> (HirFile, SymbolManifest, Vec<Diagnostic>) {
    let mut diags: Vec<Diagnostic> = Vec::new();
    let mut top = TopLevel::default();

    walk_top_level(file.syntax_children(), file_id, &mut top, &mut diags);

    // `var`/`const`/`flags` are hoisted flat regardless of nesting — a
    // whole-tree walk, same posture ink's D6 ruling requires of every
    // frontend (`docs/hir-admission-contract.md` D6: "the global vecs are
    // flat and hoisted", not "walk descendants" as an ink-specific rule).
    let mut variables = Vec::new();
    let mut constants = Vec::new();
    let mut lists = Vec::new();
    for node in file.syntax().descendants() {
        match node.kind() {
            N::VAR_DECL => {
                if let Some(v) = ast::VarDecl::cast(node)
                    .and_then(|v| decl::lower_var_decl(file_id, &v, &mut diags))
                {
                    variables.push(v);
                }
            }
            N::CONST_DECL => {
                if let Some(c) = ast::ConstDecl::cast(node)
                    .and_then(|c| decl::lower_const_decl(file_id, &c, &mut diags))
                {
                    constants.push(c);
                }
            }
            N::FLAGS_DECL => {
                if let Some(l) = ast::FlagsDecl::cast(node)
                    .and_then(|l| decl::lower_flags_decl(file_id, &l, &mut diags))
                {
                    lists.push(l);
                }
            }
            _ => {}
        }
    }

    // struct/extern/use/import declared *outside* the flattened top-level
    // scope (nested inside a flow/fn body) never reach `walk_top_level` at
    // all — diagnose each rather than letting it vanish silently (judgment
    // call #4).
    for node in file.syntax().descendants() {
        let out_of_position = matches!(
            node.kind(),
            N::STRUCT_DECL | N::EXTERN_DECL | N::USE_DECL | N::IMPORT_DECL
        ) && !decl::in_flattened_scope(&node);
        if out_of_position {
            diags.push(diagnostic(file_id, node.text_range(), DiagnosticCode::E129));
        }
    }

    let root_content = entry_root_content(&top.knots);

    let hir = HirFile {
        root_content,
        knots: top.knots,
        variables,
        constants,
        lists,
        structs: top.structs,
        externals: top.externals,
        // Textual INCLUDE is dead on the native surface (charter §13.2:
        // "THE TREE IS THE COMPILATION UNIVERSE") — always empty.
        includes: Vec::new(),
        // Filesystem-derived, project-layer fact (NF-3) — B0.10, not a
        // single-file lowering. See judgment call #7.
        module: None,
        imports: top.imports,
        // No visibility-keyword / `@[was]` syntax wired yet (judgment call #5).
        visibility: Vec::new(),
        was_directives: Vec::new(),
    };
    let manifest = project_manifest(&hir);
    (hir, manifest, diags)
}

/// The native story-entry convention (maintainer-ruled 2026-07-21,
/// `docs/decision-log.md`): **a top-level `flow main()` is a native
/// story's default standalone entry point.** Mirrors ink's own "root
/// content is the entry" model (the ink frontend's
/// [`crate::hir::lower::lower`] populates `root_content` from the file's
/// literal top-level weave body) using the same `Divert`/`Block` HIR — no
/// new entry mechanism, no new HIR node. When a top-level, non-function,
/// zero-parameter `flow` named `main` exists, `root_content` becomes a
/// single synthesized `Divert` to it. Any other top-level `flow`/`fn`
/// remains host-entry-only (effects-spec §10 "play from here") — a project
/// with no `main` has an empty `root_content`, which is not an error, only
/// "no standalone entry point".
///
/// The synthesized `Divert` carries `ptr: None` — it is not backed by any
/// real `DIVERT_STMT` source node (nothing is written at file-root
/// position; the convention is implied by the flow's name), so there is no
/// honest provenance to attach, unlike every other `Divert` this frontend
/// produces. The target `Path`'s `Name` reuses `main`'s own declared name
/// token — real source provenance, not a fabricated range.
///
/// A `main` with parameters is deliberately **not** matched: a bare entry
/// divert cannot supply arguments, and silently dropping required params
/// would be a silent data drop (a standing rule, `CLAUDE.md` "Flag silent
/// data drops"). Such a file simply has no synthesized entry — `main` is
/// still an ordinary flow, reachable as a host entry point.
fn entry_root_content(knots: &[Knot]) -> crate::Block {
    let Some(main) = knots
        .iter()
        .find(|k| !k.is_function && k.params.is_empty() && k.name.text == "main")
    else {
        return crate::Block::default();
    };
    crate::Block::from_stmts(vec![crate::Stmt::Divert(Divert {
        ptr: None,
        target: DivertTarget {
            path: DivertPath::Path(Path {
                segments: vec![main.name.clone()],
                range: main.name.range,
            }),
            args: Vec::new(),
        },
    })])
}

#[derive(Default)]
struct TopLevel {
    knots: Vec<Knot>,
    structs: Vec<StructDecl>,
    externals: Vec<ExternalDecl>,
    imports: Vec<Import>,
}

fn diagnostic(file: FileId, range: rowan::TextRange, code: DiagnosticCode) -> Diagnostic {
    Diagnostic {
        file,
        range,
        message: code.title().to_string(),
        code,
    }
}

/// Walk one "flattened top-level scope" — `SOURCE_FILE`'s direct children,
/// or (recursively) a `MODULE_DECL`'s body, since native has no HIR
/// "module container" node yet (judgment call #4) and the flat-hoist
/// posture (D6) extends to declared containers, not just globals, once a
/// module block is entered. `var`/`const`/`flags` are handled by the
/// separate whole-tree pass in [`lower`] and skipped here.
fn walk_top_level(
    items: impl Iterator<Item = SyntaxNode>,
    file_id: FileId,
    out: &mut TopLevel,
    diags: &mut Vec<Diagnostic>,
) {
    for child in items {
        match child.kind() {
            N::FLOW_DECL => {
                if let Some(f) = ast::FlowDecl::cast(child)
                    && let Some(k) =
                        container::lower_top_level_container(file_id, &FlowOrFn::Flow(f), diags)
                {
                    out.knots.push(k);
                }
            }
            N::FN_DECL => {
                if let Some(f) = ast::FnDecl::cast(child)
                    && let Some(k) =
                        container::lower_top_level_container(file_id, &FlowOrFn::Fn(f), diags)
                {
                    out.knots.push(k);
                }
            }
            N::STRUCT_DECL => {
                if let Some(s) = ast::StructDecl::cast(child)
                    && let Some(sd) = decl::lower_struct_decl(file_id, &s, diags)
                {
                    out.structs.push(sd);
                }
            }
            N::EXTERN_DECL => {
                if let Some(e) = ast::ExternDecl::cast(child)
                    && let Some(ed) = decl::lower_extern_decl(file_id, &e, diags)
                {
                    out.externals.push(ed);
                }
            }
            N::USE_DECL => {
                if let Some(u) = ast::UseDecl::cast(child)
                    && let Some(imp) = import::lower_use_decl(file_id, &u, diags)
                {
                    out.imports.push(imp);
                }
            }
            N::IMPORT_DECL => {
                if let Some(i) = ast::ImportDecl::cast(child)
                    && let Some(imp) = import::lower_import_decl(file_id, &i, diags)
                {
                    out.imports.push(imp);
                }
            }
            N::MODULE_DECL => {
                if let Some(md) = ast::ModuleDecl::cast(child.clone()) {
                    let name_range = md
                        .name_token()
                        .map_or_else(|| child.text_range(), |t| t.text_range());
                    diags.push(diagnostic(file_id, name_range, DiagnosticCode::E129));
                    if let Some(body) = md.body() {
                        walk_top_level(body.items(), file_id, out, diags);
                    }
                }
            }
            // Handled by the separate whole-tree hoist pass in `lower`.
            // `ERROR` nodes are already reported by the parser itself
            // (`Parse::errors()`) — re-diagnosing here would mislabel a
            // syntax error as "valid but not yet lowered".
            // A file/module-level inner `//!` doc comment (B0.6b) — CST-only
            // for now (`ast::SourceFile::doc`'s doc comment: no native HIR
            // "whole file" or "module container" type exists yet to receive
            // it, judgment calls #4/#7). Not an error; just has nowhere to
            // go yet, same non-diagnosis as the other kinds in this arm.
            N::VAR_DECL | N::CONST_DECL | N::FLAGS_DECL | N::ERROR | N::DOC_COMMENT => {}
            // Every other body-line construct (content, tags, choices,
            // diverts, conditionals, alternations, annotations, …) reaching
            // declaration position: real data with no home in this slice
            // (native's `root_content` equivalent is hard-`Block::default()`
            // — judgment call #4/#7). Loud, never silent.
            _ => {
                diags.push(diagnostic(
                    file_id,
                    child.text_range(),
                    DiagnosticCode::E129,
                ));
            }
        }
    }
}

/// Unifies `ast::FlowDecl`/`ast::FnDecl` for [`container`]'s shared
/// top-level lowering — the two node kinds have identical accessor shapes
/// (`name_token`/`param_list`/`body`) but no common trait in
/// `brink-syntax-native` (mirroring its own `ast_node!` macro's
/// one-struct-per-kind pattern, not worth a trait for two variants).
pub(crate) enum FlowOrFn {
    Flow(ast::FlowDecl),
    Fn(ast::FnDecl),
}

impl FlowOrFn {
    fn syntax(&self) -> &SyntaxNode {
        match self {
            Self::Flow(f) => f.syntax(),
            Self::Fn(f) => f.syntax(),
        }
    }

    fn name_token(&self) -> Option<SyntaxToken> {
        match self {
            Self::Flow(f) => f.name_token(),
            Self::Fn(f) => f.name_token(),
        }
    }

    fn param_list(&self) -> Option<ast::ParamList> {
        match self {
            Self::Flow(f) => f.param_list(),
            Self::Fn(f) => f.param_list(),
        }
    }

    fn body(&self) -> Option<ast::Block> {
        match self {
            Self::Flow(f) => f.body(),
            Self::Fn(f) => f.body(),
        }
    }

    fn is_function(&self) -> bool {
        matches!(self, Self::Fn(_))
    }

    /// The leading `///` doc comment, if one is attached (B0.6b).
    fn doc(&self) -> Option<ast::DocComment> {
        match self {
            Self::Flow(f) => f.doc(),
            Self::Fn(f) => f.doc(),
        }
    }
}

#[cfg(test)]
mod tests;
