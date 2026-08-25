//! Per-knot segmentation as salsa tracked structs (issue #3084,
//! `docs/per-knot-incremental-lowering-spec.md` §3 step 1; segment-key
//! ruling 2026-08-24: tracked structs with content-seeded identity).
//!
//! [`file_segments_query`] runs `brink_syntax::segment_file` over one
//! file's text and mints one [`FileSegment`] tracked struct per segment.
//! The design rests on salsa's tracked-struct identity model:
//!
//! - **Identity is the content.** A tracked struct's identity is salsa's
//!   hash of its *untracked* fields — here `(kind, text,
//!   header_offset)` — plus a creation-order disambiguator for
//!   duplicates. An edit that only SHIFTS a knot (typing above it)
//!   recreates a struct with identical untracked fields, so the knot
//!   keeps its identity and every future per-segment memo keyed on it
//!   backdates. Hash collisions are harmless by construction: salsa
//!   compares the untracked fields for equality on recreation, so a
//!   collision degrades into an invalidation, never wrong reuse. This is
//!   why there is no hand-rolled `content_hash` field — salsa's own
//!   identity hash *is* the ruled content-hash key.
//! - **Position is the lone `#[tracked]` field.** Tracked fields are
//!   backdated per field: a consumer that reads only `kind`/`text`
//!   (per-segment lowering) is untouched by a pure shift edit, while a
//!   consumer that reads `offset` (range-rebasing assembly) re-runs —
//!   which it must anyway.
//! - **Duplicates**: two byte-identical segments share an identity hash
//!   and are disambiguated by creation order, so they are distinct and
//!   stable across unrelated edits. Inserting a third identical copy
//!   between two existing ones shifts one disambiguator and re-lowers
//!   that single segment — a pathological case (byte-identical knots
//!   share a knot name, already a duplicate-symbol diagnostic) with a
//!   one-segment cost.
//!
//! **Memory posture (decision log 2026-08-24, tentative):** each struct
//! stores its segment's text, so source text is resident ~2× (the
//! `SourceFile` input plus the segment copies). Accepted provisionally —
//! the `heap_size` estimator below reports the duplication honestly in
//! `ProjectDb::memory_snapshot`, and the posture is revisited once
//! real numbers are in hand. The alternative (segments carrying only
//! ranges, lowering slicing the original file text) would reintroduce a
//! whole-file input dependency and nothing would ever backdate — the
//! exact trap the FG-3 range-free projections exist to avoid.
//!
//! Like `raw_lowered_query`, the query is keyed on [`SourceFile`] alone
//! (project-independent): an edit to another file can never invalidate a
//! file's segmentation. The project-aware half (conventions claim-handler
//! injection, #2289) joins at the per-segment *lowering* layer, not here.

use std::sync::Arc;

use brink_ir::{Block, Diagnostic, HirFile, Knot};
use brink_syntax::SegmentKind as SyntaxSegmentKind;
use rowan::TextSize;

use super::SourceFile;

/// What a [`FileSegment`] covers — a mirror of
/// [`brink_syntax::SegmentKind`], local so it can implement
/// `salsa::Update` (a foreign trait on a foreign type is orphan-ruled
/// out, and `brink-syntax` deliberately knows nothing about salsa).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, salsa::Update)]
pub(crate) enum SegmentKind {
    /// Everything before the first knot/stitch header.
    Header,
    /// One top-level knot, doc block included.
    Knot,
    /// One top-level stitch before any knot (promoted to a knot by
    /// lowering), doc block included.
    TopLevelStitch,
}

impl From<SyntaxSegmentKind> for SegmentKind {
    fn from(kind: SyntaxSegmentKind) -> Self {
        match kind {
            SyntaxSegmentKind::Header => Self::Header,
            SyntaxSegmentKind::Knot => Self::Knot,
            SyntaxSegmentKind::TopLevelStitch => Self::TopLevelStitch,
        }
    }
}

/// `heap_size` estimator for [`FileSegment`]'s field tuple (declaration
/// order): the owned text buffer is the only heap payload.
pub(crate) fn segment_heap_size(fields: &(SegmentKind, String, Option<u32>, u32)) -> usize {
    fields.1.capacity()
}

/// One segment of one file. See the module doc for the identity model;
/// see `brink_syntax::segment_file` for the boundary rules (parser
/// dispatch mirror + doc-block extension).
#[salsa::tracked(heap_size = segment_heap_size)]
pub(crate) struct FileSegment<'db> {
    /// Untracked → identity: what kind of segment this is.
    pub kind: SegmentKind,
    /// Untracked → identity: the segment's source text, segment-relative.
    /// Per-segment lowering reads this (and only this plus `kind`), so a
    /// shift edit backdates it.
    #[returns(ref)]
    pub text: String,
    /// Untracked → identity: the header token's offset *within* the
    /// segment (`None` for the header segment). Content-derived — for
    /// identical text it is always identical, so it never perturbs
    /// identity.
    pub header_offset: Option<u32>,
    /// Tracked → positional value, backdated independently of the
    /// content fields: the segment's current absolute byte offset in the
    /// file. Only the range-rebasing assembly reads this.
    #[tracked]
    pub offset: u32,
}

/// Segment one file into [`FileSegment`] tracked structs, in source
/// order. Segments tile the file: `offset` + `text.len()` of segment *n*
/// equals `offset` of segment *n + 1*.
#[salsa::tracked(returns(ref))]
pub(crate) fn file_segments_query(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> Vec<FileSegment<'_>> {
    let text = file.text(db);
    brink_syntax::segment_file(text)
        .into_iter()
        .map(|seg| {
            let start = u32::from(seg.range.start());
            let slice =
                &text[usize::from(seg.lowered_range.start())..usize::from(seg.lowered_range.end())];
            FileSegment::new(
                db,
                SegmentKind::from(seg.kind),
                slice.to_owned(),
                seg.header_start.map(|h| u32::from(h) - start),
                start,
            )
        })
        .collect()
}

// ─── Per-segment lowering (#3084 §3 step 2) ──────────────────────────

