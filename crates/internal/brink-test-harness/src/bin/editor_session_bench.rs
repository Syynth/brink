//! Editor-session memory profiling harness (#529).
//!
//! Drives a long, deterministic, editor-style session through a single
//! long-lived [`ProjectDb`] — the same object `brink-lsp` and `brink-ide`'s
//! `IdeSession` keep alive across edits (see `compile_bench`, #498) — and
//! reports salsa memo-table/heap growth per query kind at fixed checkpoints.
//!
//! This is the measurement half of the scripting-substrate spec's §8 risk
//! ("memory growth of memo tables… use salsa LRU on the heavy queries
//! (`parse`, `lir`) and measure in the session-length editor tests").
//! Setting actual LRU capacities is explicitly OUT of scope here (#529) —
//! this harness only reports; tuning is a follow-up ruling once this data
//! exists.
//!
//! # Session shape
//!
//! A synthetic project of [`BASE_FILES`] permanent files (never removed) plus
//! [`SCRATCH_SLOTS`] scratch files that get added and removed over the
//! session — modeling a real editor's "create a new file, delete it later"
//! churn. Each edit is one of, weighted by a fixed seeded PRNG (no wall-clock
//! or OS entropy anywhere — byte-identical on every run, every machine):
//!
//! - **Typing** (55%): regenerate one base file's knot body text. Decl
//!   *names* never change (only content), matching a real "type inside a
//!   knot" edit.
//! - **`VarValue`** (15%): regenerate one base file's `VAR` initializers only
//!   — body text untouched. Decl shape (name/kind) is unchanged, only a
//!   literal differs.
//! - **Whitespace** (15%): change only trailing blank-line count in one base
//!   file — the cutoff-seam case #517 exists to make cheap.
//! - **Churn** (15%): toggle one scratch slot: add a fresh scratch file
//!   (bumping its own revision counter, so re-adding a path never reuses old
//!   content) or remove the currently-present one, rewriting `main.ink`'s
//!   `INCLUDE` list either way. `ProjectDb::set_file` mints a *new* `FileId`
//!   for a path that was previously removed (`remove_file` only forgets
//!   `ProjectDb`'s own path→id mapping) — salsa's old `SourceFile` input and
//!   every memo keyed on its old id are never reachable again but are never
//!   reclaimed either. This is the harness's most direct probe of session
//!   memory growth.
//!
//! Every edit re-pulls diagnostics for the touched file (an LSP publishing
//! diagnostics after a keystroke); every [`STORY_DATA_EVERY`]th edit also
//! pulls `story_data()` (a "play"/"preview" action), verifying the project
//! still compiles clean throughout.
//!
//! Run with:
//!
//! ```sh
//! cargo run --release -p brink-test-harness --bin editor_session_bench [-- --edits N]
//! ```
#![expect(
    clippy::print_stdout,
    reason = "benchmark harness: the printed table is the product (same stance as compile_bench/corpus_report)"
)]

use std::collections::BTreeSet;
use std::fmt::Write as _;

use brink_db::{IngredientKind, IngredientMemory, ProjectDb};

const DEFAULT_EDITS: usize = 500;
const CHECKPOINT_EVERY: usize = 50;
const STORY_DATA_EVERY: usize = 20;

const BASE_FILES: usize = 16;
const BASE_KNOTS: usize = 10;
const SCRATCH_SLOTS: usize = 3;
const SCRATCH_KNOTS: usize = 4;

