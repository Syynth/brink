// Per-worktree build stamp (#2759). Writes this crate's own
// `CARGO_MANIFEST_DIR` — an absolute path that is worktree-specific by
// construction — into a small file under `OUT_DIR`. The shared
// `CARGO_TARGET_DIR` freshness check (`scripts/check-target-freshness.mjs`)
// reads it back and compares it against `git worktree list` to answer "was
// this cached artifact last built by a *different, still-live* worktree?"
// instead of the static "a collision is structurally possible" heuristic it
// replaces. See that script's module doc for the full design, including how
// a missing/unreadable/stale-worktree stamp is handled (never as a reason
// to go red on a legitimately fresh tree).
//
// Deliberately dependency-free (std only) and best-effort: nothing here may
// ever fail the actual build, so every fallible step is swallowed rather
// than propagated with `?`/`unwrap`/`expect` (all denied by workspace
// lints, and rightly so for a build script's side channel).
//
// No `cargo:rerun-if-changed` is emitted, on purpose — but NOT because
// cargo reruns build scripts on every invocation by default; it does not.
// Cargo's documented default (absent any `rerun-if-*` directive) is to
// rerun a build script only when a file inside the package has changed
// since the last run that actually executed it. What this design relies on
// is narrower and still holds under that real default: the stamp always
// names the last worktree for which cargo actually re-ran this build
// script — and a no-op repeat build in the *same* worktree (nothing
// changed, so the script does not rerun) simply leaves the stamp naming
// that same worktree, which is still the correct answer. The stamp
// reflects the most recent worktree that caused a rerun, not every
// worktree that has ever built this package — that is enough for the
// freshness check's purposes.
//
// This crate is published to crates.io (see docs/releasing.md's "Publish
// surface: build.rs worktree stamp" section) — the `CARGO_TARGET_DIR` guard
// below keeps this a no-op off the shared-cache convention this tool exists
// for, so a downstream consumer of the published crate never gets a stamp
// file written under their own target dir for no reason.
fn main() {
    // Only the pump's shared-cache convention sets `CARGO_TARGET_DIR`
    // explicitly (see `.claude/skills/autonomous-pump/BRINK-CONFIG.md`'s
    // Disk rule). A downstream consumer of this published crate almost
    // never does, so bail out before touching `OUT_DIR` at all in that
    // case.
    if std::env::var_os("CARGO_TARGET_DIR").is_none() {
        return;
    }
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let stamp_path = std::path::Path::new(&out_dir).join("worktree-stamp.txt");
    let _ = std::fs::write(stamp_path, manifest_dir);
}
