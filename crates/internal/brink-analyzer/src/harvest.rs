//! Project-wide harvest index over cue payloads and inline-markup span
//! kinds/attributes (`docs/prose-dialect-spec.md` §5, issue #2114).
//!
//! §5's ruling is "harvest by default, declaration upgrades": every
//! `@NAME` cue and every markup span kind/attribute name actually written
//! anywhere in the project completes everywhere, and an optional
//! declaration (a cast roster for cues, the host manifest for markup)
//! *upgrades* a harvested name with richer metadata — it never gates
//! whether the name completes at all. The ruling names the mechanism
//! explicitly: "harvest is a project-db index obligation — cue payloads
//! and span kinds are indexed project-wide (sibling of the symbol index),
//! so completion crosses files." [`harvest`] is that merge — the same
//! shape as [`crate::symbol_index_with_modules`], a pure function of every
//! file's already-lowered HIR, so `brink-db`'s `harvest_index_query` gets
//! the identical incrementality `symbol_index_query` gets from
//! `lowered_query`'s per-file memoization: an edit only invalidates this
//! merge when some file's `LoweredFile` output actually changes.
//!
//! # The two upgrade paths are not symmetric yet
//!
//! **Markup spans upgrade from the [`HostManifest`]** (`markup` field),
//! folded in here directly: the manifest is an ordinary project input (no
//! comptime evaluation), so nothing blocks reading it project-wide today.
//! [`SpanHarvest::declared`] carries the manifest's [`ManifestSpanKind`]
//! **verbatim** — not degraded to a bare `Vec<String>` — because §5 also
//! rules that "the manifest and conventions files carry editor-consumed
//! fields the compiler ignores (descriptions, attr types, display
//! metadata)... a declaration format is a documentation format", and issue
//! #1997/PR #2016 widened `ManifestSpanKind::attrs` from `Vec<String>` to
//! `Vec<ManifestSpanAttr>` specifically so a `required` flag (and headroom
//! for a future attribute-value type) has somewhere to live. Stripping
//! back to names here would silently discard exactly the fields #1997
//! added and the ruling says not to drop.
//!
//! **Cue names have no upgrade path here.** §5 names an optional "cast
//! roster" that upgrades a harvested character name with typo validation,
//! display name, editor color, and a voice ref — but no such type or
//! registration point exists anywhere in the compiler yet (grep turns up
//! nothing), and the roster is explicitly named as "a natural early tenant
//! of the §3.5 module door" — the same comptime-evaluated conventions
//! machinery issue #1840 has not landed. [`CueHarvest`] is therefore
//! harvest-only by construction; a `declared` field is additive, future
//! work once that mechanism exists, not something this issue can build
//! ahead of it.

use std::collections::{BTreeMap, BTreeSet};

use brink_ir::hir::{Content, ContentContext, ContentPart, HirFile, HirVisitor, SpanPart};
use brink_ir::{FileId, HostManifest, ManifestSpanKind};
use rowan::TextRange;

/// One occurrence of a harvested name — lets a completion consumer answer
/// "where is this used", not just "does this name exist".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HarvestSite {
    pub file: FileId,
    pub range: TextRange,
}

/// One `@NAME` cue's harvest record: every occurrence project-wide, in no
/// particular merge order (a completion consumer sorts as it needs).
///
/// See this module's doc for why there is no `declared` field yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CueHarvest {
    pub sites: Vec<HarvestSite>,
}

/// One markup span kind's harvest record.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanHarvest {
    /// Where this span kind's tag itself was written.
    pub sites: Vec<HarvestSite>,
    /// Attribute name -> every site it was written at, on any occurrence of
    /// this kind. Harvested regardless of whether the manifest (if any)
    /// declares it — an undeclared attribute is still real usage, worth
    /// completing, exactly like an undeclared tag is (freeform stays the
    /// default; `markup_check` is the separate pass that diagnoses it).
    ///
    /// The recorded site is the *enclosing span's* range, not the
    /// attribute's own — HIR's `SpanPart::attrs` is a flat
    /// `Vec<(String, String)>` with no per-attribute provenance, so there
    /// is no narrower range to record. A consumer highlighting or renaming
    /// a specific attribute will select the whole `<tag …>` node, and two
    /// occurrences of the same attribute name on one tag (e.g.
    /// `<wave a="1" a="2">`) report two byte-identical sites. This is a
    /// stated limitation of the current HIR shape, not a bug in this pass.
    pub attrs: BTreeMap<String, Vec<HarvestSite>>,
    /// The host manifest's own declaration of this kind, when registered —
    /// the "declaration upgrades" half of §5's ruling, carried verbatim
    /// (see module doc). `None` for a harvest-only (freeform) kind.
    pub declared: Option<ManifestSpanKind>,
}

