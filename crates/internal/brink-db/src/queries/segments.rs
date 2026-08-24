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

use brink_syntax::SegmentKind as SyntaxSegmentKind;

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
            let slice = &text[usize::from(seg.range.start())..usize::from(seg.range.end())];
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
}