/// One segment's lowered products, every range **segment-relative** (the
/// fragment was parsed in isolation), the file id already real (read from
/// the [`SourceFile`], never a placeholder). The composition is UNIFORM
/// across segment kinds — the fragment's content decides what is present:
/// a header fragment yields root content + the declaration surface, a
/// knot fragment yields one `knot_entries` entry plus any knot-nested
/// `VAR`/`CONST`/`LIST` hoists, a stitch fragment yields one
/// `top_level_knots` entry. Assembly (`assemble_lowered_file`) clones,
/// rebases by the segment's current offset, and concatenates in segment
/// order.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LoweredSegment {
    /// `lower_single_knot` per `KNOT_DEF` in the fragment (one for a knot
    /// segment, none otherwise). `None` knots are malformed headers whose
    /// diagnostics still count.
    pub knot_entries: Vec<(Option<Knot>, Vec<Diagnostic>)>,
    /// Stitch-promoted knots (one for a stitch segment, none otherwise).
    pub top_level_knots: Vec<Knot>,
    /// Root weave content — non-empty only for the header segment.
    pub root_content: Block,
    /// The declaration surface (`lower_declarations`): globals hoisted
    /// from anywhere in the fragment, plus — header fragment only —
    /// structs/externals/includes/module/imports and the directive
    /// collections. `knots`/`root_content` in here are empty by
    /// construction.
    pub decl_hir: HirFile,
    /// `lower_declarations`' diagnostic stream for this fragment.
    pub decl_diags: Vec<Diagnostic>,
    /// `lower_top_level`'s stream: stitch lowering, root content, and
    /// top-level `TODO:` notes (`E189`).
    pub content_diags: Vec<Diagnostic>,
    /// The fragment parse's errors as `E037` compile diagnostics —
    /// exactly the mapping the whole-file road applies.
    pub parse_errors: Vec<Diagnostic>,
}

/// Lower ONE segment in isolation. Reads the segment's `text` (identity
/// field) and the file's id — deliberately **not** `offset`, so a pure
/// shift edit leaves this memo fully validated (the tracked-struct
/// per-field contract, see [`FileSegment`]).
#[salsa::tracked(returns(ref))]
pub(crate) fn segment_lowered_query<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    segment: FileSegment<'db>,
) -> Arc<LoweredSegment> {
    let file_id = file.file_id(db);
    let parse = brink_syntax::parse(segment.text(db));
    let tree = parse.tree();

    let knot_entries: Vec<_> = tree
        .knots()
        .map(|knot_ast| brink_ir::lower_single_knot(file_id, &knot_ast))
        .collect();
    let (root_content, top_level_knots, content_diags) = brink_ir::lower_top_level(file_id, &tree);
    let (decl_hir, decl_diags) = brink_ir::lower_declarations(file_id, &tree);
    let parse_errors = parse
        .errors()
        .iter()
        .map(|e| Diagnostic {
            file: file_id,
            range: e.range,
            message: e.message.clone(),
            code: brink_ir::DiagnosticCode::E037,
        })
        .collect();

    Arc::new(LoweredSegment {
        knot_entries,
        top_level_knots,
        root_content,
        decl_hir,
        decl_diags,
        content_diags,
        parse_errors,
    })
}

/// Assemble one file's [`LoweredFile`](super::LoweredFile) from its
/// segments — the segment road's replacement for the whole-file
/// `lower_file` composition, which stays in place as this road's test
/// oracle.
///
/// Per-segment products are cloned, rebased by each segment's current
/// offset (`Rebase` — one add per position), and concatenated in segment
/// order. The assembled `HirFile`, manifest, and admission are
/// byte-identical to the whole-file road; the `diagnostics` vector holds
/// the identical MULTISET of diagnostics in a deterministic
/// segment-major order (the whole-file road's kind-grouped interleaving
/// is not reproduced — reproducing it would thread per-kind streams
/// through every product for zero semantic value; no consumer depends on
/// vector order). Pinned by the corpus equality gate in this module's
/// tests.
pub(crate) fn assemble_lowered_file(
    db: &dyn salsa::Database,
    file: SourceFile,
) -> super::LoweredFile {
    use brink_ir::hir::rebase::Rebase as _;

    let file_id = file.file_id(db);
    let segments = file_segments_query(db, file);
    let products: Vec<(TextSize, &Arc<LoweredSegment>)> = segments
        .iter()
        .map(|seg| {
            (
                TextSize::from(seg.offset(db)),
                segment_lowered_query(db, file, *seg),
            )
        })
        .collect();

    // The declaration surface: the header segment's product (always
    // present — `segment_file` emits a header segment even for an empty
    // file) is the base; every later segment's hoisted globals and
    // directive collections extend it in segment (= document) order.
    // The unreachable no-segments arm falls back to the whole-file
    // oracle on empty input rather than panicking.
    let Some(((first_delta, first_product), rest)) = products.split_first() else {
        return super::lower_file(file_id, &brink_syntax::parse(""));
    };
    let mut hir = first_product.decl_hir.clone();
    hir.rebase(*first_delta, file_id);
    let mut diagnostics: Vec<Diagnostic> = {
        let mut d = first_product.decl_diags.clone();
        for diag in &mut d {
            diag.rebase(*first_delta, file_id);
        }
        d
    };
    for (delta, product) in rest {
        let mut decls = product.decl_hir.clone();
        decls.rebase(*delta, file_id);
        hir.variables.extend(decls.variables);
        hir.constants.extend(decls.constants);
        hir.lists.extend(decls.lists);
        hir.structs.extend(decls.structs);
        hir.externals.extend(decls.externals);
        hir.includes.extend(decls.includes);
        hir.imports.extend(decls.imports);
        hir.visibility.extend(decls.visibility);
        hir.was_directives.extend(decls.was_directives);
        if decls.module.is_some() {
            hir.module = decls.module;
        }
        let mut d = product.decl_diags.clone();
        for diag in &mut d {
            diag.rebase(*delta, file_id);
        }
        diagnostics.append(&mut d);
    }

    // Root content comes from the header segment (index 0 — always
    // present); other segments' roots are empty by construction.
    if let Some((delta, product)) = products.first() {
        let mut root = product.root_content.clone();
        root.rebase(*delta, file_id);
        hir.root_content = root;
    }

    // Knots in document order, then stitch-promoted knots appended —
    // the whole-file road's `hir.knots` shape.
    for (delta, product) in &products {
        for (knot, _) in &product.knot_entries {
            if let Some(knot) = knot {
                let mut k = knot.clone();
                k.rebase(*delta, file_id);
                hir.knots.push(k);
            }
        }
    }
    for (delta, product) in &products {
        for knot in &product.top_level_knots {
            let mut k = knot.clone();
            k.rebase(*delta, file_id);
            hir.knots.push(k);
        }
    }

    // Content, knot, and parse-error diagnostics, segment-major.
    for (delta, product) in &products {
        let mut d = product.content_diags.clone();
        for diag in &mut d {
            diag.rebase(*delta, file_id);
        }
        diagnostics.append(&mut d);
        for (_, knot_diags) in &product.knot_entries {
            let mut d = knot_diags.clone();
            for diag in &mut d {
                diag.rebase(*delta, file_id);
            }
            diagnostics.append(&mut d);
        }
    }
    for (delta, product) in &products {
        let mut d = product.parse_errors.clone();
        for diag in &mut d {
            diag.rebase(*delta, file_id);
        }
        diagnostics.append(&mut d);
    }

    let manifest = brink_ir::symbols::project_manifest(&hir);
    diagnostics.extend(brink_analyzer::check_anonymous_stateful(file_id, &hir));

    let file_len = segments.last().map_or(TextSize::from(0), |seg| {
        TextSize::from(seg.offset(db)) + TextSize::of(seg.text(db).as_str())
    });
    let admission = brink_analyzer::validate_admission(file_id, &hir, &manifest, file_len);

    super::LoweredFile {
        hir,
        manifest,
        diagnostics,
        admission,
    }
}

