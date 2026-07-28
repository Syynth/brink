//! End-to-end regression tests for #1504: two files with root-level weave
//! content used to miscompile into colliding container ids.
//!
//! This is the user-facing half of the analysis in
//! `docs/root-content-identity-findings.md`. The LIR-level tests
//! live in `brink-ir/tests/lir_lowering/root_content_definition_id_soundness.rs`.
//!
//! Reachable through the ordinary compiler entry point
//! (`brink_compiler::compile`) with a plain `INCLUDE` — no unusual flags,
//! no native dialect, no incremental session. Any ink project where the
//! entry file **and** an included file both carried root-level weave content
//! hit it.
//!
//! The fix qualifies a file's *anonymous* root-content scope path by that
//! file's own project path (`hir::root_content_scope_path`), so `c-0` in
//! `main.ink` and `c-0` in `inc.ink` no longer hash to one `DefinitionId`.
//! These tests were written as acceptance tests while the fix shape was
//! design-gated; they now run unconditionally. Do not rewrite them to
//! assert the pre-fix behavior.
//!
//! #1673's codegen-boundary guard — which refuses a `Program` containing
//! two containers with the same `DefinitionId` — stays as the backstop; see
//! `included_and_entry_root_weaves_compile_without_tripping_the_uniqueness_guard`
//! below, which asserts it has nothing left to fire on for this shape.

#![allow(clippy::unwrap_used, clippy::panic)]

use std::collections::{BTreeMap, HashMap};
use std::sync::{Mutex, MutexGuard, OnceLock};

use brink_runtime::{DotNetRng, Line, Story};

/// Every test in this file compiles through `prepare_driver`, which calls
/// `brink_driver::native_source_root` — and for a bare/`./`-relative entry
/// (every `compile_mem` call below passes one), that walks up from the
/// *real process cwd* looking for a `brink.toml`. Only
/// `root_content_ids_are_stable_when_brink_toml_lives_above_the_entry_dir`
/// below actually changes cwd (there is no other way to exercise a
/// relatively-spelled entry against a real, disk-resident `brink.toml`),
/// but every test here is implicitly cwd-sensitive, so all of them take this
/// same lock for their duration — otherwise a `chdir` landing mid-test could
/// make an unrelated test's two sequential `compile_mem` calls resolve
/// against two different roots and spuriously diverge.
fn cwd_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Compile from an in-memory file system, mirroring `driver.rs`'s helper.
fn compile_mem(
    entry: &str,
    files: &HashMap<&str, &str>,
) -> Result<brink_format::StoryData, brink_compiler::CompileError> {
    brink_compiler::compile(entry, |path| {
        files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
            std::io::Error::new(
                std::io::ErrorKind::NotFound,
                format!("file not found: {path}"),
            )
        })
    })
    .map(|output| output.data)
}

/// The compiled program must not contain two containers with one id.
///
/// Measured before the fix (`origin/main` at commit 999581354): 8
/// containers, of which three ids appeared twice (`0x1779765f903c98e`,
/// `0x1dde84850f175fb`, `0x1ef2ee91775101d`).
#[test]
fn included_and_entry_root_weaves_get_distinct_container_ids() {
    let _cwd_guard = cwd_lock();
    let files: HashMap<&str, &str> = HashMap::from([
        (
            "main.ink",
            "INCLUDE inc.ink\n* main one\n* main two\n- main gathered\n",
        ),
        ("inc.ink", "* inc one\n* inc two\n- inc gathered\n"),
    ]);

    let story = compile_mem("main.ink", &files).unwrap();

    let mut seen: BTreeMap<u64, usize> = BTreeMap::new();
    for container in &story.containers {
        *seen.entry(container.id.to_raw()).or_default() += 1;
    }
    let dupes: Vec<String> = seen
        .iter()
        .filter(|(_, count)| **count > 1)
        .map(|(id, count)| format!("0x{id:x} appears {count}x"))
        .collect();

    assert!(
        dupes.is_empty(),
        "duplicate container ids in the compiled program: {dupes:#?}",
    );
}

