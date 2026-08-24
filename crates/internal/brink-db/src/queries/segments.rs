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