// ─── Per-segment projection (#3064 B2) ───────────────────────────────

/// One segment's STRUCTURAL projection walk (`project_walk_parts`):
/// spans + join keys + option paths, segment-relative, handles local
/// from 0. Reads the segment's lowered fragment and its text — never
/// `offset`, so shift edits leave it memoized (the same contract as
/// [`segment_lowered_query`]).
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn segment_projection_query<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    segment: FileSegment<'db>,
) -> super::NoEqArc<brink_ir::hir::projection::ProjectionParts> {
    let product = segment_lowered_query(db, file, segment);
    let hir = fragment_hir(product);
    super::NoEqArc(Arc::new(brink_ir::hir::projection::project_walk_parts(
        &hir,
        segment.text(db),
    )))
}

/// The assembled, identity-joined whole-file projection (#3064 B2) —
/// the per-keystroke replacement for the retired wipe-on-every-edit
/// session projection cache. Composition mirrors the whole-file
/// `project_hir` exactly:
///
/// 1. the file-level declaration prologue over the ASSEMBLED HIR
///    (`project_file_decl_parts` — shared emitter, cannot drift);
/// 2. every segment's memoized walk parts, rebased by the segment's
///    current offset, handles renumbered by a running count, in the
///    whole-file `visit` order — header root content, then knot
///    segments, then stitch segments (matching `hir.knots`' assembly
///    order: knots first, stitch-promotions appended);
/// 3. the analyzer identity join, replayed at assembly from the
///    recorded [`brink_ir::hir::projection::JoinKey`]s against
///    `resolutions_index` (the cheap FG-3 half — never the diagnostics
///    bundle);
/// 4. `build_line_stacks` once over the assembled spans.
///
/// A native (`.brink`) file has no segment road — its walk runs
/// whole-file here (delta 0), the same composition with one fragment.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn projection_query(
    db: &dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
) -> super::NoEqArc<brink_ir::hir::projection::Projection> {
    use brink_ir::hir::projection as proj;

    let source = file.text(db);
    let lowered = super::lowered_query(db, project, file);

    // Pass 1: the declaration prologue over the assembled HIR.
    let decl_parts = proj::project_file_decl_parts(&lowered.hir, source);
    let mut spans = decl_parts.spans;
    let mut join_keys: Vec<(Option<proj::JoinKey>, TextSize)> = decl_parts
        .join_keys
        .into_iter()
        .map(|k| (k, TextSize::from(0)))
        .collect();
    let mut option_paths: std::collections::BTreeMap<u32, Vec<u32>> = decl_parts.option_paths;
    let mut handle_offset = decl_parts.handle_count;

    // Pass 2: segment walks in whole-file visit order.
    let append = |parts: &proj::ProjectionParts,
                  delta: TextSize,
                  spans: &mut Vec<proj::ProjectedSpan>,
                  join_keys: &mut Vec<(Option<proj::JoinKey>, TextSize)>,
                  option_paths: &mut std::collections::BTreeMap<u32, Vec<u32>>,
                  handle_offset: &mut u32| {
        for (span, key) in parts.spans.iter().zip(&parts.join_keys) {
            let mut s = *span;
            s.range += delta;
            if let Some(h) = s.handle.as_mut() {
                *h += *handle_offset;
            }
            spans.push(s);
            join_keys.push((*key, delta));
        }
        for (handle, path) in &parts.option_paths {
            option_paths.insert(handle + *handle_offset, path.clone());
        }
        *handle_offset += parts.handle_count;
    };

    if super::file_language(file.path(db)) == super::Language::Ink {
        let segs = file_segments_query(db, file);
        for pass_stitches in [false, true] {
            for seg in segs {
                let is_stitch = seg.kind(db) == SegmentKind::TopLevelStitch;
                if is_stitch != pass_stitches {
                    continue;
                }
                let parts = &segment_projection_query(db, file, *seg).0;
                append(
                    parts,
                    TextSize::from(seg.offset(db)),
                    &mut spans,
                    &mut join_keys,
                    &mut option_paths,
                    &mut handle_offset,
                );
            }
        }
    } else {
        let parts = proj::project_walk_parts(&lowered.hir, source);
        append(
            &parts,
            TextSize::from(0),
            &mut spans,
            &mut join_keys,
            &mut option_paths,
            &mut handle_offset,
        );
    }

    // Pass 3: identity join via rebased keys.
    let resolved = super::resolutions_index_query(db, project);
    let file_id = file.file_id(db);
    let mut decl_ids: std::collections::BTreeMap<(u32, u32), brink_format::DefinitionId> =
        std::collections::BTreeMap::new();
    for info in resolved.index.symbols.values() {
        if info.file == file_id {
            decl_ids.insert(proj::range_key(info.range), info.id);
        }
    }
    let mut ref_targets: std::collections::BTreeMap<(u32, u32), brink_format::DefinitionId> =
        std::collections::BTreeMap::new();
    for r in &resolved.resolutions {
        if r.file == file_id {
            ref_targets.insert(proj::range_key(r.range), r.target);
        }
    }
    for (span, (key, delta)) in spans.iter_mut().zip(&join_keys) {
        match key {
            Some(proj::JoinKey::Decl(r)) => {
                span.def_id = decl_ids.get(&proj::range_key(*r + *delta)).copied();
            }
            Some(proj::JoinKey::Ref(r)) => {
                span.target_id = ref_targets.get(&proj::range_key(*r + *delta)).copied();
            }
            None => {}
        }
    }

    // Pass 4: line stacks over the assembled whole.
    let lines = proj::build_line_stacks(&spans, source);
    super::NoEqArc(Arc::new(proj::Projection {
        spans,
        lines,
        option_paths,
    }))
}

// ─── Per-segment line contexts (#3064 B3) ────────────────────────────

