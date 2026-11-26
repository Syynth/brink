//! Incremental == from-scratch fuzz harness (scripting-substrate spec §6.2).
//!
//! Applies a sequence of random-but-*seeded* edits to corpus-derived
//! projects through a long-lived `ProjectDb` and asserts, after every edit,
//! that the incremental `story_data()` is bit-identical to a fresh compile
//! of the same sources in a brand-new db. This is the safety net for
//! early-cutoff and query-purity mistakes: salsa owns dependency tracking,
//! but *we* own query purity.
//!
//! Determinism: a fixed-constant LCG drives every choice — no clock or OS
//! entropy anywhere. Edits never touch `INCLUDE` lines, so the discovered
//! file set stays fixed (a long-lived db intentionally keeps orphaned files;
//! a from-scratch discovery would not — that difference is pre-existing
//! LSP-vs-compiler behavior, not what this harness tests).
//!
//! Remove/re-add churn (#536): some steps on multi-file projects tombstone
//! a non-entry file (`remove_file`), pull `story_data()` with the file
//! absent (exercising missing-INCLUDE resolution against a tombstoned
//! input), then re-add the path with mutated content before the compare —
//! so every comparison still sees the full file set. The re-added path must
//! reuse its original `FileId` (durable path→id identity), which is exactly
//! what keeps the incremental ids aligned with the fresh db's and lets the
//! bit-identical assertion double as the tombstone-purity check: any stale
//! memo surviving the remove/re-add round-trip diverges from the fresh
//! compile.
//!
//! Call-graph topology op (issue #639): every other mutation op edits a
//! def's body without changing which defs call which — a pure body edit
//! never changes `scc_membership()`'s condensation, only
//! `call_edges(def)`-level topology changes do (design doc §9's FG-2
//! bullet). `mutate()`'s op 7 adds or removes a `~ callee(args)` call or a
//! `-> callee(args)` divert-with-args edge between defs that already exist
//! in the project (self-recursion counts — it turns a singleton SCC into a
//! self-loop), marked so the same op can find and remove it again later.
//! This is on top of, not instead of, FG-2's own hand-crafted
//! `fg2_scc_dependency_edges.rs` regression test.
//!
//! Topology-thin corpus (issue #986): the original `PROJECTS` list was
//! mostly shallow, few-edge call graphs, so op 7 had little real
//! dependency-graph structure to churn against. `tier2/callgraph/*` adds
//! purpose-built seeds with deeper/wider shapes — a call chain, a
//! diamond (fan-out/fan-in with a shared callee), and a genuine cycle built
//! from mutually-tunneling knots — see `PROJECTS`'s doc comment for the
//! per-fixture rationale. Deliberately excluded from the oracle corpus (no
//! `oracle/` subdirectory) so they add real topology without moving the
//! ratchet.
//!
//! Bounded: `EDITS_PER_PROJECT` edits over a fixed project list; every step
//! is a small-project compile, so the whole test stays well under the
//! workspace's test-time budget and cannot hang (no loops without bounds).

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use brink_driver::ProjectDb;
use regex::Regex;

/// Seeded edits applied to each project.
const EDITS_PER_PROJECT: u64 = 16;

/// Corpus-derived projects under `tests/` (workspace root), chosen to cover
/// single-file, flat-include, and nested-include shapes.
///
/// `tier2/callgraph/*` (issue #986): purpose-built fixtures — not part of
/// the oracle corpus (no `oracle/` subdirectory, so `collect_oracle_cases`
/// never sees them and the ratchet is untouched) — that give the call-graph
/// topology op (issue #639, op 7 in `mutate()`) real dependency-graph depth
/// and width to churn, instead of the few-edges-thin shapes the rest of
/// `PROJECTS` happens to have:
///
/// - `deep-chain`: a 5-deep function-call chain and a parallel 5-deep
///   knot-divert chain, so an added/removed edge can land mid-chain and
///   shift `scc_membership()`'s condensation across real depth.
/// - `diamond-shared-callee`: a fan-out/fan-in shape at both the function
///   level (`diamond_top` calls `diamond_left` and `diamond_right`, both of
///   which call the shared `diamond_bottom`) and the knot-divert level (a
///   choice fans out to two branches that reconverge), exercising a
///   dependency graph with real width and a shared descendant.
/// - `cycle-via-tunnels`: a genuine 3-node call-graph cycle (`ping` tunnels
///   to `pong`, `pong` tunnels to `ring`, `ring` tunnels back to `ping`,
///   depth-guarded so it still terminates), giving op 7 a non-trivial SCC
///   (size > 1) to add/remove edges against instead of every other
///   project's DAG-only shape.
const PROJECTS: &[&str] = &[
    "tier1/basics/I002-fogg-comforts-passepartout",
    "tier1/weave/I041-weave-gathers",
    "tier1/knots/I128-knot-stitch-gather-counts",
    "tier2/variabletext/sequence",
    "tier2/functions/using-function-and-increment-together",
    "tier2/callgraph/deep-chain",
    "tier2/callgraph/diamond-shared-callee",
    "tier2/callgraph/cycle-via-tunnels",
    "tier3/misc/I024-includes",
    "tier3/misc/I025-nested-includes",
];

