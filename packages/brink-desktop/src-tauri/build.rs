//! Build script for the Tauri shell crate.
//!
//! `tauri_build::build()` resolves `bundle.externalBin` from
//! `tauri.conf.json` UNCONDITIONALLY — not only when a bundle is actually
//! produced — so `binaries/brink-cli-<target-triple>` has to exist on disk
//! before this crate will even `cargo check`. That path is gitignored
//! (`.gitignore`, `/binaries`) because the triple suffix is host-specific,
//! which meant CLAUDE.md's documented desktop gate
//!
//! ```text
//! cd packages/brink-desktop/src-tauri && cargo test
//! ```
//!
//! failed on every fresh checkout and every fresh git worktree with
//!
//! ```text
//! resource path `binaries/brink-cli-x86_64-unknown-linux-gnu` doesn't exist
//! ```
//!
//! before a single test ran (#2617). The only local workaround was to
//! hand-stub the file — which is exactly the audience
//! `every_pnpm_install_lane_builds_wasm_first_in_the_same_job` (the #2504
//! workflow-ordering guard that lives in this crate's `src/lib.rs`) exists
//! for.
//!
//! CI does not hit this because `.github/workflows/desktop-smoke.yml` runs a
//! "Stage brink-cli sidecar" step — `node scripts/ensure-cli-sidecar.mjs`
//! under `BRINK_SIDECAR_STUB: "1"` — before its `cargo check`/`clippy`/`test`
//! steps. [`stage_dev_sidecar_if_missing`] below runs **that same script with
//! that same variable**, rather than inventing a second staging mechanism: the
//! stub payload, the triple detection and the staged filename all stay owned
//! by `packages/brink-desktop/scripts/ensure-cli-sidecar.mjs`.
//!
//! A stub is the honest substitute here, not a fake pass. Nothing built by
//! `cargo test`/`cargo check` ever EXECUTES the sidecar — `run_cli` in
//! `src/lib.rs` is its only caller and it needs a running `AppHandle` — so
//! the file's *existence* is the whole requirement, which is precisely why
//! #2469 switched the smoke lane itself to a stub. `STUB_SIDECAR` is a
//! loudly-failing POSIX shell script (exit 127 with an explanatory message),
//! so if something ever does start executing it, it says so.
//!
//! ⚠ Debug profile ONLY. `cargo tauri build` (release) still fails loudly on
//! a missing sidecar exactly as before — a real bundle must ship the real
//! `brink-cli`, and silently substituting a stub there would be the failure
//! mode this whole file is careful not to create. The release path stages
//! the genuine binary through `beforeBuildCommand` -> `pnpm build` ->
//! `ensure-cli-sidecar.mjs` with no stub variable set.

use std::path::PathBuf;
use std::process::{Command, Stdio};

fn main() {
    stage_dev_sidecar_if_missing();
    tauri_build::build();
}

/// Emit a `cargo:warning=` line.
///
/// A build script's stdout IS cargo's directive channel, and `cargo:warning=`
/// has no other transport — which is why clippy exempts build scripts from
/// `print_stdout` on its own. No `#[expect]`/`#[allow]` here: adding one is a
/// hard error under this crate's `-D warnings`, because the lint never fires
/// (`unfulfilled_lint_expectations`).
fn warn(message: &str) {
    println!("cargo:warning={message}");
}

/// Stage a stub `brink-cli` sidecar for debug builds when none is present,
/// so the documented `cargo test` gate runs on a fresh tree.
///
/// Silent no-op in the common case: as soon as the file exists — staged by
/// this function, by `pnpm dev`/`pnpm build`'s preflight, or by the smoke
/// lane's own step — nothing runs and nothing is printed.
fn stage_dev_sidecar_if_missing() {
    // Release builds keep the old, loud failure: see the module note.
    if std::env::var("PROFILE").as_deref() != Ok("debug") {
        return;
    }

    // `TARGET` is the triple `tauri-build` resolves `externalBin` against
    // (it is what lands in `TAURI_ENV_TARGET_TRIPLE`), so probe the exact
    // path Tauri is about to look for rather than re-deriving one.
    let Ok(target) = std::env::var("TARGET") else {
        return;
    };
    let Ok(manifest_dir) = std::env::var("CARGO_MANIFEST_DIR").map(PathBuf::from) else {
        return;
    };

    // The `.exe` suffix is a hard Tauri `externalBin` requirement on Windows
    // triples, mirrored from `sidecarPaths` in ensure-cli-sidecar.mjs.
    let exe_suffix = if target.contains("windows") {
        ".exe"
    } else {
        ""
    };
    let staged = manifest_dir
        .join("binaries")
        .join(format!("brink-cli-{target}{exe_suffix}"));
    if staged.exists() {
        return;
    }

    // `ensure-cli-sidecar.mjs` stages under `hostTriple()` (from `rustc
    // -vV`), not the `TARGET` this function is probing — under a
    // cross-target `cargo test/check --target <other>` those two triples
    // differ, so the script would succeed while writing a wrong-triple file
    // that this function can never see as staged. Cargo sets `HOST` for
    // build scripts specifically so it can be compared against `TARGET`;
    // bail with the same warning below rather than invoke a script whose
    // host-triple assumption does not match.
    let host_matches_target = std::env::var("HOST").as_deref() == Ok(target.as_str());

    // `ensure-cli-sidecar.mjs` refuses to stage the POSIX stub under a
    // `.exe`-suffixed name (#2481) — Windows would load it through the PE
    // loader and never start it. Don't ask for something it will reject.
    if exe_suffix.is_empty() && host_matches_target {
        // The desktop package dir: `scripts/` sits beside `src-tauri/`.
        let package_dir = manifest_dir.join("..");
        let script = package_dir.join("scripts").join("ensure-cli-sidecar.mjs");
        if script.is_file() {
            let ran = Command::new("node")
                .arg("scripts/ensure-cli-sidecar.mjs")
                .current_dir(&package_dir)
                .env("BRINK_SIDECAR_STUB", "1")
                // The child's `console.log` output would otherwise land in
                // cargo's build-script directive channel (stdout) — harmless
                // today since none of its lines start with `cargo:`, but
                // cargo swallows that stream either way, so nothing is lost
                // by keeping it out and every warning stays on the
                // `cargo:warning=` channel below where a developer can see it.
                .stdout(Stdio::null())
                .status();
            if matches!(ran, Ok(status) if status.success()) && staged.exists() {
                warn(&format!(
                    "staged a STUB brink-cli sidecar at {} so this debug build can resolve \
                     bundle.externalBin (#2617). Nothing in `cargo test`/`cargo check` executes \
                     it; run `pnpm --filter @brink/desktop build` for the real binary.",
                    staged.display()
                ));
                return;
            }
        }
    }

    warn(&format!(
        "no brink-cli sidecar at {} and this build script could not stage a stub — \
         `tauri_build::build()` is about to fail on it. Stage one by hand with: \
         (cd packages/brink-desktop && BRINK_SIDECAR_STUB=1 node scripts/ensure-cli-sidecar.mjs)",
        staged.display()
    ));
}