/// The project-wide harvest index (issue #2114): every `@NAME` cue payload
/// and every inline-markup span kind/attribute name, keyed by name so
/// completion crosses files — the compiler-side sibling of
/// [`crate::SymbolIndex`](crate::symbol_index).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarvestIndex {
    pub cues: BTreeMap<String, CueHarvest>,
    pub spans: BTreeMap<String, SpanHarvest>,
}

/// Range-free completion projection of a [`HarvestIndex`] (issue #2134).
///
/// Every [`HarvestSite`] carries a `TextRange`, so the raw index can never
/// `Eq`-cutoff — nearly any edit shifts a site's range and would defeat
/// early cutoff for every dependent, exactly the property that forced
/// `resolution_index_query` to exist as a range-zeroed early-cutoff
/// projection of the symbol index (see `brink-db`'s
/// `queries/mod.rs` module doc, "The `resolution_index` cutoff seam"). A
/// completion consumer only needs to know *that* a name exists project-wide,
/// never *where* — so this projection keeps just the name sets (and, for
/// spans, the manifest's `declared` metadata, which carries no ranges
/// either). `harvest_completion_index_query` in `brink-db` is this
/// projection's own tracked-query wrapper, the harvest-index sibling of
/// `resolution_index_query`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HarvestNames {
    pub cues: BTreeSet<String>,
    pub spans: BTreeMap<String, SpanNames>,
}

/// One markup span kind's range-free completion record — see [`HarvestNames`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SpanNames {
    pub attrs: BTreeSet<String>,
    /// The host manifest's declaration of this kind, when registered — see
    /// [`SpanHarvest::declared`].
    pub declared: Option<ManifestSpanKind>,
}

impl HarvestIndex {
    /// Project this index down to the range-free name sets a completion
    /// consumer needs (issue #2134) — see [`HarvestNames`]'s doc for why the
    /// projection exists. Backdates across any edit that shifts a site's
    /// range without adding or removing a name/attribute.
    #[must_use]
    pub fn names(&self) -> HarvestNames {
        HarvestNames {
            cues: self.cues.keys().cloned().collect(),
            spans: self
                .spans
                .iter()
                .map(|(name, span)| {
                    (
                        name.clone(),
                        SpanNames {
                            attrs: span.attrs.keys().cloned().collect(),
                            declared: span.declared.clone(),
                        },
                    )
                })
                .collect(),
        }
    }
}

/// Build the project-wide harvest index from every file's HIR, upgrading
/// any markup span kind the host manifest declares.
///
/// The manifest's vocabulary is folded in *first* so a declared-but-never-
/// used kind still completes (§5: "declaration upgrades", not "declaration
/// replaces harvest") — mirroring `markup_check::check`'s own reading of
/// the same field. `manifest: None`, or one that declares no `markup` key
/// at all, contributes nothing beyond harvested usage — the same
/// freeform-by-default posture `markup_check` holds.
#[must_use]
pub fn harvest(files: &[(FileId, &HirFile)], manifest: Option<&HostManifest>) -> HarvestIndex {
    let mut index = HarvestIndex::default();

    if let Some(manifest) = manifest {
        for kind in &manifest.markup {
            let entry = index.spans.entry(kind.name.clone()).or_default();
            entry.declared = Some(match entry.declared.take() {
                // A duplicate kind declaration's attrs merge additively,
                // never overwrite — the same never-loosens-on-merge
                // posture `markup_check::check`'s own vocab builder has.
                Some(existing) => merge_declared(&existing, kind),
                None => kind.clone(),
            });
        }
    }

    for &(file, hir) in files {
        for cue in &hir.cue_names {
            index
                .cues
                .entry(cue.name.clone())
                .or_default()
                .sites
                .push(HarvestSite {
                    file,
                    range: cue.range,
                });
        }
        let mut walker = SpanHarvestWalker {
            file,
            index: &mut index,
        };
        brink_ir::hir::visit::visit(hir, &mut walker);
    }

    index
}

/// Merge a duplicate `markup` declaration of the same kind name: attribute
/// names union, and `required` only ever turns on, never off (matching
/// `markup_check::check`'s vocab-merge doc).
fn merge_declared(existing: &ManifestSpanKind, incoming: &ManifestSpanKind) -> ManifestSpanKind {
    let mut attrs = existing.attrs.clone();
    for attr in &incoming.attrs {
        match attrs.iter_mut().find(|a| a.name == attr.name) {
            Some(present) => present.required |= attr.required,
            None => attrs.push(attr.clone()),
        }
    }
    ManifestSpanKind {
        name: existing.name.clone(),
        attrs,
    }
}