/// Build the fragment `HirFile` a segment's structural views project
/// from — the lowered product's decl skeleton plus its knots and root
/// content, exactly the shape [`segment_projection_query`] walks.
fn fragment_hir(product: &LoweredSegment) -> HirFile {
    let mut hir = product.decl_hir.clone();
    hir.knots = product
        .knot_entries
        .iter()
        .filter_map(|(knot, _)| knot.clone())
        .collect();
    hir.knots.extend(product.top_level_knots.iter().cloned());
    hir.root_content = product.root_content.clone();
    hir
}

/// One segment's per-line contexts (#3064 B3): fragment parse (trivia
/// facet), fragment-local structural projection (decl prologue + the
/// memoized walk), and the registered dialect's classify+chain
/// post-pass — all fragment-relative. `LineContext` carries no absolute
/// positions (dialect spans are line-relative), so assembly is pure
/// per-line concatenation under the line-ownership rule in
/// [`line_contexts_query`]. Chains cannot cross fragments: a fragment
/// boundary is a knot/stitch header (structural — breaks every chain),
/// and the only lines before it in the fragment are its doc block's
/// comment lines.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn segment_line_contexts_query<'db>(
    db: &'db dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
    segment: FileSegment<'db>,
) -> super::NoEqArc<Vec<brink_ir::hir::line_context::LineContext>> {
    use brink_ir::hir::{line_context as lc, projection as proj};

    let product = segment_lowered_query(db, file, segment);
    let text = segment.text(db);
    let parse = brink_syntax::parse(text);
    let root = parse.syntax();

    let hir = fragment_hir(product);
    let decl_parts = proj::project_file_decl_parts(&hir, text);
    let walk = &segment_projection_query(db, file, segment).0;
    let mut spans = decl_parts.spans;
    spans.extend(walk.spans.iter().copied());
    let mut option_paths = decl_parts.option_paths;
    for (handle, path) in &walk.option_paths {
        option_paths.insert(*handle, path.clone());
    }
    let lines = proj::build_line_stacks(&spans, text);
    let projection = proj::Projection {
        spans,
        lines,
        option_paths,
    };

    let contexts = match &super::resolved_dialect_query(db, project).0 {
        Some(dialect) => lc::line_contexts_with_dialect(text, &root, &projection, dialect),
        None => lc::line_contexts(text, &root, &projection),
    };
    super::NoEqArc(Arc::new(contexts))
}

/// The assembled whole-file line contexts (#3064 B3) — per-segment
/// memoized for ink, whole-file for native (no segment road there yet).
///
/// Assembly is per-line concatenation under a line-OWNERSHIP rule: a
/// file line belongs to the segment whose TILING range contains the
/// line's start offset (segments' `lowered_range`s overlap on doc
/// blocks; tiling ranges don't). For the owned line `L`, the context
/// comes from the owner's fragment at index `L - line_of(owner start)`.
/// A mid-line cut (the trailing-comment doc quirk) leaves the cut line
/// owned by the PRECEDING segment, whose fragment ends exactly at the
/// cut — its classification of the truncated line matches the
/// whole-file one for every corpus case (equality-gated below).
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn line_contexts_query(
    db: &dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
) -> super::NoEqArc<Vec<brink_ir::hir::line_context::LineContext>> {
    use brink_ir::hir::line_context as lc;

    let source = file.text(db);

    if super::file_language(file.path(db)) != super::Language::Ink {
        // Native: whole-file composition (the pre-B3 shape), using the
        // assembled projection.
        let projection = &projection_query(db, project, file).0;
        let parse = super::parse_native_query(db, file);
        let root = parse.syntax();
        let contexts = match &super::resolved_dialect_query(db, project).0 {
            Some(dialect) => {
                lc::line_contexts_with_dialect_native(source, &root, projection, dialect)
            }
            None => lc::line_contexts_native(source, &root, projection),
        };
        return super::NoEqArc(Arc::new(contexts));
    }

    let owned = segment_owned_lines(db, file);
    let mut out: Vec<lc::LineContext> = Vec::with_capacity(owned.total_lines);
    for i in 0..owned.segments.len() {
        out.extend(segment_line_contexts_slice(db, project, file, &owned, i));
    }
    super::NoEqArc(Arc::new(out))
}

// ─── Per-segment semantic tokens (#3064 B4) ──────────────────────────

/// The file's absolute range-keyed resolution-kind map — one pass over
/// `resolutions_index` filtered to this file. Cheap, re-executed per
/// edit; the per-segment CUTOFF lives one query down.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn file_resolution_kinds_query(
    db: &dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
) -> super::NoEqArc<std::collections::BTreeMap<(u32, u32), u32>> {
    use brink_ir::hir::projection::range_key;
    let resolved = super::resolutions_index_query(db, project);
    let file_id = file.file_id(db);
    let mut map = std::collections::BTreeMap::new();
    for rref in &resolved.resolutions {
        if rref.file == file_id
            && let Some(info) = resolved.index.symbols.get(&rref.target)
        {
            map.insert(range_key(rref.range), info.kind.to_u32());
        }
    }
    super::NoEqArc(Arc::new(map))
}

/// One segment's FRAGMENT-RELATIVE resolution-kind map — the range-free
/// backdating seam (the FG-3 `call_site_metas` pattern): this query
/// re-executes every edit (it reads the file map and the segment's
/// tracked offset), but for a segment whose content and resolutions
/// didn't change the OUTPUT is bit-identical, so
/// [`segment_semantic_tokens_query`] backdates and never re-walks the
/// fragment. `Vec<(start, end, kind)>` rather than a map: the payload
/// needs `salsa::Update`, which std tuples/Vecs carry.
#[salsa::tracked(returns(ref))]
pub(crate) fn segment_resolution_kinds_query<'db>(
    db: &'db dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
    segment: FileSegment<'db>,
) -> Vec<(u32, u32, u32)> {
    let map = &file_resolution_kinds_query(db, project, file).0;
    let start = segment.offset(db);
    let end = start + u32::try_from(segment.text(db).len()).unwrap_or(u32::MAX);
    map.range((start, 0)..(end, u32::MAX))
        .filter(|((_, e), _)| *e <= end)
        .map(|((s, e), kind)| (s - start, e - start, *kind))
        .collect()
}

/// One segment's semantic tokens, fragment-relative positions (#3064
/// B4): fragment parse + the stateless token classifier against the
/// fragment-relative kind map. Backdates across shift edits AND
/// unrelated-content edits via the seam above.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn segment_semantic_tokens_query<'db>(
    db: &'db dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
    segment: FileSegment<'db>,
) -> super::NoEqArc<Vec<brink_ir::semantic_tokens::RawToken>> {
    let text = segment.text(db);
    let parse = brink_syntax::parse(text);
    let root = parse.syntax();
    let kinds: std::collections::BTreeMap<(u32, u32), brink_ir::SymbolKind> =
        segment_resolution_kinds_query(db, project, file, segment)
            .iter()
            .filter_map(|(s, e, k)| brink_ir::SymbolKind::from_u32(*k).map(|k| ((*s, *e), k)))
            .collect();
    super::NoEqArc(Arc::new(brink_ir::semantic_tokens::tokens_with_kinds(
        text, &root, &kinds,
    )))
}