/// Tiny deterministic PRNG (LCG step, fixed constant seed per project) — the
/// same shape the `compile_bench` harness uses. No entropy sources.
struct Lcg(u64);

impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }

    fn next(&mut self) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0 >> 33
    }

    fn pick(&mut self, n: usize) -> usize {
        usize::try_from(self.next()).unwrap_or(0) % n.max(1)
    }
}

fn workspace_root() -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.pop(); // brink-test-harness
    p.pop(); // internal
    p.pop(); // crates
    p
}

/// Load every `.ink` file in a case directory as `filename → text`,
/// flat-keyed the way the corpus INCLUDEs reference them.
fn load_project(dir: &Path) -> BTreeMap<String, String> {
    let mut files = BTreeMap::new();
    let mut entries: Vec<_> = std::fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()))
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|ext| ext == "ink"))
        // `.ink.json` has extension "json"; plain `.ink` only.
        .collect();
    entries.sort();
    for path in entries {
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let text = std::fs::read_to_string(&path).unwrap();
        files.insert(name, text);
    }
    assert!(
        files.contains_key("story.ink"),
        "{} has no story.ink",
        dir.display()
    );
    files
}

/// A top-level knot/function def discovered by scanning the project's
/// *original* source for `== name ==` / `=== function name(params) ===`
/// style headers (issue #639). Single-`=` stitch headers are excluded —
/// stitches need a dotted, parent-knot-qualified name to call from outside
/// their own knot, which this synthetic edge doesn't model.
///
/// Computed once per project from `originals` (headers are never added or
/// removed by any mutation op, this one included — only the call/divert
/// *sites* referencing them churn), so every step's `mutate()` call reuses
/// the same candidate list.
struct CallableDef {
    name: String,
    arity: usize,
}

/// Marks a call/divert-with-args line this file's `mutate()` op 7 inserted,
/// so a later invocation of the same op can find and remove it again (the
/// "remove" half of "add/remove a call-graph edge"). A valid ink line
/// comment, so it never changes the statement it trails.
const CALL_EDGE_MARKER: &str = "// fz-call-edge";

/// Scan every file's text for def headers eligible as a call/divert-with-
/// args target: `={2,}` (excludes single-`=` stitches), an optional
/// `function` keyword, a name, and an optional parenthesized param list
/// whose comma count gives the arity a synthetic call site must match.
fn discover_callable_defs(files: &BTreeMap<String, String>) -> Vec<CallableDef> {
    let header = Regex::new(
        r"^\s*={2,}\s*(?:function\s+)?([A-Za-z_][A-Za-z0-9_]*)\s*(?:\(([^)]*)\))?\s*=*\s*$",
    )
    .expect("valid regex");
    let mut defs: Vec<CallableDef> = Vec::new();
    for text in files.values() {
        for line in text.lines() {
            let Some(caps) = header.captures(line) else {
                continue;
            };
            let name = caps[1].to_owned();
            let arity = caps.get(2).map_or(0, |params| {
                let params = params.as_str().trim();
                if params.is_empty() {
                    0
                } else {
                    params.split(',').count()
                }
            });
            defs.push(CallableDef { name, arity });
        }
    }
    defs.sort_by(|a, b| a.name.cmp(&b.name));
    defs.dedup_by(|a, b| a.name == b.name);
    defs
}