/// #1673's codegen-boundary uniqueness guard must have nothing to fire on
/// once #1504(a) is fixed.
///
/// While the collision existed, this shape tripped the guard with an `E060`
/// (`duplicate DefinitionId … at paths …`) — the guard's whole point was to
/// turn the silent miscompile into a loud compile error. The #1504 fix
/// removes the collision itself, so the same shape must now compile
/// cleanly; the guard stays as the backstop for any future id-derivation
/// defect. Rewritten to assert `Ok` exactly as this test's pre-fix doc
/// instructed.
#[test]
fn included_and_entry_root_weaves_compile_without_tripping_the_uniqueness_guard() {
    let _cwd_guard = cwd_lock();
    let files: HashMap<&str, &str> = HashMap::from([
        (
            "main.ink",
            "INCLUDE inc.ink\n* main one\n* main two\n- main gathered\n",
        ),
        ("inc.ink", "* inc one\n* inc two\n- inc gathered\n"),
    ]);

    let story = compile_mem("main.ink", &files)
        .expect("root weaves in two files must compile — #1504 removed the id collision");

    // Sanity: the guard's precondition holds, not just its absence of error.
    let mut seen: BTreeMap<u64, usize> = BTreeMap::new();
    for container in &story.containers {
        *seen.entry(container.id.to_raw()).or_default() += 1;
    }
    assert!(
        seen.values().all(|count| *count == 1),
        "every container must carry a unique DefinitionId: {seen:#?}",
    );
}

/// The collision was observable as wrong output: the linker's address map is
/// last-write-wins (`brink-runtime/src/linker.rs`), so the entry file's
/// root-weave containers overwrote the included file's, and picking the
/// included file's first choice ran the **entry** file's first choice body.
///
/// Measured before the fix (`origin/main` at commit 999581354), choosing
/// index 0 from the `inc one` / `inc two` set yielded `main one` +
/// `MAIN-ONE-BODY`; `INC-ONE-BODY` never executed.
#[test]
fn choosing_an_included_files_choice_runs_that_files_body() {
    let _cwd_guard = cwd_lock();
    let files: HashMap<&str, &str> = HashMap::from([
        (
            "main.ink",
            "INCLUDE inc.ink\n* main one\n  MAIN-ONE-BODY\n* main two\n  MAIN-TWO-BODY\n- main gathered\n",
        ),
        (
            "inc.ink",
            "* inc one\n  INC-ONE-BODY\n* inc two\n  INC-TWO-BODY\n- inc gathered\n",
        ),
    ]);

    let data = compile_mem("main.ink", &files).unwrap();
    let (program, line_tables) = brink_runtime::link(&data).unwrap();
    let mut story = Story::<DotNetRng>::new(std::sync::Arc::new(program), line_tables);

    // The first choice set is the included file's.
    let Line::Choices { choices, .. } = story.continue_single().unwrap() else {
        panic!("expected the included file's root choice set first");
    };
    assert_eq!(choices[0].text, "inc one");
    story.choose(0).unwrap();

    // Whatever ran must be the body of the choice the player was offered.
    let mut output = String::new();
    for _ in 0..4 {
        let line = story.continue_single().unwrap();
        output.push_str(line.text());
        if matches!(line, Line::Done { .. } | Line::End { .. }) {
            break;
        }
    }

    assert!(
        output.contains("INC-ONE-BODY"),
        "picking `inc one` ran the wrong choice body; got: {output:?}",
    );
    assert!(
        !output.contains("MAIN-ONE-BODY"),
        "picking `inc one` ran the entry file's body instead; got: {output:?}",
    );
}