/// One segment's CLASSIFIER-ONLY semantic tokens (#3064 micro): the
/// same fragment walk as [`segment_semantic_tokens_query`] with an
/// empty resolution-kind map — project-INDEPENDENT, so pulling it never
/// forces the symbol index or resolution pass. The keystroke path
/// serves the edited knot from this query and lets the deferred refresh
/// swap in the resolution-refined slice (~120 ms later); identifiers
/// whose color depends on resolution briefly render with the
/// classifier's default in the edited knot only.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn segment_semantic_tokens_classifier_query<'db>(
    db: &'db dyn salsa::Database,
    file: SourceFile,
    segment: FileSegment<'db>,
) -> super::NoEqArc<Vec<brink_ir::semantic_tokens::RawToken>> {
    let _ = file;
    let text = segment.text(db);
    let parse = brink_syntax::parse(text);
    let root = parse.syntax();
    let kinds = std::collections::BTreeMap::new();
    super::NoEqArc(Arc::new(brink_ir::semantic_tokens::tokens_with_kinds(
        text, &root, &kinds,
    )))
}

/// The assembled whole-file semantic tokens (#3064 B4). Assembly is
/// per-line: each file line's tokens come from the segment that OWNS the
/// line (the same trivia-prefix ownership rule as line contexts), with
/// the line number rebased by the owner's start line. Two boundary
/// refinements for a segment whose cut sits mid-line:
///
/// - the owner's fragment sees the line WITHOUT the prefix before the
///   cut, so its tokens on fragment line 0 get their columns shifted by
///   the cut's column;
/// - the PRECEDING segment's fragment holds the prefix's tokens for that
///   same line (its truncated last line), so a boundary line merges
///   both sources — prefix tokens first (columns already correct), then
///   the owner's rebased line-0 tokens, preserving column order.
#[salsa::tracked(returns(ref), no_eq)]
pub(crate) fn semantic_tokens_query(
    db: &dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
) -> super::NoEqArc<Vec<brink_ir::semantic_tokens::RawToken>> {
    let source = file.text(db);

    if super::file_language(file.path(db)) != super::Language::Ink {
        // Native: whole-file walk against the absolute kind map.
        let parse = super::parse_native_query(db, file);
        let kinds: std::collections::BTreeMap<(u32, u32), brink_ir::SymbolKind> =
            file_resolution_kinds_query(db, project, file)
                .0
                .iter()
                .filter_map(|((s, e), k)| brink_ir::SymbolKind::from_u32(*k).map(|k| ((*s, *e), k)))
                .collect();
        return super::NoEqArc(Arc::new(
            brink_ir::semantic_tokens::tokens_with_kinds_native(source, &parse.syntax(), &kinds),
        ));
    }

    let owned = segment_owned_lines(db, file);
    let mut out: Vec<brink_ir::semantic_tokens::RawToken> = Vec::new();
    for i in 0..owned.segments.len() {
        let owned_from = owned.segments[i].owned_from;
        for mut t in segment_semantic_tokens_slice(db, project, file, &owned, i) {
            t.line += u32::try_from(owned_from).unwrap_or(u32::MAX);
            out.push(t);
        }
    }
    super::NoEqArc(Arc::new(out))
}

// ─── Segment line ownership (shared by assembly + the delta protocol) ─

/// Per-segment line bookkeeping for assembly and the outbound delta
/// protocol (#3064 option A): the segment, the first line it OWNS, the
/// line its fragment starts on, and the UTF-16 width of the mid-line-cut
/// prefix (0 for a line-start cut). One definition of the ownership rule
/// — trivia-only prefixes (indentation, the BOM) give the line to the
/// cut's segment; content prefixes leave it with the preceding one — so
/// the two assembled queries and the per-segment slices cannot drift.
pub(crate) struct SegmentLines<'db> {
    pub seg: FileSegment<'db>,
    pub owned_from: usize,
    pub seg_start_line: usize,
    pub cut_col_utf16: u32,
}

pub(crate) struct OwnedLines<'db> {
    pub segments: Vec<SegmentLines<'db>>,
    pub total_lines: usize,
}

impl OwnedLines<'_> {
    pub fn owned_to(&self, i: usize) -> usize {
        self.segments
            .get(i + 1)
            .map_or(self.total_lines, |n| n.owned_from)
    }
}

pub(crate) fn segment_owned_lines(db: &dyn salsa::Database, file: SourceFile) -> OwnedLines<'_> {
    let source = file.text(db);
    let mut line_starts: Vec<u32> = vec![0];
    for (i, b) in source.bytes().enumerate() {
        if b == b'\n' {
            line_starts.push(u32::try_from(i + 1).unwrap_or(u32::MAX));
        }
    }
    let total_lines = brink_ir::hir::line_context::line_count_for(source);
    let line_of = |offset: u32| -> usize {
        match line_starts.binary_search(&offset) {
            Ok(l) => l,
            Err(next) => next - 1,
        }
    };
    let segs = file_segments_query(db, file);
    let mut segments = Vec::with_capacity(segs.len());
    for seg in segs {
        let start = seg.offset(db);
        let l = line_of(start);
        let prefix = &source[line_starts[l] as usize..start as usize];
        let only_trivia = prefix.chars().all(|c| c.is_whitespace() || c == '\u{feff}');
        segments.push(SegmentLines {
            seg: *seg,
            owned_from: if only_trivia { l } else { l + 1 },
            seg_start_line: l,
            cut_col_utf16: u32::try_from(prefix.encode_utf16().count()).unwrap_or(u32::MAX),
        });
    }
    OwnedLines {
        segments,
        total_lines,
    }
}

/// One segment's OWNED line-context slice (#3064 option A): exactly the
/// rows the assembled [`line_contexts_query`] emits for this segment, in
/// owned-line order — so a consumer-side concatenation of every
/// segment's slice equals the whole-file result by construction
/// (pinned in the parity gate).
pub(crate) fn segment_line_contexts_slice(
    db: &dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
    owned: &OwnedLines<'_>,
    i: usize,
) -> Vec<brink_ir::hir::line_context::LineContext> {
    let sl = &owned.segments[i];
    let ctxs = &segment_line_contexts_query(db, project, file, sl.seg).0;
    (sl.owned_from..owned.owned_to(i))
        .map(|line| {
            ctxs.get(line - sl.seg_start_line)
                .cloned()
                .unwrap_or_default()
        })
        .collect()
}