/// One seeded mutation of one file. Never creates or deletes `INCLUDE`
/// lines, so the project file set is stable across the whole run.
fn mutate(
    rng: &mut Lcg,
    original: &str,
    current: &str,
    step: u64,
    callable_defs: &[CallableDef],
) -> String {
    let lines: Vec<&str> = current.lines().collect();
    // Line indices that are safe to touch (not INCLUDE).
    let safe: Vec<usize> = (0..lines.len())
        .filter(|&i| !lines[i].trim_start().starts_with("INCLUDE"))
        .collect();
    if safe.is_empty() {
        return current.to_owned();
    }

    let op = rng.pick(9);
    let at = safe[rng.pick(safe.len())];
    let mut out: Vec<String> = lines.iter().map(|&l| l.to_owned()).collect();
    match op {
        // Insert a plain text line.
        0 => out.insert(at, format!("A fuzzed line, step {step}.")),
        // Delete a line (keep at least one line).
        1 => {
            if out.len() > 1 {
                out.remove(at);
            }
        }
        // Duplicate a line.
        2 => {
            let dup = out[at].clone();
            out.insert(at, dup);
        }
        // Whitespace-only churn: trailing spaces shift every later range
        // without changing structure — the resolution_index cutoff exercise.
        3 => out[at] = format!("{}  ", out[at]),
        // Insert a temp declaration (locals-in-index exercise; may be
        // invalid at top level, which exercises the diagnostics path).
        4 => out.insert(at, format!("~ temp fz_{step} = {}", rng.pick(9))),
        // VAR/CONST initializer edit (FG-2.1, issue #638): mutate a
        // declared global's initializer value in place, when the chosen
        // line happens to be one — exercises the lazy-globals edge
        // (`referenced_globals`/`signature_query`'s narrow `BodyCtx.globals`
        // map, Ruling 1) under incremental-equals-fresh. A body edit is
        // already covered by the other ops; this specifically targets the
        // *declaration* a body's global lookup resolves through. Falls back
        // to a plain line insert if this line isn't a VAR/CONST decl (still
        // a valid, bounded mutation either way).
        5 => {
            let trimmed = out[at].trim_start();
            let keyword = if trimmed.starts_with("VAR ") {
                Some("VAR")
            } else if trimmed.starts_with("CONST ") {
                Some("CONST")
            } else {
                None
            };
            match keyword.and_then(|kw| {
                trimmed[kw.len() + 1..]
                    .split_once('=')
                    .map(|(name, _)| (kw, name))
            }) {
                Some((kw, name)) => out[at] = format!("{kw} {name}= {}", rng.pick(999)),
                None => out.insert(at, format!("A fuzzed line, step {step}.")),
            }
        }
        // Inline `///` doc churn (FG-3 completion, issue #750): insert a
        // doc comment line — when the next line happens to be a
        // declaration (VAR/CONST/knot/EXTERNAL), this attaches real doc
        // content, exercising the inline_docs/value-meta/external-meta
        // family's incremental path; anywhere else it is harmless comment
        // churn that must still leave every query equal to fresh.
        6 => out.insert(at, format!("/// Fuzzed doc, step {step}.")),
        // Call-graph topology op (issue #639): add or remove a call/
        // divert-with-args edge between defs that already exist in this
        // project — a body-only edit never changes `scc_membership()`'s
        // condensation, only a topology change does, per the design doc's
        // FG-2 ask that this harness fuzz that class of edit too.
        7 => {
            if callable_defs.is_empty() {
                // No callable def exists in this project shape (a
                // single-flow, no-knot corpus case) — harmless fallback,
                // same shape as op 5's "not applicable here" branch.
                out.insert(at, format!("A fuzzed line, step {step}."));
            } else if let Some(idx) = out.iter().position(|l| l.contains(CALL_EDGE_MARKER)) {
                // Remove: a prior invocation of this op already added a
                // marked edge somewhere in the file — strip it. This is
                // the topology-*removing* half of add/remove.
                out.remove(idx);
            } else {
                // Add: a call or divert-with-args edge to a def that
                // already exists in the project. Self-recursion is a
                // legitimate topology change too — it turns a singleton
                // SCC into a self-loop.
                let callee = &callable_defs[rng.pick(callable_defs.len())];
                let args = (0..callee.arity)
                    .map(|_| rng.pick(9).to_string())
                    .collect::<Vec<_>>()
                    .join(", ");
                let line = if rng.pick(2) == 0 {
                    format!("~ {}({args}) {CALL_EDGE_MARKER}", callee.name)
                } else {
                    format!("-> {}({args}) {CALL_EDGE_MARKER}", callee.name)
                };
                out.insert(at, line);
            }
        }
        // Revert the file to its original content.
        _ => return original.to_owned(),
    }
    let mut s = out.join("\n");
    s.push('\n');
    s
}

