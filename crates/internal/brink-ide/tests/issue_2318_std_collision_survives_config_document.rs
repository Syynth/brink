//! Regression test for issue #2318: a native project's own declarations
//! colliding, in the editor only, with the mounted stdlib's same-named
//! declarations.
//!
//! `std/conventions/screenplay.brink` declares `struct Cue`, `fn cue`, and
//! `fn heading`. Under #2245's peer-root ruling `std::` is a peer of
//! `story::`, not a parent, so a project that declares its OWN `Cue`/`cue`/
//! `heading` (an ordinary case — an author writing their own screenplay
//! conventions instead of using the built-in preset) must not collide with
//! it. `brink compile` on such a project exits `0`; only the editor
//! disagreed.
//!
//! The root cause (traced live, not guessed): `IdeSession`'s off-db
//! analysis gates M-2d cross-declared-module coexistence on
//! `ProjectDb::is_all_native` — every *native* file's module is always
//! "declared" (path-derived), so two same-name declarations in different
//! native modules coexist rather than colliding, exactly like `std::` vs
//! `story::`. But `is_all_native` used to answer `false` the moment the db
//! held even one non-`.brink` file, including a `brink.toml` — and
//! `IdeSession`'s real callers (`brink-web`'s `EditorSession`, and
//! `packages/ink-editor/src/project-session.ts`'s `listFiles`/`updateFile`
//! loop behind it) load `brink.toml` into the very same session as an
//! ordinary document, so the Binder can list/edit it. A native project with
//! its own config file open lost the M-2d exemption entirely: the mounted
//! std declaration and the project's own collided as an ordinary duplicate,
//! and the project's later-inserted declaration was dropped — which is what
//! produced the issue's own reported self-contradiction (`Cue` reported as
//! both a *duplicate* definition and *undeclared except under `std::`* in
//! one run: the merge saw two, inserted one, and then a struct-construction
//! reference to the survivor's std-only copy required an explicit
//! `use std::…`).
//!
//! This test mounts the real stdlib source (not a hand-copied stand-in,
//! which would drift the moment the preset changes), loads `brink.toml` as
//! an ordinary document exactly as the editor's file-loading contract does,
//! and asserts the collision-shaped diagnostics never fire.

use brink_ide::session::IdeSession;

/// The project's own conventions module — the SAME three names
/// (`Cue`/`cue`/`heading`) `std/conventions/screenplay.brink` declares,
/// matching the issue's own repro.
const CONVENTIONS: &str = "\
struct Cue {
  speaker: string,
}

@[convention(claims = \"^(?<name>[A-Z][A-Z '-]*)$\", attach = Cue, order = 10)]
fn cue(name: string): Cue {
  return Cue { speaker: name };
}

@[convention(claims = \"^(?<kind>INT|EXT)\\\\. (?<title>.+)$\", order = 20)]
fn heading(kind: string, title: string) {
  return \"-- {kind}. {title} --\";
}
";

const STORY: &str = "\
pub flow main() {
  INT. MARKET SQUARE - NIGHT
  The square is empty.

  VENDOR
  You shouldn't be here after dark.
}
";

const BRINK_TOML: &str = "\
[project]
entry = \"story.brink\"
conventions = \"conventions.brink\"
";

/// Builds the session the way the editor's own file-loading contract does:
/// the mounted stdlib first (mirroring `EditorSession::new`), then every
/// project file — `brink.toml` included, exactly as
/// `packages/ink-editor/src/project-session.ts`'s `listFiles`/`updateFile`
/// loop feeds every discovered path into the session with no source-vs-
/// config distinction.
fn session_with_config_document() -> IdeSession {
    let mut session = IdeSession::new();
    for (key, text) in brink_environment::stdlib_sources() {
        session.update_source(key, (*text).to_owned());
    }
    session.update_source("brink.toml", BRINK_TOML.to_owned());
    session.update_source("conventions.brink", CONVENTIONS.to_owned());
    session.update_and_analyze("story.brink", STORY.to_owned());
    session
}

/// The root-cause assertion: a `brink.toml` document sharing the session
/// must not disqualify an otherwise all-native project from
/// `ProjectDb::is_all_native` — the M-2d cross-declared-module coexistence
/// gate `IdeSession`'s off-db analysis reads.
#[test]
fn brink_toml_document_does_not_disqualify_is_all_native() {
    let session = session_with_config_document();
    assert!(
        session.db().is_all_native(),
        "a `brink.toml` document loaded alongside real `.brink` source files must not \
         flip `is_all_native` to false — it is not a recognized source file at all"
    );
}

/// The user-visible assertion: no duplicate/undeclared diagnostics fire for
/// the project's own `Cue`/`cue`/`heading` despite the stdlib mount
/// declaring the same three names.
#[test]
fn project_names_matching_the_mounted_stdlib_preset_do_not_collide() {
    let session = session_with_config_document();
    let analysis = session.analysis().expect("analysis");

    let offending: Vec<&brink_ir::Diagnostic> = analysis
        .diagnostics
        .iter()
        .filter(|d| {
            matches!(
                d.code,
                brink_ir::DiagnosticCode::E022 | brink_ir::DiagnosticCode::E023
            ) || d.message.contains("std::")
        })
        .collect();

    assert!(
        offending.is_empty(),
        "the project's own `Cue`/`cue`/`heading` must coexist with the mounted stdlib's \
         same-named declarations, not collide with them: {offending:#?}"
    );
}