/// One segment's OWNED semantic-token slice, token lines RELATIVE to the
/// segment's `owned_from` (#3064 option A) — relative so a consumer's
/// cached slice survives shift edits unchanged; the consumer adds the
/// manifest's `owned_from` back at assembly. Includes the boundary-line
/// contributions exactly as the assembled query emits them: this
/// segment's cut-column-rebased line-0 tokens when a trivia-prefix cut
/// gave it the line, and the NEXT segment's rebased line-0 tokens when a
/// content-prefix cut left the boundary line here.
pub(crate) fn segment_semantic_tokens_slice(
    db: &dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
    owned: &OwnedLines<'_>,
    i: usize,
) -> Vec<brink_ir::semantic_tokens::RawToken> {
    segment_semantic_tokens_slice_with(db, project, file, owned, i, false)
}

/// [`segment_semantic_tokens_slice`] with a source selector:
/// `classifier_only` swaps in the project-independent classifier query —
/// no index/resolve pull — for the keystroke-synchronous path.
pub(crate) fn segment_semantic_tokens_slice_with(
    db: &dyn salsa::Database,
    project: super::ProjectInput,
    file: SourceFile,
    owned: &OwnedLines<'_>,
    i: usize,
    classifier_only: bool,
) -> Vec<brink_ir::semantic_tokens::RawToken> {
    let sl = &owned.segments[i];
    let owned_from = sl.owned_from;
    let owned_to = owned.owned_to(i);
    let mut out: Vec<brink_ir::semantic_tokens::RawToken> = Vec::new();

    let push_from = |sl: &SegmentLines<'_>, merge_boundary_only: bool, out: &mut Vec<_>| {
        let tokens = if classifier_only {
            &segment_semantic_tokens_classifier_query(db, file, sl.seg).0
        } else {
            &segment_semantic_tokens_query(db, project, file, sl.seg).0
        };
        for t in tokens.iter() {
            let file_line = sl.seg_start_line + t.line as usize;
            let in_window = file_line >= owned_from && file_line < owned_to;
            let boundary = merge_boundary_only && t.line == 0;
            if merge_boundary_only {
                if !boundary || !in_window {
                    continue;
                }
            } else {
                let self_owned = file_line >= sl.owned_from;
                if !in_window || !self_owned {
                    continue;
                }
            }
            let mut t = t.clone();
            t.line = u32::try_from(file_line - owned_from).unwrap_or(u32::MAX);
            if file_line == sl.seg_start_line {
                t.start_char += sl.cut_col_utf16;
            }
            out.push(t);
        }
    };

    push_from(sl, false, &mut out);
    // A content-prefix cut on the NEXT boundary leaves that line owned
    // here; the next fragment's line-0 tokens land on it.
    if let Some(next) = owned.segments.get(i + 1)
        && next.owned_from > next.seg_start_line
    {
        push_from(next, true, &mut out);
    }
    out
}

#[cfg(test)]
mod tests {
    use salsa::plumbing::AsId;

    use super::{FileSegment, SegmentKind, file_segments_query};
    use crate::ProjectDb;

    const BASE: &str = "\
VAR x = 1
Intro prose.
== alpha ==
Alpha body.
== beta ==
Beta body.
== gamma ==
Gamma body.
";