/// Fixed seed for the edit-kind/target driver. Distinct from the content
/// generators' own per-(file, revision) seeding below — this one only
/// decides *which* edit happens next.
const EDIT_DRIVER_SEED: u64 = 0xED17_0525_5E55_10ED;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let edits = parse_edits()?;

    println!(
        "editor_session_bench | session | base_files={BASE_FILES} scratch_slots={SCRATCH_SLOTS} edits={edits} checkpoint_every={CHECKPOINT_EVERY} story_data_every={STORY_DATA_EVERY}"
    );

    let mut db = ProjectDb::new();
    let mut session = Session::new();

    for f in 0..BASE_FILES {
        db.set_file(&file_path(f), generate_file(f, session.revisions[f]));
    }
    db.set_file("main.ink", generate_main(session.scratch_present));
    db.set_entry("main.ink")
        .ok_or("failed to set entry point main.ink")?;

    verify_clean(&db, "initial load")?;

    let mut checkpoints: Vec<(usize, Vec<IngredientMemory>)> = vec![(0, checkpoint(&db, 0, 0))];

    let mut rng = Lcg::new(EDIT_DRIVER_SEED);
    let mut tally = EditTally::default();

    for edit_index in 1..=edits {
        let edit = pick_edit(&mut rng);
        tally.record(&edit);
        apply(&mut db, &mut session, edit);

        if let Some(id) = db.file_id(&touched_path(&edit)) {
            // LSP-style: publish diagnostics for the file just edited. This
            // pulls the whole chain (parse -> lowered -> ... -> analysis)
            // for a single-file-scoped call.
            let _ = db.diagnostics(id);
        }

        if edit_index % STORY_DATA_EVERY == 0 {
            verify_clean(&db, &format!("edit {edit_index}"))?;
        }

        if edit_index % CHECKPOINT_EVERY == 0 || edit_index == edits {
            let idx = checkpoints.len();
            checkpoints.push((edit_index, checkpoint(&db, idx, edit_index)));
        }
    }

    println!(
        "editor_session_bench | edit_tally | typing={} var_value={} whitespace={} churn={} scratch_ever_added={}",
        tally.typing, tally.var_value, tally.whitespace, tally.churn, session.scratch_ever_added
    );

    if let (Some(first), Some(last)) = (checkpoints.first(), checkpoints.last()) {
        print_growth(&first.1, &last.1, first.0, last.0);
    }

    Ok(())
}

// ── CLI ──────────────────────────────────────────────────────────────

fn parse_edits() -> Result<usize, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [] => Ok(DEFAULT_EDITS),
        [flag, value] if flag == "--edits" => value
            .parse::<usize>()
            .map_err(|e| format!("--edits {value}: {e}"))
            .and_then(|n| {
                if n == 0 {
                    Err("--edits must be >= 1".to_string())
                } else {
                    Ok(n)
                }
            }),
        other => Err(format!(
            "unsupported arguments: {} (only --edits N is supported)",
            other.join(" ")
        )),
    }
}

// ── Session state + edit application ────────────────────────────────

/// Per-file revision counters, threaded independently so a `Typing` edit
/// only changes body text, a `VarValue` edit only changes `VAR` literals,
/// and a `Whitespace` edit only changes trailing blank-line count — each
/// edit kind changes exactly the thing its name says and nothing else.
#[derive(Debug, Clone, Copy, Default)]
struct FileRevision {
    text: u64,
    var: u64,
    blank: u64,
}

struct Session {
    revisions: Vec<FileRevision>,
    scratch_present: [bool; SCRATCH_SLOTS],
    scratch_variant: [u64; SCRATCH_SLOTS],
    /// Total number of absent -> present transitions across all scratch
    /// slots — each one mints a fresh `FileId`/`SourceFile` (see module
    /// docs), so this is the expected lower bound on struct-table growth.
    scratch_ever_added: usize,
}