/// Compile `files` in a fresh db (same insertion order as the incremental
/// db, so `FileId`s line up) and return its `CompileProduct`.
fn fresh_compile(files: &BTreeMap<String, String>) -> brink_driver::ProjectDb {
    let mut db = ProjectDb::new();
    for (path, text) in files {
        db.set_file(path, text.clone());
    }
    db.set_entry("story.ink").expect("story.ink present");
    db
}

/// Serialize a compiled story to `.inkb` bytes for the bit-identical check.
fn story_bytes(product: &brink_driver::CompileProduct) -> Option<Vec<u8>> {
    product.story.as_ref().map(|story| {
        let mut buf = Vec::new();
        brink_format::write_inkb(story, &mut buf);
        buf
    })
}

/// FG-3 (issue #632): assert the decomposed `analysis_query` family agrees
/// between a long-lived incremental db and a from-scratch one — the
/// RESOLUTIONS/INDEX half (`resolutions_index()`) and every file's per-file
/// diagnostic contributors (`per_file_diagnostics(file)`). Extracted from
/// the main fuzz loop to keep it under clippy's line-count lint.
fn assert_fg3_families_match(
    db: &ProjectDb,
    fresh_db: &ProjectDb,
    project: &str,
    paths: &[String],
    step: u64,
) {
    assert_eq!(
        *db.resolutions_index(),
        *fresh_db.resolutions_index(),
        "{project}: resolutions_index() diverged from fresh compile after edit {step}"
    );
    for path in paths {
        let incremental_id = db.file_id(path);
        let fresh_id = fresh_db.file_id(path);
        assert_eq!(
            incremental_id, fresh_id,
            "{project}: file id for {path} diverged between incremental and fresh"
        );
        if let Some(id) = incremental_id {
            assert_eq!(
                db.per_file_diagnostics(id),
                fresh_db.per_file_diagnostics(id),
                "{project}: per_file_diagnostics({path}) diverged from fresh compile after edit {step}"
            );
            // Issue #750 (FG-3 completion): the decomposed external-check
            // family's per-file contributors must agree too — value metas
            // (VAR/CONST/LIST enrichment, exercised by the doc-churn and
            // initializer-edit mutations) and call-site diagnostics.
            assert_eq!(
                db.file_value_meta(id),
                fresh_db.file_value_meta(id),
                "{project}: file_value_meta({path}) diverged from fresh compile after edit {step}"
            );
            assert_eq!(
                db.file_call_site_diagnostics(id),
                fresh_db.file_call_site_diagnostics(id),
                "{project}: file_call_site_diagnostics({path}) diverged from fresh compile after edit {step}"
            );
        }
    }
}

/// FG-1 (#630)/FG-2 (#631)/T2-1 (#860, swept in via #746): assert every
/// per-def/per-SCC query family agrees between a long-lived incremental db
/// and a from-scratch one, for every non-local def:
///
/// - `signature(def)` (FG-1) — the equivalence gate the design doc asks
///   `incremental_fuzz` to carry for the narrowed dependency edge
///   (declaring-file-only, not every project file's HIR);
/// - `inferred_signature(def)`/`infer_body(def)` (FG-2) — the per-def/
///   per-SCC inference views, per the design doc §7's explicit ask to sweep
///   every new per-def family into this harness;
/// - `effects(def)` (T2-1, #860) — the same shape of per-def/per-SCC query,
///   sited beside `inferred_signature` in `brink-db` with its own per-SCC
///   fixpoint (`solve_scc_effects`), so it gets the identical gate.
///
/// Extracted from the main fuzz loop to keep it under clippy's line-count
/// lint, the same reason `assert_fg3_families_match` above was extracted.
fn assert_per_def_families_match(db: &ProjectDb, fresh_db: &ProjectDb, project: &str, step: u64) {
    let mut defs: Vec<_> = db
        .symbol_index()
        .symbols
        .iter()
        .filter(|(_, info)| {
            !matches!(
                info.kind,
                brink_ir::SymbolKind::Param | brink_ir::SymbolKind::Temp
            )
        })
        .map(|(id, _)| *id)
        .collect();
    defs.sort();
    for def in defs {
        assert_eq!(
            db.signature(def),
            fresh_db.signature(def),
            "{project}: signature({def:?}) diverged from fresh compile after edit {step}"
        );
        assert_eq!(
            db.inferred_signature(def),
            fresh_db.inferred_signature(def),
            "{project}: inferred_signature({def:?}) diverged from fresh compile after edit {step}"
        );
        assert_eq!(
            db.infer_body(def),
            fresh_db.infer_body(def),
            "{project}: infer_body({def:?}) diverged from fresh compile after edit {step}"
        );
        assert_eq!(
            db.effects(def),
            fresh_db.effects(def),
            "{project}: effects({def:?}) diverged from fresh compile after edit {step}"
        );
    }
}