/// The intl surface does **not** move with #1504's re-keying.
///
/// `brink-intl`'s export keys each translation scope on a
/// `ScopeLineTable::scope_id` (`export.rs`), and codegen opens a line table
/// only for a *scope-kind* container — `Root`, `Knot`, `Stitch`. Every
/// root-level choice and gather inherits the enclosing **root** scope's id,
/// which is the hash of the empty path and is not qualified by file. So
/// qualifying anonymous root-content container ids leaves every XLIFF unit
/// id for root-level lines exactly where it was.
///
/// This pins that two ways. First, discriminatingly: byte-identical content
/// compiled under two *different* entry filenames must export the same root
/// scope id. Every line table in either fixture is the root's, whose id is
/// `context::root_definition_id()` — a fixed hash of the empty path that no
/// file qualifier can move — so this assertion fails the moment a root scope
/// id ever becomes file-qualified, which a same-content/same-name comparison
/// (the second assertion below) cannot detect: with only one entry name in
/// play, that comparison would keep passing even in the world it claims to
/// exclude, since both fixtures would still qualify by the same name. Second,
/// as the original regression: the same entry file exports the same
/// root-content scope id whether or not it `INCLUDE`s a second file that also
/// carries root weave — the case in which #1504's qualifier is doing work.
#[test]
fn root_content_translation_scope_id_is_unaffected_by_the_qualifier() {
    const ENTRY: &str = "* main one\n* main two\n- main gathered\n";
    let _cwd_guard = cwd_lock();

    let scope_ids = |data: &brink_format::StoryData| -> Vec<u64> {
        data.line_tables
            .iter()
            .map(|table| table.scope_id.to_raw())
            .collect()
    };

    // Discriminating: same content, two different entry filenames, no
    // INCLUDE graph in play at all.
    let under_main: HashMap<&str, &str> = HashMap::from([("main.ink", ENTRY)]);
    let under_other: HashMap<&str, &str> = HashMap::from([("other.ink", ENTRY)]);
    let main_data = compile_mem("main.ink", &under_main).unwrap();
    let other_data = compile_mem("other.ink", &under_other).unwrap();
    assert_eq!(
        scope_ids(&main_data),
        scope_ids(&other_data),
        "root-content translation scope ids must not depend on the entry file's name",
    );

    // Original regression: the shape #1504's qualifier actually touches — an
    // entry that INCLUDEs a second file which also carries root weave.
    let entry_with_include = format!("INCLUDE inc.ink\n{ENTRY}");
    let solo: HashMap<&str, &str> = HashMap::from([("main.ink", ENTRY)]);
    let with_include: HashMap<&str, &str> = HashMap::from([
        ("main.ink", entry_with_include.as_str()),
        ("inc.ink", "* inc one\n* inc two\n- inc gathered\n"),
    ]);
    let solo_data = compile_mem("main.ink", &solo).unwrap();
    let include_data = compile_mem("main.ink", &with_include).unwrap();
    assert_eq!(
        scope_ids(&solo_data),
        scope_ids(&include_data),
        "root-content translation scope ids must not depend on the #1504 file qualifier",
    );
}

/// #1696 (follow-up to #1693's review): the qualifier
/// [`hir::root_content_scope_path`](brink_ir::hir::root_content_scope_path)
/// uses is now a **root-relative key**
/// (`brink_db::modules::root_relative_key` against `ProjectDb::ink_root`,
/// registered by `prepare_driver` via `brink_driver::native_source_root` —
/// the same `brink.toml`-walk-up rule native compiles already used, extended
/// to ink), not the file's raw registered path. Three spellings of what is,
/// on disk, the *same* file therefore now mint the SAME anonymous
/// root-content `DefinitionId`s: `brink compile main.ink`, `./main.ink`, and
/// an absolute spelling all agree — closing the CLI-vs-`brink-lsp` (absolute
/// OS path, `backend.rs`'s `uri_to_path`) disagreement this test used to pin
/// as a known limitation.
///
/// ⚠ This is the identity break the fix accepts, not a regression: every
/// project's anonymous root-content ids move once more (on top of #1504's
/// one-time move) for any entry whose registered spelling was not already
/// bare-relative. See `.changeset/issue-1696-ink-root-content-key-
/// normalization.md` for the save/translation-impact writeup.
#[test]
fn root_content_ids_are_stable_across_entry_path_spellings() {
    let _cwd_guard = cwd_lock();
    let content = "* one\n* two\n- gathered\n";
    let bare: HashMap<&str, &str> = HashMap::from([("main.ink", content)]);
    let dotslash: HashMap<&str, &str> = HashMap::from([("./main.ink", content)]);
    let absolute: HashMap<&str, &str> =
        HashMap::from([("/nonexistent-brink-test-root/proj/main.ink", content)]);

    let bare_data = compile_mem("main.ink", &bare).unwrap();
    let dotslash_data = compile_mem("./main.ink", &dotslash).unwrap();
    let absolute_data =
        compile_mem("/nonexistent-brink-test-root/proj/main.ink", &absolute).unwrap();

    let ids = |data: &brink_format::StoryData| -> Vec<u64> {
        data.containers.iter().map(|c| c.id.to_raw()).collect()
    };

    assert_eq!(
        ids(&bare_data),
        ids(&dotslash_data),
        "byte-identical content compiled under `main.ink` vs `./main.ink` \
         minted DIFFERENT container ids — the #1696 root-relative-key \
         normalization regressed",
    );
    assert_eq!(
        ids(&bare_data),
        ids(&absolute_data),
        "byte-identical content compiled under `main.ink` vs an absolute \
         spelling of the same file minted DIFFERENT container ids — the \
         #1696 root-relative-key normalization regressed",
    );
}

