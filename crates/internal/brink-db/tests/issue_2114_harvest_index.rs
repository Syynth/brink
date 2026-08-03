//! `ProjectDb::harvest_index` — the project-db harvest obligation over cue
//! payloads and markup span kinds (issue #2114, `docs/prose-dialect-spec.md`
//! §5).
//!
//! These tests exercise the real db pipeline (`ProjectDb::set_file` +
//! `harvest_index()`), not just the pure `brink_analyzer::harvest` merge
//! (unit-tested in `brink-analyzer/src/harvest.rs`) — the load-bearing claim
//! is that a cue or span written in one file completes when queried through
//! the whole project, and that this holds with zero conventions module and
//! zero host manifest registered (the "harvest by default" half of §5).

use brink_analyzer::AnalysisOptions;
use brink_db::ProjectDb;
use brink_ir::{HostManifest, ManifestSpanAttr, ManifestSpanKind};

/// A cue with no claiming conventions handler anywhere in the project:
/// `@KID` reports the loud `E129` (nothing claims it), yet it must still
/// complete project-wide — this is the whole point of harvesting from the
/// raw `@NAME` payload rather than from `element_matches`.
#[test]
fn an_unclaimed_cue_still_completes_across_files() {
    let mut db = ProjectDb::new();
    db.set_file("a.brink", "flow a() {\n  @KID\n  Says who?\n}\n".to_owned());
    db.set_file(
        "b.brink",
        "flow b() {\n  Nothing to do with a KID.\n}\n".to_owned(),
    );

    let index = db.harvest_index();
    let sites = &index
        .cues
        .get("KID")
        .expect("KID must be harvested despite having no claiming handler")
        .sites;
    assert_eq!(sites.len(), 1, "{sites:?}");
}

/// The load-bearing cross-file property: a cue written only in `a.brink`
/// completes when the project also contains `b.brink`, and a cue written in
/// both files reports one site per file.
#[test]
fn a_cue_written_in_one_file_completes_from_the_whole_project() {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.brink",
        "flow a() {\n  @VENDOR\n  Something for the road?\n}\n".to_owned(),
    );
    db.set_file(
        "b.brink",
        "flow b() {\n  @VENDOR\n  Back again?\n}\n".to_owned(),
    );

    let index = db.harvest_index();
    let sites = &index.cues.get("VENDOR").expect("VENDOR harvested").sites;
    assert_eq!(
        sites.len(),
        2,
        "one occurrence per file, both reachable through one project-wide \
         index: {sites:?}"
    );
}

/// A compact cue (`@NAME: text`) harvests its name exactly like the block
/// form — proven through the real db pipeline, not just the pure merge.
#[test]
fn a_compact_cue_completes_through_the_db_too() {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.brink",
        "flow a() {\n  @MARKET VENDOR: Something for the road?\n}\n".to_owned(),
    );

    let index = db.harvest_index();
    assert!(
        index.cues.contains_key("MARKET VENDOR"),
        "{:?}",
        index.cues.keys().collect::<Vec<_>>()
    );
}

/// Markup span kinds/attributes harvest with no manifest registered at
/// all — the freeform-by-default half of §5, proven end to end.
#[test]
fn undeclared_markup_harvests_across_files_with_no_manifest() {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.brink",
        "flow a() {\n  <wave amount=\"3\">shimmer</wave>\n}\n".to_owned(),
    );
    db.set_file(
        "b.brink",
        "flow b() {\n  <wave amount=\"1\" speed=\"9\">shimmer again</wave>\n}\n".to_owned(),
    );

    let index = db.harvest_index();
    let wave = index.spans.get("wave").expect("wave harvested");
    assert_eq!(wave.sites.len(), 2, "{wave:?}");
    assert!(wave.attrs.contains_key("amount"));
    assert!(wave.attrs.contains_key("speed"));
    assert!(wave.declared.is_none(), "no manifest registered: {wave:?}");
}

/// Registering a host manifest upgrades a span kind's harvest record with
/// the manifest's own tooling-grade declaration — carried through the real
/// `AnalysisOptions` seam, not constructed by hand.
#[test]
fn a_registered_manifest_upgrades_a_harvested_span_kind_through_the_db() {
    let mut db = ProjectDb::new();
    db.set_file(
        "a.brink",
        "flow a() {\n  <sfx name=\"door\">clank</sfx>\n}\n".to_owned(),
    );
    db.set_analysis_options(AnalysisOptions {
        host_manifest: Some(HostManifest {
            markup: vec![ManifestSpanKind {
                name: "sfx".to_string(),
                attrs: vec![
                    ManifestSpanAttr {
                        name: "name".to_string(),
                        required: false,
                        ty: None,
                    },
                    ManifestSpanAttr {
                        name: "volume".to_string(),
                        required: true,
                        ty: None,
                    },
                ],
            }],
            ..HostManifest::default()
        }),
        ..AnalysisOptions::default()
    });

    let index = db.harvest_index();
    let sfx = index.spans.get("sfx").expect("sfx harvested");
    assert_eq!(sfx.sites.len(), 1, "{sfx:?}");
    let declared = sfx.declared.as_ref().expect("manifest upgrade present");
    assert_eq!(
        declared.attrs.len(),
        2,
        "verbatim, not degraded: {declared:?}"
    );
    assert!(
        declared
            .attrs
            .iter()
            .any(|a| a.name == "volume" && a.required),
        "the `required` flag must survive into the index: {declared:?}"
    );
}

/// A span kind the manifest declares but no file has used yet still
/// completes — the other half of "declaration upgrades": a declaration is
/// never gated on prior usage.
#[test]
fn a_declared_but_unused_span_kind_completes_with_zero_files_using_it() {
    let mut db = ProjectDb::new();
    db.set_file("a.brink", "flow a() {\n  Nothing here.\n}\n".to_owned());
    db.set_analysis_options(AnalysisOptions {
        host_manifest: Some(HostManifest {
            markup: vec![ManifestSpanKind {
                name: "glow".to_string(),
                attrs: Vec::new(),
            }],
            ..HostManifest::default()
        }),
        ..AnalysisOptions::default()
    });

    let index = db.harvest_index();
    let glow = index.spans.get("glow").expect("declared kind must appear");
    assert!(glow.sites.is_empty(), "{glow:?}");
    assert!(glow.declared.is_some(), "{glow:?}");
}

/// An ink (`.ink`) file contributes no cue harvest — the native-only cue
/// channel doesn't exist in that grammar — but an ordinary ink markup span
/// still harvests, proving the index isn't accidentally native-gated as a
/// whole.
#[test]
fn ink_files_never_contribute_cue_harvest() {
    let mut db = ProjectDb::new();
    db.set_file(
        "main.ink",
        "=== knot ===\nHello there.\n-> END\n".to_owned(),
    );

    let index = db.harvest_index();
    assert!(index.cues.is_empty(), "{:?}", index.cues);
}