impl Session {
    fn new() -> Self {
        Self {
            revisions: vec![FileRevision::default(); BASE_FILES],
            scratch_present: [false; SCRATCH_SLOTS],
            scratch_variant: [0; SCRATCH_SLOTS],
            scratch_ever_added: 0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
enum EditKind {
    Typing(usize),
    VarValue(usize),
    Whitespace(usize),
    Churn(usize),
}

#[derive(Debug, Default)]
struct EditTally {
    typing: usize,
    var_value: usize,
    whitespace: usize,
    churn: usize,
}

impl EditTally {
    fn record(&mut self, edit: &EditKind) {
        match edit {
            EditKind::Typing(_) => self.typing += 1,
            EditKind::VarValue(_) => self.var_value += 1,
            EditKind::Whitespace(_) => self.whitespace += 1,
            EditKind::Churn(_) => self.churn += 1,
        }
    }
}

fn pick_edit(rng: &mut Lcg) -> EditKind {
    let roll = rng.pick(0, 99);
    let target_file = rng.pick(0, BASE_FILES - 1);
    if roll < 55 {
        EditKind::Typing(target_file)
    } else if roll < 70 {
        EditKind::VarValue(target_file)
    } else if roll < 85 {
        EditKind::Whitespace(target_file)
    } else {
        EditKind::Churn(rng.pick(0, SCRATCH_SLOTS - 1))
    }
}

fn touched_path(edit: &EditKind) -> String {
    match *edit {
        EditKind::Typing(f) | EditKind::VarValue(f) | EditKind::Whitespace(f) => file_path(f),
        EditKind::Churn(_) => "main.ink".to_string(),
    }
}

fn apply(db: &mut ProjectDb, session: &mut Session, edit: EditKind) {
    match edit {
        EditKind::Typing(f) => {
            session.revisions[f].text += 1;
            db.update_file(&file_path(f), generate_file(f, session.revisions[f]));
        }
        EditKind::VarValue(f) => {
            session.revisions[f].var += 1;
            db.update_file(&file_path(f), generate_file(f, session.revisions[f]));
        }
        EditKind::Whitespace(f) => {
            session.revisions[f].blank += 1;
            db.update_file(&file_path(f), generate_file(f, session.revisions[f]));
        }
        EditKind::Churn(slot) => {
            if session.scratch_present[slot] {
                db.remove_file(&scratch_path(slot));
                session.scratch_present[slot] = false;
            } else {
                session.scratch_variant[slot] += 1;
                session.scratch_ever_added += 1;
                db.set_file(
                    &scratch_path(slot),
                    generate_scratch_file(slot, session.scratch_variant[slot]),
                );
                session.scratch_present[slot] = true;
            }
            db.update_file("main.ink", generate_main(session.scratch_present));
        }
    }
}

fn verify_clean(db: &ProjectDb, when: &str) -> Result<(), String> {
    let product = db
        .story_data()
        .ok_or_else(|| format!("{when}: story_data unavailable (no entry set)"))?;
    if !product.errors.is_empty() {
        let detail = product
            .errors
            .iter()
            .take(5)
            .map(|d| {
                let path = db.file_path(d.file).unwrap_or("<unknown file>");
                format!("{path}: {}", d.message)
            })
            .collect::<Vec<_>>()
            .join("; ");
        return Err(format!(
            "{when}: synthetic session produced compile errors: {detail}"
        ));
    }
    Ok(())
}

// ── Synthetic project generation ────────────────────────────────────

fn file_path(f: usize) -> String {
    format!("file_{f:02}.ink")
}

fn scratch_path(slot: usize) -> String {
    format!("scratch_{slot}.ink")
}

/// Tiny deterministic PRNG (PCG-style LCG step), same shape as
/// `compile_bench`'s. No wall-clock or OS entropy anywhere.
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

    fn pick(&mut self, lo: usize, hi: usize) -> usize {
        lo + usize::try_from(self.next()).unwrap_or(0) % (hi - lo + 1)
    }
}

const WORDS: [&str; 24] = [
    "lantern", "harbor", "signal", "vault", "ember", "cipher", "meadow", "static", "orchard",
    "beacon", "drift", "hollow", "ledger", "murmur", "quarry", "relay", "sable", "tundra",
    "vesper", "wharf", "zenith", "gable", "isthmus", "keel",
];

fn sentence(rng: &mut Lcg, min_words: usize, max_words: usize) -> String {
    let n = rng.pick(min_words, max_words);
    let mut words = Vec::with_capacity(n);
    for i in 0..n {
        let w = WORDS[rng.pick(0, WORDS.len() - 1)];
        if i == 0 {
            let mut chars = w.chars();
            let first = chars.next().map(|c| c.to_ascii_uppercase());
            words.push(first.into_iter().chain(chars).collect::<String>());
        } else {
            words.push(w.to_string());
        }
    }
    let mut s = words.join(" ");
    s.push('.');
    s
}

/// Mix a file/slot index and a revision counter into one seed so distinct
/// (index, revision) pairs diverge quickly.
fn seed_mix(index: usize, revision: u64) -> u64 {
    let index = u64::try_from(index).unwrap_or(0);
    index
        .wrapping_mul(0x9E37_79B9_7F4A_7C15)
        .wrapping_add(revision)
}

/// `main.ink`: INCLUDEs every base file plus whichever scratch files are
/// currently present, then diverts into the base project's entry knot.
fn generate_main(scratch_present: [bool; SCRATCH_SLOTS]) -> String {
    let mut s = String::from("// Editor-session synthetic project — generated, deterministic.\n");
    for f in 0..BASE_FILES {
        let _ = writeln!(s, "INCLUDE {}", file_path(f));
    }
    for (slot, present) in scratch_present.iter().enumerate() {
        if *present {
            let _ = writeln!(s, "INCLUDE {}", scratch_path(slot));
        }
    }
    s.push_str("VAR main_counter = 0\n");
    s.push_str("The opening line of the editor-session project. # generated\n");
    s.push_str("~ main_counter = main_counter + 1\n");
    s.push_str("-> k00_00\n");
    s
}

/// One base file: two `VAR`s (value driven by `rev.var`) and
/// [`BASE_KNOTS`] knots cycling through four shapes (text-only,
/// var-mutation + divert, choices + gather, inline conditional), same mix
/// `compile_bench`'s synthetic project uses. Decl names never depend on any
/// revision counter, so a knot/VAR name is stable across a file's whole
/// history — only literals, sentence text, and trailing blank lines vary.
fn generate_file(idx: usize, rev: FileRevision) -> String {
    let mut s = format!(
        "// Generated file {idx:02} (text_rev={} var_rev={} blank={}).\n",
        rev.text, rev.var, rev.blank
    );

    let mut var_rng = Lcg::new(0xFEED_0000_0000_0000 ^ seed_mix(idx, rev.var));
    for v in 0..2 {
        let _ = writeln!(s, "VAR var_{idx:02}_{v} = {}", var_rng.pick(0, 99));
    }
    s.push('\n');

    let mut body_rng = Lcg::new(0xBEEF_0000_0000_0000 ^ seed_mix(idx, rev.text));
    for k in 0..BASE_KNOTS {
        let _ = writeln!(s, "=== k{idx:02}_{k:02} ===");
        let next = if k + 1 < BASE_KNOTS {
            format!("-> k{idx:02}_{:02}", k + 1)
        } else {
            "-> DONE".to_string()
        };
        match (idx * BASE_KNOTS + k) % 4 {
            0 => {
                for _ in 0..body_rng.pick(2, 4) {
                    s.push_str(&sentence(&mut body_rng, 4, 9));
                    s.push('\n');
                }
                s.push_str("-> DONE\n");
            }
            1 => {
                s.push_str(&sentence(&mut body_rng, 3, 6));
                s.push('\n');
                let _ = writeln!(s, "~ var_{idx:02}_0 = var_{idx:02}_0 + 1");
                let _ = writeln!(s, "The counter reads {{var_{idx:02}_0}} here.");
                s.push_str(&next);
                s.push('\n');
            }
            2 => {
                s.push_str(&sentence(&mut body_rng, 3, 6));
                s.push('\n');
                let _ = writeln!(s, "* [{}]", sentence(&mut body_rng, 2, 4));
                let _ = writeln!(s, "    {}", sentence(&mut body_rng, 3, 6));
                let _ = writeln!(s, "* [{}]", sentence(&mut body_rng, 2, 4));
                let _ = writeln!(s, "    {}", sentence(&mut body_rng, 3, 6));
                let _ = writeln!(s, "- {}", sentence(&mut body_rng, 2, 5));
                s.push_str(&next);
                s.push('\n');
            }
            _ => {
                let _ = writeln!(
                    s,
                    "{{var_{idx:02}_1 > 40: {}|{}}}",
                    sentence(&mut body_rng, 2, 4),
                    sentence(&mut body_rng, 2, 4)
                );
                s.push_str(&next);
                s.push('\n');
            }
        }
        s.push('\n');
    }

    for _ in 0..(rev.blank % 4) {
        s.push('\n');
    }

    s
}

/// A scratch file: unreachable from the main flow (no divert points into
/// it — it only exists via its `INCLUDE`), same as a real editor's
/// freshly-created, not-yet-wired-up scene file. `variant` is bumped every
/// time the slot goes from absent to present, so content is never identical
/// across two "creations" of the same slot.
fn generate_scratch_file(slot: usize, variant: u64) -> String {
    let mut s = format!("// Scratch file {slot} (variant={variant}).\n");
    let mut rng = Lcg::new(0xACE0_0000_0000_0000 ^ seed_mix(slot, variant));
    let _ = writeln!(s, "VAR scratch_{slot}_ctr = {}", rng.pick(0, 9));
    s.push('\n');
    for k in 0..SCRATCH_KNOTS {
        let _ = writeln!(s, "=== s{slot}_{k:02} ===");
        s.push_str(&sentence(&mut rng, 3, 7));
        s.push('\n');
        if k + 1 < SCRATCH_KNOTS {
            let _ = writeln!(s, "-> s{slot}_{:02}", k + 1);
        } else {
            s.push_str("-> DONE\n");
        }
        s.push('\n');
    }
    s
}

// ── Output ───────────────────────────────────────────────────────────

fn checkpoint(db: &ProjectDb, checkpoint_idx: usize, edits: usize) -> Vec<IngredientMemory> {
    let rows = db.memory_snapshot();
    for m in &rows {
        row(checkpoint_idx, edits, m);
    }
    rows
}

fn row(checkpoint_idx: usize, edits: usize, m: &IngredientMemory) {
    let kind = kind_str(m.kind);
    let heap = m
        .heap_bytes
        .map_or_else(|| "-".to_string(), |b| b.to_string());
    println!(
        "editor_session_bench | checkpoint={checkpoint_idx:03} edits={edits:>5} | {kind:<6} | {:<32} | count={:>6} metadata_bytes={:>10} fields_bytes={:>10} heap_bytes={heap:>10}",
        m.name, m.count, m.metadata_bytes, m.fields_bytes
    );
}

fn kind_str(kind: IngredientKind) -> &'static str {
    match kind {
        IngredientKind::Struct => "struct",
        IngredientKind::Query => "query",
    }
}

/// A signed `after - before` delta, rendered without ever casting a `usize`
/// count to a signed type (both sides of the subtraction stay in `usize`).
fn signed_delta(before: usize, after: usize) -> String {
    if after >= before {
        format!("+{}", after - before)
    } else {
        format!("-{}", before - after)
    }
}

/// Print the headline growth table: every ingredient that appeared in
/// either the first or last checkpoint, first vs. last `count` and
/// `metadata_bytes`. A query/struct that only appears in `last` (count in
/// `first` reported as 0) means the query was never invoked before that
/// checkpoint — not itself a growth signal, just first exercise.
fn print_growth(
    first: &[IngredientMemory],
    last: &[IngredientMemory],
    first_edits: usize,
    last_edits: usize,
) {
    println!("editor_session_bench | growth | edits {first_edits} -> {last_edits}");

    let mut names: BTreeSet<(IngredientKind, &str)> = BTreeSet::new();
    for m in first {
        names.insert((m.kind, m.name.as_str()));
    }
    for m in last {
        names.insert((m.kind, m.name.as_str()));
    }

    for (kind, name) in names {
        let f = first.iter().find(|m| m.kind == kind && m.name == name);
        let l = last.iter().find(|m| m.kind == kind && m.name == name);
        let fc = f.map_or(0, |m| m.count);
        let lc = l.map_or(0, |m| m.count);
        let fm = f.map_or(0, |m| m.metadata_bytes);
        let lm = l.map_or(0, |m| m.metadata_bytes);
        println!(
            "editor_session_bench | growth | {:<6} | {name:<32} | count {fc:>6} -> {lc:>6} (\u{394}{}) | metadata_bytes {fm:>10} -> {lm:>10} (\u{394}{})",
            kind_str(kind),
            signed_delta(fc, lc),
            signed_delta(fm, lm),
        );
    }
}