/// A unique, empty directory under the system temp dir — real disk, not the
/// in-memory `compile_mem` fixture, because this test's whole point is
/// `native_source_root`'s real `brink.toml`-walk-up behavior, which only
/// runs against a real filesystem.
fn unique_temp_dir(tag: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-test-{tag}-{}-{:?}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    std::fs::create_dir_all(&dir)
        .unwrap_or_else(|e| panic!("create temp dir {}: {e}", dir.display()));
    dir
}

/// Restores the process cwd on drop (including on panic/unwind) — used by
/// `root_content_ids_are_stable_when_brink_toml_lives_above_the_entry_dir`,
/// the one test in this file that actually `chdir`s.
struct RestoreCwd(std::path::PathBuf);
impl Drop for RestoreCwd {
    fn drop(&mut self) {
        let _ = std::env::set_current_dir(&self.0);
    }
}

/// Review finding on #1706: `root_content_ids_are_stable_across_entry_path_
/// spellings` above only exercises `root = "."` (no `brink.toml` found at
/// all, since `compile_mem` has no real filesystem) — the one case where a
/// bare `Path::strip_prefix` already happens to work for both `main.ink` and
/// `./main.ink`. It never reaches the case that actually motivated this fix:
/// a `brink.toml` living *above* the entry's own directory, which sends
/// `native_source_root` down its #1413 absolutized-retry path and hands back
/// an ABSOLUTE root for a relatively-spelled entry.
///
/// Real disk, real `brink.toml`, real `chdir` into the entry's own directory
/// — the only way a bare `main.ink` / `./main.ink` spelling is meaningful at
/// all (both are resolved against the process cwd by the OS, not by this
/// test). Guarded by `cwd_lock` for the whole body; `chdir` is restored
/// (even on panic, via the drop guard) before the lock is released.
#[test]
fn root_content_ids_are_stable_when_brink_toml_lives_above_the_entry_dir() {
    let _cwd_guard = cwd_lock();
    let restore_guard = RestoreCwd(std::env::current_dir().expect("process must have a cwd"));

    let root_dir = unique_temp_dir("brink-toml-above-entry-dir");
    // `brink.toml` lives ONE directory above the entry — never in the
    // entry's own directory.
    std::fs::write(root_dir.join("brink.toml"), "[project]\n").expect("write brink.toml");
    let entry_dir = root_dir.join("sub");
    std::fs::create_dir_all(&entry_dir).expect("mkdir sub");
    std::fs::write(entry_dir.join("main.ink"), "* one\n* two\n- gathered\n")
        .expect("write main.ink");

    std::env::set_current_dir(&entry_dir).expect("chdir into the entry's own directory");

    let bare = brink_compiler::compile_path(std::path::Path::new("main.ink"))
        .expect("bare relative spelling must compile");
    let dotslash = brink_compiler::compile_path(std::path::Path::new("./main.ink"))
        .expect("`./`-relative spelling must compile");
    let absolute = brink_compiler::compile_path(&entry_dir.join("main.ink"))
        .expect("absolute spelling must compile");

    let ids = |data: &brink_format::StoryData| -> Vec<u64> {
        data.containers.iter().map(|c| c.id.to_raw()).collect()
    };

    assert_eq!(
        ids(&bare.data),
        ids(&dotslash.data),
        "`main.ink` vs `./main.ink`, compiled against a brink.toml living \
         ABOVE the entry's directory (so native_source_root's absolutized \
         retry hands back an absolute root for a relatively-spelled entry), \
         minted DIFFERENT container ids — root_relative_key must absolutize \
         both sides before stripping",
    );
    assert_eq!(
        ids(&bare.data),
        ids(&absolute.data),
        "`main.ink` vs an absolute spelling, compiled against a brink.toml \
         living ABOVE the entry's directory, minted DIFFERENT container \
         ids — root_relative_key must absolutize both sides before \
         stripping",
    );

    drop(restore_guard);
    std::fs::remove_dir_all(&root_dir).expect("cleanup temp dir");
}