/// Collects markup span/attribute harvest facts for one file.
///
/// Descends by hand rather than relying purely on the shared walker for
/// the same reason `markup_check::SpanWalker` does: `HirVisitor`'s content
/// hook hands over the whole [`Content`], and the shared `walk_content_part`
/// recurses *through* a span into its children without exposing the
/// [`SpanPart`] itself.
struct SpanHarvestWalker<'a> {
    file: FileId,
    index: &'a mut HarvestIndex,
}

impl SpanHarvestWalker<'_> {
    fn harvest_span(&mut self, span: &SpanPart) {
        let range = span.ptr.text_range();
        let entry = self.index.spans.entry(span.name.clone()).or_default();
        entry.sites.push(HarvestSite {
            file: self.file,
            range,
        });
        // Attribute *values* are never harvested — span attributes are
        // static text by construction (`SyntaxKind::SPAN_ATTR_VALUE`), and
        // §4.2's schema (mirrored by `ManifestSpanAttr`) never models them
        // either; only the attribute *name* is a completion candidate.
        for (attr, _value) in &span.attrs {
            entry
                .attrs
                .entry(attr.clone())
                .or_default()
                .push(HarvestSite {
                    file: self.file,
                    range,
                });
        }
        for child in &span.children {
            self.harvest_part(child);
        }
    }

    fn harvest_part(&mut self, part: &ContentPart) {
        match part {
            ContentPart::Span(span) => self.harvest_span(span),
            // Logic nests freely inside markup and vice versa (§4.3): a
            // branch's content is its own `Content` node, delivered through
            // `enter_content` in turn — nothing to recurse into here.
            ContentPart::Text(_)
            | ContentPart::Glue
            | ContentPart::Spring
            | ContentPart::Interpolation(_)
            | ContentPart::InlineConditional(_)
            | ContentPart::InlineSequence(_) => {}
        }
    }
}