#[test]
fn incremental_story_data_equals_fresh_compile() {
    let root = workspace_root().join("tests");
    let mut churn_steps = 0usize;

    for (p_idx, project) in PROJECTS.iter().enumerate() {
        let dir = root.join(project);
        let originals = load_project(&dir);
        let mut current = originals.clone();
        let paths: Vec<String> = current.keys().cloned().collect();
        // Call-graph topology op (#639): the candidate callee list is
        // fixed per project — def headers are never added/removed by any
        // mutation op, only the call/divert sites referencing them churn.
        let callable_defs = discover_callable_defs(&originals);

        // Long-lived incremental db.
        let mut db = ProjectDb::new();
        for (path, text) in &current {
            db.set_file(path, text.clone());
        }
        db.set_entry("story.ink").expect("story.ink present");

        // Non-entry files are eligible for remove/re-add churn (#536); the
        // entry must stay present so `story_data()` is always comparable.
        let churnable: Vec<String> = paths
            .iter()
            .filter(|p| *p != "story.ink")
            .cloned()
            .collect();

        // Baseline: before any edit, incremental == fresh.
        let mut rng = Lcg::new(0x5EED_0501 + p_idx as u64);
        for step in 0..=EDITS_PER_PROJECT {
            if step > 0 {
                let churn = !churnable.is_empty() && rng.pick(4) == 0;
                if churn {
                    churn_steps += 1;
                    // Remove a file, pull the pipeline with it absent, then
                    // re-add it (with fresh content) before the compare.
                    let path = churnable[rng.pick(churnable.len())].clone();
                    let id_before = db.file_id(&path);
                    db.remove_file(&path);
                    assert_eq!(
                        db.file_id(&path),
                        None,
                        "{project}: removed file still visible"
                    );
                    // Exercise the query graph in the removed state — any
                    // INCLUDE of this path must now miss. Output is not
                    // compared here (the file set differs from `current`).
                    let _ = db.story_data().expect("entry still set");
                    let edited = mutate(
                        &mut rng,
                        &originals[&path],
                        &current[&path],
                        step,
                        &callable_defs,
                    );
                    current.insert(path.clone(), edited.clone());
                    let id_after = db.set_file(&path, edited);
                    assert_eq!(
                        Some(id_after),
                        id_before,
                        "{project}: re-added path must reuse its FileId (step {step})"
                    );
                } else {
                    let path = &paths[rng.pick(paths.len())];
                    let edited = mutate(
                        &mut rng,
                        &originals[path],
                        &current[path],
                        step,
                        &callable_defs,
                    );
                    current.insert(path.clone(), edited.clone());
                    db.update_file(path, edited);
                }
            }

            let incremental = db.story_data().expect("entry set");
            let fresh_db = fresh_compile(&current);
            let fresh = fresh_db.story_data().expect("entry set");

            assert_eq!(
                incremental, fresh,
                "{project}: incremental != from-scratch after edit {step}"
            );
            assert_eq!(
                story_bytes(incremental),
                story_bytes(fresh),
                "{project}: serialized StoryData differs after edit {step}"
            );

            // FG-3 (#632): the decomposed analysis_query family — the
            // RESOLUTIONS/INDEX half and every file's per-file diagnostic
            // contributors — must agree between the long-lived incremental
            // db and a from-scratch one, the same equivalence gate FG-1/FG-2
            // carry for their own new query families (design doc §7's
            // explicit sweep-every-new-family ask).
            assert_fg3_families_match(&db, &fresh_db, project, &paths, step);

            // FG-1/FG-2/T2-1 per-def query families — extracted to its own
            // function (see its doc) to keep this loop under clippy's
            // line-count lint, the same reason `assert_fg3_families_match`
            // was extracted above.
            assert_per_def_families_match(&db, &fresh_db, project, step);
        }
    }

    // The remove/re-add coverage (#536) must actually run: the seeded
    // driver hits the churn arm on the multi-file projects every time, and
    // this assertion keeps that coverage from silently vanishing if the
    // project list or edit mix changes.
    assert!(
        churn_steps > 0,
        "no remove/re-add churn steps executed — #536 coverage lost"
    );
}