    fn segments<'db>(db: &'db ProjectDb, path: &str) -> &'db [FileSegment<'db>] {
        let id = db.file_id(path).expect("file is loaded");
        let file = db.test_source_file(id).expect("source file exists");
        file_segments_query(db.test_salsa(), file)
    }

    /// The query's output mirrors `brink_syntax::segment_file`: same
    /// kinds, offsets, and text slices, tiling the file.
    #[test]
    fn segments_match_the_segmenter() {
        let mut db = ProjectDb::new();
        db.update_file("a.ink", BASE.to_owned());

        let expected = brink_syntax::segment_file(BASE);
        let got = segments(&db, "a.ink");
        assert_eq!(got.len(), expected.len());
        let salsa = db.test_salsa();
        let mut pos = 0u32;
        for (seg, exp) in got.iter().zip(&expected) {
            assert_eq!(seg.kind(salsa), SegmentKind::from(exp.kind));
            assert_eq!(seg.offset(salsa), u32::from(exp.range.start()));
            assert_eq!(seg.offset(salsa), pos, "segments must tile the file");
            assert_eq!(seg.text(salsa), &BASE[exp.range]);
            pos += u32::try_from(seg.text(salsa).len()).unwrap_or(u32::MAX);
        }
        assert_eq!(pos, u32::try_from(BASE.len()).unwrap_or(u32::MAX));
    }

    /// The ruled identity property: an edit INSIDE one knot gives that
    /// knot a new identity while every other segment — including the
    /// ones the edit SHIFTED — keeps its identity, so their future
    /// per-segment memos can backdate. The shifted segment's tracked
    /// `offset` field still reflects the new position.
    #[test]
    fn shift_edit_preserves_every_unedited_segment_identity() {
        let mut db = ProjectDb::new();
        db.update_file("a.ink", BASE.to_owned());
        let before: Vec<salsa::Id> = segments(&db, "a.ink").iter().map(AsId::as_id).collect();
        assert_eq!(before.len(), 4, "header + three knots");

        // Grow beta by one line: alpha/header keep their bytes AND
        // offsets; gamma keeps its bytes but shifts.
        let edited = BASE.replace("Beta body.\n", "Beta body.\nA second beta line.\n");
        db.update_file("a.ink", edited.clone());
        let after: Vec<salsa::Id> = segments(&db, "a.ink").iter().map(AsId::as_id).collect();

        assert_eq!(before[0], after[0], "header identity must survive");
        assert_eq!(before[1], after[1], "alpha identity must survive");
        assert_ne!(before[2], after[2], "edited beta must get a new identity");
        assert_eq!(
            before[3], after[3],
            "gamma shifted but its content is unchanged — identity must survive"
        );

        let gamma = segments(&db, "a.ink")[3];
        let expected_offset =
            u32::try_from(edited.find("== gamma ==").expect("gamma exists")).unwrap_or(u32::MAX);
        assert_eq!(
            gamma.offset(db.test_salsa()),
            expected_offset,
            "the tracked offset field must reflect the shifted position"
        );
    }

    /// Two byte-identical knots are distinct segments with stable
    /// identities across an unrelated edit (creation-order
    /// disambiguation).
    #[test]
    fn duplicate_knots_are_distinct_and_stable() {
        const DUP: &str =
            "Intro.\n== twin ==\nSame body.\n== twin ==\nSame body.\n== tail ==\nTail body.\n";
        let mut db = ProjectDb::new();
        db.update_file("a.ink", DUP.to_owned());
        let before: Vec<salsa::Id> = segments(&db, "a.ink").iter().map(AsId::as_id).collect();
        assert_eq!(before.len(), 4);
        assert_ne!(
            before[1], before[2],
            "identical twins are distinct segments"
        );

        let edited = DUP.replace("Tail body.\n", "Tail body, edited.\n");
        db.update_file("a.ink", edited);
        let after: Vec<salsa::Id> = segments(&db, "a.ink").iter().map(AsId::as_id).collect();
        assert_eq!(
            before[1], after[1],
            "first twin stable across unrelated edit"
        );
        assert_eq!(
            before[2], after[2],
            "second twin stable across unrelated edit"
        );
        assert_ne!(before[3], after[3], "edited tail gets a new identity");
    }

    // ─── Segment-road equality gate (#3084 §4: the byte-identity bar) ──

    /// Compare the segment road against the whole-file oracle for one
    /// source text: HIR, manifest, and admission must be byte-identical;
    /// diagnostics must be multiset-identical (the assembled vector's
    /// order is deliberately segment-major — see `assemble_lowered_file`).
    #[expect(
        clippy::too_many_lines,
        reason = "the roads-agree gate: one comparison block per parity surface \
                  (lowering, projection, contexts, tokens, delta reconstruction) — \
                  splitting them would obscure that they all run per fixture"
    )]
    fn assert_roads_agree(source: &str, label: &str) {
        let mut db = ProjectDb::new();
        let id = db.update_file("gate.ink", source.to_owned());
        let file = db.test_source_file(id).expect("file exists");
        let salsa = db.test_salsa();

        let assembled = super::assemble_lowered_file(salsa, file);
        let parse = brink_syntax::parse(source);
        let oracle = crate::queries::lower_file(id, &parse);

        assert_eq!(assembled.hir, oracle.hir, "HIR diverged: {label}");
        assert_eq!(
            assembled.manifest, oracle.manifest,
            "manifest diverged: {label}"
        );
        assert_eq!(
            assembled.admission, oracle.admission,
            "admission diverged: {label}"
        );
        // Projection parity (#3064 B2): the assembled per-segment
        // projection must equal the whole-file walk with the identity
        // join, spans/handles/line-stacks/option-paths included.
        {
            use brink_ir::hir::projection as proj;
            let analysis = db.analysis();
            let mut decl_ids = std::collections::BTreeMap::new();
            for info in analysis.index.symbols.values() {
                if info.file == id {
                    decl_ids.insert(proj::range_key(info.range), info.id);
                }
            }
            let mut ref_targets = std::collections::BTreeMap::new();
            for r in &analysis.resolutions {
                if r.file == id {
                    ref_targets.insert(proj::range_key(r.range), r.target);
                }
            }
            let oracle_projection =
                proj::project_with_maps(&oracle.hir, source, &decl_ids, &ref_targets);
            let assembled_projection = db.projection(id).expect("projection");
            assert_eq!(
                *assembled_projection, oracle_projection,
                "projection diverged: {label}"
            );
        }

        // Line-context parity (#3064 B3): assembled per-segment contexts
        // must equal the whole-file classification.
        {
            use brink_ir::hir::line_context as lc;
            let root = brink_syntax::parse(source).syntax();
            let projection = db.projection(id).expect("projection");
            let oracle_contexts = lc::line_contexts(source, &root, &projection);
            let assembled_contexts = db.line_contexts(id).expect("contexts");
            assert_eq!(
                *assembled_contexts, oracle_contexts,
                "line contexts diverged: {label}"
            );
        }

        // Semantic-token parity (#3064 B4): the assembled per-segment
        // tokens must equal the whole-file walk with the identity join.
        {
            use brink_ir::hir::projection::range_key;
            let analysis = db.analysis();
            let mut kinds = std::collections::BTreeMap::new();
            for rref in &analysis.resolutions {
                if rref.file == id
                    && let Some(info) = analysis.index.symbols.get(&rref.target)
                {
                    kinds.insert(range_key(rref.range), info.kind);
                }
            }
            let root = brink_syntax::parse(source).syntax();
            let oracle_tokens = brink_ir::semantic_tokens::tokens_with_kinds(source, &root, &kinds);
            let assembled_tokens = db.semantic_tokens(id).expect("tokens");
            assert_eq!(
                *assembled_tokens, oracle_tokens,
                "semantic tokens diverged: {label}"
            );
        }

        // Outbound-delta parity (#3064 option A): reconstructing the
        // whole-document results from the manifest + per-segment slices
        // must equal the assembled queries exactly.
        if let Some((manifest, _total)) = db.segment_manifest(id) {
            let mut contexts = Vec::new();
            let mut tokens = Vec::new();
            for (key, owned_from) in &manifest {
                contexts.extend(db.segment_line_contexts_slice(id, key).expect("live key"));
                for mut t in db.segment_semantic_tokens_slice(id, key).expect("live key") {
                    t.line += owned_from;
                    tokens.push(t);
                }
            }
            assert_eq!(
                contexts,
                *db.line_contexts(id).expect("contexts"),
                "delta-reconstructed contexts diverged: {label}"
            );
            assert_eq!(
                tokens,
                *db.semantic_tokens(id).expect("tokens"),
                "delta-reconstructed tokens diverged: {label}"
            );
        }

        let mut a = assembled.diagnostics.clone();
        let mut b = oracle.diagnostics.clone();
        let key = |d: &brink_ir::Diagnostic| {
            (
                d.file.0,
                u32::from(d.range.start()),
                u32::from(d.range.end()),
                format!("{:?}", d.code),
                d.message.clone(),
            )
        };
        a.sort_by_key(key);
        b.sort_by_key(key);
        assert_eq!(a, b, "diagnostic multiset diverged: {label}");
    }

    #[test]
    fn roads_agree_on_crafted_fixtures() {
        let fixtures: &[(&str, &str)] = &[
            ("base", BASE),
            ("empty", ""),
            ("header only", "VAR x = 1\nJust prose.\n"),
            (
                "knot-nested globals",
                "Intro.\n== alpha ==\nBody.\nVAR nested = 3\nMore body.\n== beta ==\n{nested}\n-> END\n",
            ),
            (
                "top-level stitch + knot stitch",
                "= lobby\nStitch content.\n== alpha ==\nBody.\n= inner\nInner.\n-> END\n",
            ),
            (
                "doc blocks travel",
                "/// About alpha.\n== alpha ==\nBody.\n/// About beta.\n/// @kind scene\n== beta ==\nBody.\n",
            ),
            (
                "module directives",
                "#@module(alpha)\n#@was(old_alpha)\n== greet ==\nHello.\n-> END\n",
            ),
            (
                "orphan was before knot",
                "#@was(ghost)\n== greet ==\nHello.\n-> END\n",
            ),
            (
                "todo notes both levels",
                "TODO: top note\nProse.\n== alpha ==\nTODO: knot note\nBody.\n",
            ),
            (
                "parse errors in knot",
                "== alpha ==\n{ unclosed conditional\nBody.\n== beta ==\nFine.\n-> END\n",
            ),
            (
                "block comment hiding header",
                "Intro.\n/* dead\n== ghost ==\n*/\n== alpha ==\nBody.\n-> DONE\n",
            ),
            (
                "choices and weave",
                "== alpha ==\n* [One] First.\n* [Two] Second. # aside\n- Gather.\n-> DONE\n",
            ),
        ];
        for (label, source) in fixtures {
            assert_roads_agree(source, label);
        }
    }

    /// Dialect classification parity (#3064 B3): with a dialect config
    /// registered, the assembled per-segment contexts must equal the
    /// whole-file dialect pass — chains, hidden geometry, carry attrs.
    #[test]
    fn dialect_line_contexts_agree_with_whole_file() {
        use brink_ir::hir::line_context as lc;

        // The `@Name:<>` at-cue preset — the same default the studio
        // registers (reproduces the hardcoded screenplay behavior).
        let config = brink_ir::DialogueDialect::default();

        let source = "Intro prose.
== alpha ==
@Alice:<>
Hello there.
Second dialogue line.

Plain narrative after blank.
== beta ==
@Bob:<>
Hi.
-> END
";
        let mut db = ProjectDb::new();
        db.set_dialect(Some(config));
        let id = db.update_file("gate.ink", source.to_owned());

        let root = brink_syntax::parse(source).syntax();
        let projection = db.projection(id).expect("projection");
        let dialect = std::sync::Arc::clone(db.resolved_dialect().expect("dialect compiles"));
        let oracle = lc::line_contexts_with_dialect(source, &root, &projection, &dialect);
        let assembled = db.line_contexts(id).expect("contexts");
        assert_eq!(*assembled, oracle, "dialect contexts diverged");
        assert!(
            oracle.iter().any(|c| c.dialect.is_some()),
            "fixture must actually exercise dialect classification"
        );
    }

    /// The delta protocol's core promise (#3064 option A): a knot-interior
    /// edit changes ONLY the edited segment's manifest key — every other
    /// segment, shifted ones included, keeps its `index:generation`
    /// version, so a consumer's cached slices stay valid.
    #[test]
    fn manifest_keys_survive_shift_edits() {
        let mut db = ProjectDb::new();
        db.update_file("a.ink", BASE.to_owned());
        let id = db.file_id("a.ink").expect("loaded");
        let (before, _) = db.segment_manifest(id).expect("manifest");
        assert_eq!(before.len(), 4, "header + three knots");

        let edited = BASE.replace("Beta body.\n", "Beta body.\nA second beta line.\n");
        db.update_file("a.ink", edited);
        let (after, _) = db.segment_manifest(id).expect("manifest");

        assert_eq!(before[0].0, after[0].0, "header key survives");
        assert_eq!(before[1].0, after[1].0, "alpha key survives");
        assert_ne!(before[2].0, after[2].0, "edited beta gets a new key");
        assert_eq!(
            before[3].0, after[3].0,
            "gamma shifted but unchanged — its key (and any cached slice) survives"
        );
        assert_ne!(
            before[3].1, after[3].1,
            "gamma's owned-from line DID move — the manifest carries the shift"
        );
    }

    /// The tier corpora, swept file by file. `tests_github`/`tests_patched`
    /// join under `BRINK_SEGMENT_SWEEP_FULL=1` (a local one-off; CI cover
    /// for those comes from the oracle ratchet and goldens running through
    /// the rewired road).
    #[test]
    fn roads_agree_across_the_tier_corpora() {
        fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
            let Ok(entries) = std::fs::read_dir(dir) else {
                return;
            };
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_dir() {
                    collect(&path, out);
                } else if path.extension().is_some_and(|e| e == "ink") {
                    out.push(path);
                }
            }
        }
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../..")
            .join("tests");
        let mut dirs = vec![root.join("tier1"), root.join("tier2"), root.join("tier3")];
        if std::env::var("BRINK_SEGMENT_SWEEP_FULL").is_ok() {
            dirs.push(root.join("tests_github"));
            dirs.push(root.join("tests_patched"));
        }
        let mut files = Vec::new();
        for dir in &dirs {
            collect(dir, &mut files);
        }
        files.sort();
        assert!(
            files.len() >= 200,
            "corpus discovery looks broken: {} files",
            files.len()
        );
        for path in &files {
            let Ok(source) = std::fs::read_to_string(path) else {
                continue;
            };
            assert_roads_agree(&source, &path.display().to_string());
        }
    }

    /// The incrementality pin — the reason the firewall exists: after an
    /// edit inside ONE knot, every other segment's lowering memo is
    /// untouched (same `Arc` pointer — salsa backdated it), including the
    /// segments the edit SHIFTED; only the edited knot's memo is a fresh
    /// allocation. Pointer identity is this repo's standard backdating
    /// observable (the fg-suite pattern) and is immune to test
    /// parallelism, unlike an execution counter.
    #[test]
    fn knot_interior_edit_relowers_only_that_segment() {
        fn product_ptrs(db: &ProjectDb, path: &str) -> Vec<*const super::LoweredSegment> {
            let id = db.file_id(path).expect("loaded");
            let file = db.test_source_file(id).expect("file");
            let salsa = db.test_salsa();
            super::file_segments_query(salsa, file)
                .iter()
                .map(|seg| std::sync::Arc::as_ptr(super::segment_lowered_query(salsa, file, *seg)))
                .collect()
        }

        let mut db = ProjectDb::new();
        db.update_file("a.ink", BASE.to_owned());
        let before = product_ptrs(&db, "a.ink");
        assert_eq!(before.len(), 4, "header + three knots");

        // Grow beta by one line: header/alpha unshifted, gamma shifted.
        let edited = BASE.replace("Beta body.\n", "Beta body.\nA second beta line.\n");
        db.update_file("a.ink", edited);
        let after = product_ptrs(&db, "a.ink");

        assert_eq!(before[0], after[0], "header memo must be untouched");
        assert_eq!(before[1], after[1], "alpha memo must be untouched");
        assert_ne!(before[2], after[2], "edited beta must re-lower");
        assert_eq!(
            before[3], after[3],
            "gamma shifted but content-unchanged — its memo must be untouched"
        );
    }
}