impl HirVisitor for SpanHarvestWalker<'_> {
    fn enter_content(&mut self, content: &Content, _ctx: ContentContext) {
        // `content.parts` only, not `content.tags` — see
        // `markup_check::SpanWalker::enter_content`'s doc: native's
        // `lower_tag` flattens a tag's raw tokens into one `Text` part, so
        // there is never a `Span` under a tag to harvest.
        for part in &content.parts {
            self.harvest_part(part);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use brink_ir::{ExternalKind, ManifestExternal, ManifestSpanAttr, SemanticTypeDef, TypeRef};

    fn lower_native(src: &str) -> HirFile {
        let parse = brink_syntax_native::parse(src);
        assert!(
            parse.errors().is_empty(),
            "parse errors: {:?}",
            parse.errors()
        );
        let (hir, _manifest, _diags) = brink_ir::hir::lower_native::lower(FileId(0), &parse.tree());
        hir
    }

    fn lower_native_at(file: FileId, src: &str) -> HirFile {
        let parse = brink_syntax_native::parse(src);
        assert!(
            parse.errors().is_empty(),
            "parse errors: {:?}",
            parse.errors()
        );
        let (hir, _manifest, _diags) = brink_ir::hir::lower_native::lower(file, &parse.tree());
        hir
    }

    fn attr(name: &str) -> ManifestSpanAttr {
        ManifestSpanAttr {
            name: name.to_string(),
            required: false,
            ty: None,
        }
    }

    fn required_attr(name: &str) -> ManifestSpanAttr {
        ManifestSpanAttr {
            name: name.to_string(),
            required: true,
            ty: None,
        }
    }

    // ── Cues: harvested regardless of any declared handler ──────────────

    #[test]
    fn an_unclaimed_cue_is_still_harvested() {
        // No conventions module claims this — the line reports E129 and
        // produces no `ElementMatch`/`Stmt` — but the cue's own name is
        // still harvested (§5: "harvest by default").
        let hir = lower_native("flow a() {\n  @KID\n  Says who?\n}\n");
        let index = harvest(&[(FileId(0), &hir)], None);
        assert!(
            index.cues.contains_key("KID"),
            "an unclaimed cue must still be harvested: {index:?}"
        );
    }

    #[test]
    fn a_claimed_cue_is_harvested_the_same_way() {
        // Whether a conventions handler claims the line changes nothing
        // about the raw harvest — the whole-tree scan is independent of
        // `element::try_claim`.
        let hir = lower_native(
            "@[convention(claims = \"^(?<name>[A-Z][A-Z]*)$\", order = 10, block)]\nfn cue(name: string, body: content) >{\n  {name}\n  {body}\n}\n\nflow a() {\n  @KID\n  Says who?\n}\n",
        );
        let index = harvest(&[(FileId(0), &hir)], None);
        assert!(
            index.cues.contains_key("KID"),
            "a claimed cue must still be harvested: {index:?}"
        );
    }

    #[test]
    fn a_compact_cue_name_is_harvested_too() {
        let hir = lower_native("flow a() {\n  @VENDOR: Something for the road?\n}\n");
        let index = harvest(&[(FileId(0), &hir)], None);
        assert!(
            index.cues.contains_key("VENDOR"),
            "a compact cue's name must be harvested: {index:?}"
        );
    }

    #[test]
    fn the_same_cue_name_in_two_files_completes_from_either() {
        // The load-bearing property (§5: "so completion crosses files").
        let a = lower_native_at(FileId(0), "flow a() {\n  @KID\n  Hey.\n}\n");
        let b = lower_native_at(FileId(1), "flow b() {\n  @KID\n  You again.\n}\n");
        let index = harvest(&[(FileId(0), &a), (FileId(1), &b)], None);
        let sites = &index.cues.get("KID").expect("KID harvested").sites;
        assert_eq!(sites.len(), 2, "one site per file: {sites:?}");
        let files: std::collections::BTreeSet<_> = sites.iter().map(|s| s.file).collect();
        assert_eq!(
            files,
            [FileId(0), FileId(1)].into_iter().collect(),
            "both files' occurrences must be present: {sites:?}"
        );
    }

    #[test]
    fn cue_names_are_never_harvested_from_the_ink_frontend() {
        let parse = brink_syntax::parse("=== knot ===\nHello.\n-> END\n");
        let (hir, _manifest, _diags) = brink_ir::hir::lower(FileId(0), &parse.tree());
        assert!(hir.cue_names.is_empty(), "ink grammar has no cue channel");
        let index = harvest(&[(FileId(0), &hir)], None);
        assert!(index.cues.is_empty(), "{index:?}");
    }

    // ── Markup spans: harvested by default, upgraded by the manifest ────

    #[test]
    fn an_undeclared_span_still_harvests_its_kind_and_attribute() {
        let hir = lower_native("flow a() {\n  <wave amount=\"3\">shimmer</wave>\n}\n");
        let index = harvest(&[(FileId(0), &hir)], None);
        let wave = index.spans.get("wave").expect("wave harvested");
        assert_eq!(wave.sites.len(), 1);
        assert!(wave.attrs.contains_key("amount"));
        assert!(wave.declared.is_none(), "no manifest registered: {wave:?}");
    }

    #[test]
    fn a_declared_but_never_used_span_kind_still_completes() {
        // The other half of "declaration upgrades": a kind the manifest
        // declares must complete even with zero occurrences anywhere.
        let manifest = HostManifest {
            markup: vec![ManifestSpanKind {
                name: "sfx".to_string(),
                attrs: vec![attr("name"), required_attr("volume")],
            }],
            ..HostManifest::default()
        };
        let hir = lower_native("flow a() {\n  Nothing here.\n}\n");
        let index = harvest(&[(FileId(0), &hir)], Some(&manifest));
        let sfx = index.spans.get("sfx").expect("declared kind must appear");
        assert!(sfx.sites.is_empty(), "never used: {sfx:?}");
        assert_eq!(
            sfx.declared.as_ref().expect("declared").attrs,
            vec![attr("name"), required_attr("volume")],
            "the manifest's attribute records must survive verbatim, \
             `required` included — not degraded to bare names"
        );
    }

    #[test]
    fn a_harvested_span_kind_the_manifest_also_declares_merges_both_halves() {
        let manifest = HostManifest {
            markup: vec![ManifestSpanKind {
                name: "wave".to_string(),
                attrs: vec![attr("amount")],
            }],
            ..HostManifest::default()
        };
        let hir = lower_native("flow a() {\n  <wave amount=\"3\" speed=\"2\">shimmer</wave>\n}\n");
        let index = harvest(&[(FileId(0), &hir)], Some(&manifest));
        let wave = index.spans.get("wave").expect("wave present");
        assert_eq!(wave.sites.len(), 1, "harvested occurrence: {wave:?}");
        // Both the declared attribute and the undeclared one actually
        // written are harvested — freeform stays the default even under a
        // manifest (that's `markup_check`'s job to flag, not this index's).
        assert!(wave.attrs.contains_key("amount"));
        assert!(wave.attrs.contains_key("speed"));
        assert_eq!(
            wave.declared.as_ref().expect("declared").attrs,
            vec![attr("amount")]
        );
    }

    #[test]
    fn duplicate_declared_kinds_merge_attrs_and_never_unrequire() {
        let manifest = HostManifest {
            markup: vec![
                ManifestSpanKind {
                    name: "sfx".to_string(),
                    attrs: vec![required_attr("volume")],
                },
                ManifestSpanKind {
                    name: "sfx".to_string(),
                    attrs: vec![attr("name")],
                },
            ],
            ..HostManifest::default()
        };
        let hir = lower_native("flow a() {\n  Nothing here.\n}\n");
        let index = harvest(&[(FileId(0), &hir)], Some(&manifest));
        let sfx = index.spans.get("sfx").expect("declared kind");
        let declared = sfx.declared.as_ref().expect("declared");
        let volume = declared
            .attrs
            .iter()
            .find(|a| a.name == "volume")
            .expect("volume present");
        assert!(volume.required, "must still be required after the merge");
    }

    #[test]
    fn a_manifest_with_no_markup_key_contributes_nothing_beyond_harvest() {
        let manifest = HostManifest {
            externals: vec![ManifestExternal {
                name: "play_sfx".to_string(),
                params: Vec::new(),
                returns: TypeRef::default(),
                kind: ExternalKind::default(),
                doc: None,
                widgets: Vec::new(),
                path: Vec::new(),
            }],
            types: vec![SemanticTypeDef {
                name: "actor_id".to_string(),
                base: brink_ir::BaseType::Int,
                constraint: None,
                values: None,
                widget: None,
            }],
            markup: Vec::new(),
        };
        let hir = lower_native("flow a() {\n  <glow>shine</glow>\n}\n");
        let index = harvest(&[(FileId(0), &hir)], Some(&manifest));
        let glow = index.spans.get("glow").expect("harvested regardless");
        assert!(glow.declared.is_none(), "externals-only manifest: {glow:?}");
    }

    #[test]
    fn a_nested_span_is_harvested_not_just_the_outermost() {
        let hir = lower_native("flow a() {\n  <b><glitch>hi</glitch></b>\n}\n");
        let index = harvest(&[(FileId(0), &hir)], None);
        assert!(index.spans.contains_key("b"));
        assert!(index.spans.contains_key("glitch"));
    }

    // ── HarvestNames: the range-free completion projection (#2134) ──────

    #[test]
    fn names_projection_drops_ranges_but_keeps_every_cue_and_span_name() {
        let hir = lower_native(
            "flow a() {\n  @KID\n  <wave amount=\"3\">shimmer</wave>\n  Says hi.\n}\n",
        );
        let index = harvest(&[(FileId(0), &hir)], None);
        let names = index.names();
        assert!(names.cues.contains("KID"));
        let wave = names.spans.get("wave").expect("wave harvested");
        assert!(wave.attrs.contains("amount"));
    }

    #[test]
    fn names_projection_is_eq_stable_across_a_range_only_change() {
        // The load-bearing property (#2134): two harvests of the *same*
        // name from different byte offsets must produce identical
        // `HarvestNames` output, even though the raw `HarvestIndex` (whose
        // sites carry real ranges) differs.
        let a = lower_native("flow a() {\n  @KID\n  Hi.\n}\n");
        let b = lower_native("flow a() {\n\n\n  @KID\n  Hi.\n}\n");
        let index_a = harvest(&[(FileId(0), &a)], None);
        let index_b = harvest(&[(FileId(0), &b)], None);
        assert_ne!(
            index_a, index_b,
            "sanity: the raw indexes differ by range, so this test is real"
        );
        assert_eq!(
            index_a.names(),
            index_b.names(),
            "the range-free projection must be Eq-stable across a pure range shift"
        );
    }

    #[test]
    fn names_projection_preserves_declared_span_metadata() {
        let manifest = HostManifest {
            markup: vec![ManifestSpanKind {
                name: "sfx".to_string(),
                attrs: vec![required_attr("volume")],
            }],
            ..HostManifest::default()
        };
        let hir = lower_native("flow a() {\n  Nothing here.\n}\n");
        let index = harvest(&[(FileId(0), &hir)], Some(&manifest));
        let names = index.names();
        let sfx = names.spans.get("sfx").expect("declared kind must appear");
        assert!(sfx.declared.as_ref().expect("declared").attrs[0].required);
    }
}
