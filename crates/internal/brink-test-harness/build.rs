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
// No `cargo:rerun-if-changed` is emitted, on purpose: with none given,
// cargo's documented default is to rerun a build script on *every*
// invocation that touches this package, even when the compiled artifact
// itself is already up to date and the recompile is skipped. That is
// exactly the freshness signal wanted here — the stamp always reflects
// whichever worktree most recently asked cargo to build/check/test this
// package, not only the worktree that last forced a real recompile.
fn main() {
    let Ok(out_dir) = std::env::var("OUT_DIR") else {
        return;
    };
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR") else {
        return;
    };
    let stamp_path = std::path::Path::new(&out_dir).join("worktree-stamp.txt");
    let _ = std::fs::write(stamp_path, manifest_dir);
}
