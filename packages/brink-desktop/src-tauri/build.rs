//! Build script for the Tauri shell crate.
//!
//! Nothing but `tauri_build::build()` — which is the point.
//!
//! This file used to carry ~150 lines of sidecar staging. `tauri_build`
//! resolves `bundle.externalBin` from `tauri.conf.json` UNCONDITIONALLY,
//! not only when a bundle is produced, so `binaries/brink-cli-<triple>` had
//! to exist on disk before this crate would even `cargo check` — and that
//! path is gitignored, because the triple suffix is host-specific. The
//! documented gate
//!
//! ```text
//! cd packages/brink-desktop/src-tauri && cargo test
//! ```
//!
//! therefore failed on every fresh checkout and every fresh git worktree
//! (#2617), which is why this script grew a stub-staging step.
//!
//! `bundle.externalBin` is gone (`docs/desktop-ota-spec.md` Stage 1: the
//! `brink-cli` sidecar is deleted, and the intl operations it existed for
//! run in the wasm), so the requirement it created is gone with it and the
//! staging has nothing left to work around.

fn main() {
    tauri_build::build();
}
