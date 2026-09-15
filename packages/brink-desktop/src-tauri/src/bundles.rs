//! The OTA web-bundle store (`docs/desktop-ota-spec.md` Stage 2).
//!
//! The desktop's web bundle is compiled into the binary
//! (`tauri.conf.json`'s `frontendDist`), and on macOS the code signature
//! covers everything inside the `.app` — writing updated assets into
//! `Contents/Resources` invalidates it and Gatekeeper refuses to launch. So
//! an over-the-air bundle **cannot** live in the app. It lives under
//! `app_data_dir()` instead:
//!
//! ```text
//! <app_data_dir>/bundles/
//!   current.json          the pointer: active version, previous, sentinel
//!   <version>/            extracted, read-only once active
//!   staging/              download + extract target, renamed into place
//! ```
//!
//! The embedded copy stays exactly as it is, as a **known-good floor**: a
//! bad bundle is recovered by deleting a directory, never by reinstalling.
//! Every function here treats "no bundle" as an ordinary, correct state
//! rather than an error, which is what makes that floor real — a corrupt
//! `current.json`, an unreadable directory or a half-written install all
//! resolve to the embedded assets rather than to a broken app.
//!
//! Everything in this module is a pure function of a root path, so the whole
//! state machine is exercised by `cargo test` over a temp directory with no
//! Tauri runtime, no window, and no network.

use std::path::{Component, Path, PathBuf};

/// Directory under `app_data_dir()` holding every OTA bundle.
pub const BUNDLES_DIR: &str = "bundles";
/// Download/extract target, renamed into place once verified.
pub const STAGING_DIR: &str = "staging";
/// The pointer file naming the active bundle.
pub const STATE_FILE: &str = "current.json";
/// Served for a request with no path of its own.
pub const INDEX_FILE: &str = "index.html";

/// How many bundle directories the store keeps: the active one plus
/// [`RETENTION`] - 1 of history.
///
/// Three, because bundles are tens of megabytes unpacked and history is not
/// free. Two would cover rollback alone; three covers rollback plus a
/// usable recent history to pick from.
pub const RETENTION: usize = 3;

/// The bundle pointer, as it lives in `current.json`.
///
/// `attempting` is the rollback sentinel: it is stamped with the version
/// about to be served **before the webview loads**, and the frontend clears
/// it through `bundle_ready` once the shell is up. A sentinel that survives
/// a launch therefore means that bundle did not boot — nothing else can
/// produce that state, because a booting bundle always clears it.
#[derive(Debug, Default, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct BundleState {
    /// Active bundle version; `None` means the embedded floor is active.
    pub version: Option<String>,
    /// Previously-active bundles, **most recent first**.
    ///
    /// A list rather than one slot (Stage 4): the author can return to a
    /// version they liked, not merely undo the last step. Capped at
    /// [`RETENTION`] minus the active one — see [`promote`] for the
    /// exemption that keeps a pinned version out of the cap's reach.
    pub history: Vec<String>,
    /// Install time of `version`, epoch milliseconds.
    ///
    /// Epoch millis rather than RFC 3339 deliberately: a timestamp format
    /// is not worth a date dependency in a crate whose only reader is this
    /// module and a diagnostics command.
    pub installed_at_ms: Option<u64>,
    /// Rollback sentinel — see the type docs.
    pub attempting: Option<String>,
}

/// What the shell should serve for this launch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LaunchOutcome {
    /// No bundle installed (or none usable): serve the embedded floor.
    Embedded,
    /// Serve this bundle, falling back to embedded per missing file.
    Bundle(String),
    /// `failed` did not boot last launch and has been removed.
    ///
    /// `now_serving` is the bundle reverted to, or `None` for the embedded
    /// floor. The shell reports this to the author — a silent rollback
    /// would leave them on an older bundle with no idea why.
    RolledBack {
        failed: String,
        now_serving: Option<String>,
    },
}

/// `<app_data>/bundles`.
pub fn root_of(app_data: &Path) -> PathBuf {
    app_data.join(BUNDLES_DIR)
}

/// `<root>/current.json`.
pub fn state_path(root: &Path) -> PathBuf {
    root.join(STATE_FILE)
}

/// `<root>/staging`.
pub fn staging_dir(root: &Path) -> PathBuf {
    root.join(STAGING_DIR)
}

/// `<root>/<version>` — `None` when `version` is not a single safe path
/// component, so a crafted `current.json` cannot point the store outside
/// its own root.
pub fn version_dir(root: &Path, version: &str) -> Option<PathBuf> {
    is_safe_component(version).then(|| root.join(version))
}

/// Whether `s` is usable as exactly one directory name.
///
/// Rejects the traversal forms (`.`, `..`, anything with a separator) and
/// the reserved names, so a version string read out of an untrusted
/// `current.json` or manifest can never widen the store's reach. Deliberately
/// an allowlist of the characters a semver-ish version actually needs.
fn is_safe_component(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s != STAGING_DIR
        && s.len() <= 64
        && s.chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | '_'))
}

/// Read the pointer, treating every failure as "no bundle".
///
/// A missing file is the fresh-install case. A corrupt or truncated one is
/// a half-written install or a disk problem — both resolve to the embedded
/// floor, which is the whole point of keeping that floor.
pub fn read_state(root: &Path) -> BundleState {
    let Ok(text) = std::fs::read_to_string(state_path(root)) else {
        return BundleState::default();
    };
    serde_json::from_str(&text).unwrap_or_default()
}

/// Write the pointer atomically — write a sibling temp file, then rename.
///
/// A torn `current.json` is exactly the corrupt-state case above, so it
/// would be *recoverable*; the atomic write is still worth it because a
/// torn pointer silently discards a good installed bundle.
pub fn write_state(root: &Path, state: &BundleState) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let json = serde_json::to_string_pretty(state)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let tmp = root.join(".current.json.tmp");
    std::fs::write(&tmp, json)?;
    std::fs::rename(&tmp, state_path(root))
}

/// Resolve one request path against a bundle directory.
///
/// ⚠ This is the security boundary of the serving path: the request path
/// comes from the webview. The rules, in order —
///
/// 1. percent-decode, so an encoded traversal (`%2e%2e`) is judged as what
///    it decodes to rather than as an opaque component;
/// 2. accept only [`Component::Normal`] components, which rejects `..`,
///    `.`, a root prefix and (on Windows) a drive prefix;
/// 3. reject any decoded component that still carries a separator or a NUL,
///    which is what `%2f` and `%00` decode to;
/// 4. join, canonicalize, and require the result to still sit inside the
///    canonicalized bundle directory — this is what defeats a **symlink**
///    planted in the archive, which none of the string-level rules can see;
/// 5. require a regular file.
///
/// Returns `None` for anything that fails, and `None` is not an error: the
/// caller falls back to the embedded asset.
pub fn resolve_asset(bundle_dir: &Path, request_path: &str) -> Option<PathBuf> {
    let decoded = percent_decode(request_path)?;
    let trimmed = decoded.trim_start_matches('/');
    let relative = if trimmed.is_empty() {
        INDEX_FILE
    } else {
        trimmed
    };

    let mut candidate = bundle_dir.to_path_buf();
    let mut pushed = 0usize;
    for component in Path::new(relative).components() {
        let Component::Normal(part) = component else {
            return None;
        };
        let part = part.to_str()?;
        if part.contains('/') || part.contains('\\') || part.contains('\0') {
            return None;
        }
        candidate.push(part);
        pushed += 1;
    }
    if pushed == 0 {
        return None;
    }

    // Canonicalize BOTH sides: comparing a canonical candidate against a
    // non-canonical root would reject every request on a machine whose
    // app-data path runs through a symlink (macOS `/var` -> `/private/var`
    // is the standard case), and comparing two non-canonical paths would
    // miss a symlink inside the bundle entirely.
    let root = bundle_dir.canonicalize().ok()?;
    let resolved = candidate.canonicalize().ok()?;
    if !resolved.starts_with(&root) {
        return None;
    }
    resolved.is_file().then_some(resolved)
}

/// Decode `%XX` escapes, rejecting a malformed escape outright.
///
/// Hand-rolled rather than pulling a dependency for it: the grammar is four
/// lines, and rejecting a malformed escape (rather than passing it through,
/// as a lenient decoder does) is the behaviour this boundary wants.
fn percent_decode(raw: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = raw.get(i + 1..i + 3)?;
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

/// Decide what to serve, and stamp the rollback sentinel for this launch.
///
/// Call once, before the webview loads. The returned outcome is what the
/// protocol handler serves; the state written back carries `attempting` set
/// to whatever is about to be served, which `mark_ready` clears once the
/// frontend confirms it booted.
///
/// A surviving sentinel means the bundle it names did not boot, so that
/// bundle's directory is **removed** and the store reverts to `previous`,
/// or to the embedded floor when there is none. Removing it rather than
/// merely demoting it is deliberate: a bundle that cannot boot has no
/// second use, and leaving it on disk invites a later install promoting it
/// back into `previous`.
pub fn begin_launch(root: &Path, now_ms: u64) -> (LaunchOutcome, BundleState) {
    let mut state = read_state(root);

    let outcome = if let Some(failed) = state.attempting.take() {
        if let Some(dir) = version_dir(root, &failed) {
            let _ = std::fs::remove_dir_all(dir);
        }
        let now_serving = if state.history.is_empty() {
            None
        } else {
            Some(state.history.remove(0))
        };
        state.version.clone_from(&now_serving);
        state.installed_at_ms = now_serving.is_some().then_some(now_ms);
        LaunchOutcome::RolledBack {
            failed,
            now_serving,
        }
    } else {
        match state.version.clone() {
            // A pointer naming a version whose directory is gone is the
            // same situation as no pointer at all.
            Some(version) if bundle_is_present(root, &version) => LaunchOutcome::Bundle(version),
            Some(_) => {
                state.version = None;
                LaunchOutcome::Embedded
            }
            None => LaunchOutcome::Embedded,
        }
    };

    // Stamp for this launch. The embedded floor needs no sentinel — it is
    // the thing being fallen back TO, and cannot itself fail to boot.
    state.attempting.clone_from(&state.version);
    let _ = write_state(root, &state);
    (outcome, state)
}

/// Stamp the rollback sentinel for an IN-SESSION activation and report what
/// will be served.
///
/// The sibling of [`begin_launch`], for the case that has no launch: Stage 4
/// activates a bundle by swapping a pointer and reloading the webview, so the
/// process that stamps the sentinel is the one already running.
///
/// Returns the version now pointed at, or `None` for the embedded floor.
///
/// Three things it deliberately does NOT do, each because [`begin_launch`]
/// owns it:
///
/// - **It never rolls back.** A rollback is a verdict on a *previous*
///   attempt, reached by finding a sentinel still set at startup. Acting on
///   one here would judge the bundle the author is running right now, mid
///   session.
/// - **It never deletes anything.** Same reason.
/// - **It does not move the pointer.** It stamps and reports; choosing which
///   version to serve happens before this is called.
///
/// A pointer naming a version whose directory has gone is treated as no
/// pointer at all — identical to [`begin_launch`]'s handling, so the two
/// paths cannot disagree about what "present" means.
pub fn begin_activation(root: &Path) -> std::io::Result<Option<String>> {
    let mut state = read_state(root);

    let serving = match state.version.clone() {
        Some(version) if bundle_is_present(root, &version) => Some(version),
        Some(_) => {
            state.version = None;
            None
        }
        None => None,
    };

    // The embedded floor needs no sentinel — it is the thing being fallen
    // back TO, and cannot itself fail to boot. Same rule as `begin_launch`.
    state.attempting.clone_from(&serving);
    write_state(root, &state)?;
    Ok(serving)
}

/// Step back to the previously-active bundle and stamp the sentinel for it.
///
/// The store half of the shell's revert menu item — the escape hatch that has
/// to work when the webview renders nothing, which is why it lives here and
/// in `on_menu_event` rather than behind an IPC command.
///
/// Returns what is now active (`None` for the embedded floor), or `None`
/// early when there is nothing to step back to.
///
/// **It cycles rather than toggling, and deletes nothing.** The
/// previously-active version goes to the END of the history, not the front,
/// so reverting twice from a broken bundle reaches a *third* version instead
/// of returning to the broken one. A toggle would trap an author on exactly
/// the bundle they were trying to escape, which is the one job this has.
///
/// Nothing is deleted because an author may be stepping back over taste
/// rather than breakage; only [`begin_launch`] prunes, and only a bundle that
/// actually failed to boot.
pub fn revert(root: &Path) -> std::io::Result<Option<String>> {
    let mut state = read_state(root);
    if state.history.is_empty() {
        return Ok(state.version.clone());
    }

    let restored = state.history.remove(0);
    if !bundle_is_present(root, &restored) {
        // A history entry whose directory has gone is no target at all.
        // Drop it and leave the pointer where it is, so a second invocation
        // tries the next one rather than serving nothing.
        write_state(root, &state)?;
        return Ok(state.version.clone());
    }

    if let Some(stepped_over) = state.version.replace(restored.clone()) {
        state.history.push(stepped_over);
    }
    state.installed_at_ms = Some(0);
    state.attempting = Some(restored.clone());
    write_state(root, &state)?;
    Ok(Some(restored))
}

/// Trim `history` to the retention budget, returning what was dropped.
///
/// **`protected` survives regardless of its position**, and that exemption is
/// load-bearing rather than tidy: with a plain "keep the N most recent", a
/// version the author pinned three updates ago is pruned out from under them
/// and the forever-bundle they chose silently vanishes. The pin is the one
/// thing a retention policy must not outrank.
///
/// It is passed in rather than read here because the pin lives in the app's
/// settings, not in the store — this module stays a pure function of a root
/// path.
fn prune_history(history: &mut Vec<String>, protected: Option<&str>) -> Vec<String> {
    let budget = RETENTION.saturating_sub(1);
    let mut kept = Vec::with_capacity(history.len());
    let mut dropped = Vec::new();
    for version in history.drain(..) {
        if kept.len() < budget || protected == Some(version.as_str()) {
            kept.push(version);
        } else {
            dropped.push(version);
        }
    }
    *history = kept;
    dropped
}

/// Whether `version`'s directory exists and holds an `index.html`.
fn bundle_is_present(root: &Path, version: &str) -> bool {
    version_dir(root, version).is_some_and(|dir| dir.join(INDEX_FILE).is_file())
}

/// Clear the rollback sentinel — the frontend booted.
pub fn mark_ready(root: &Path) -> std::io::Result<()> {
    let mut state = read_state(root);
    if state.attempting.is_none() {
        return Ok(());
    }
    state.attempting = None;
    write_state(root, &state)
}

/// Promote a verified `staging/` into `<version>/` and point at it.
///
/// The rename is atomic within one filesystem, which is why `staging/` is a
/// sibling under the same root rather than a temp directory elsewhere: a
/// cross-device rename would degrade to a copy and reintroduce the
/// half-written-bundle window this ordering exists to close.
///
/// Verification (sha256, signature) happens **before** this — nothing here
/// re-checks it, so do not call it on an unverified directory.
pub fn promote(
    root: &Path,
    version: &str,
    now_ms: u64,
    protected: Option<&str>,
) -> std::io::Result<BundleState> {
    let invalid = |msg: &str| std::io::Error::new(std::io::ErrorKind::InvalidInput, msg.to_owned());

    let target = version_dir(root, version).ok_or_else(|| invalid("unsafe bundle version"))?;
    let staging = staging_dir(root);
    if !staging.join(INDEX_FILE).is_file() {
        return Err(invalid("staging holds no index.html"));
    }

    let mut state = read_state(root);
    // Re-promoting the active version would rename staging over the very
    // directory being served; treat it as a replace rather than an error.
    if target.exists() {
        std::fs::remove_dir_all(&target)?;
    }
    std::fs::rename(&staging, &target)?;

    // Roll the pointer: the version being replaced goes to the front of the
    // history, and anything past the retention budget is dropped. The
    // dropped directories are deleted here; this is the only place that
    // prunes.
    let superseded = state.version.replace(version.to_owned());
    state.installed_at_ms = Some(now_ms);
    if let Some(superseded) = superseded {
        state.history.retain(|held| held != &superseded);
        state.history.insert(0, superseded);
    }
    // The newly active version is not also history.
    state.history.retain(|held| held != version);

    for dropped in prune_history(&mut state.history, protected) {
        if let Some(dir) = version_dir(root, &dropped) {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
    write_state(root, &state)?;
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::{
        begin_activation, begin_launch, is_safe_component, mark_ready, percent_decode, promote,
        read_state, resolve_asset, revert, staging_dir, version_dir, write_state, BundleState,
        LaunchOutcome, INDEX_FILE, RETENTION,
    };
    use std::path::{Path, PathBuf};

    /// A temp root that cleans itself up. `std::env::temp_dir` plus the test
    /// name keeps the trees disjoint without a dev-dependency.
    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("brink-bundles-{name}-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).expect("create temp root");
            Self(dir)
        }
        fn path(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    /// Set the pointer directly. `install` lays down a version directory;
    /// `promote` is the staging path and is not what these tests exercise.
    fn point_at(root: &Path, version: Option<&str>, previous: Option<&str>) {
        write_state(
            root,
            &BundleState {
                version: version.map(str::to_owned),
                history: previous.into_iter().map(str::to_owned).collect(),
                installed_at_ms: Some(1),
                attempting: None,
            },
        )
        .expect("write state");
    }

    /// Reverting CYCLES rather than toggling: the version stepped over goes
    /// to the END of the history.
    ///
    /// A toggle would put an author back on the exact bundle they were
    /// escaping on the second invocation — trapping them on the one thing
    /// this feature exists to get away from.
    #[test]
    fn revert_cycles_backwards_instead_of_toggling() {
        let root = TempRoot::new("revert-cycles");
        for version in ["0.0.1", "0.0.2", "0.0.3"] {
            install(root.path(), version, &[]);
        }
        write_state(
            root.path(),
            &BundleState {
                version: Some("0.0.3".into()),
                history: vec!["0.0.2".into(), "0.0.1".into()],
                installed_at_ms: Some(1),
                attempting: None,
            },
        )
        .expect("write state");

        assert_eq!(
            revert(root.path()).expect("first"),
            Some("0.0.2".to_owned())
        );
        assert_eq!(
            revert(root.path()).expect("second"),
            Some("0.0.1".to_owned()),
            "a second revert must reach a THIRD version, not return to 0.0.3"
        );
    }

    /// Reverting stamps the sentinel for what it activates, so a bundle that
    /// still will not boot is rolled back on the next launch. The escape
    /// hatch must not be able to strand someone either.
    #[test]
    fn revert_stamps_the_sentinel_for_what_it_restores() {
        let root = TempRoot::new("revert-stamps");
        install(root.path(), "0.0.1", &[]);
        install(root.path(), "0.0.2", &[]);
        point_at(root.path(), Some("0.0.2"), Some("0.0.1"));

        assert_eq!(
            revert(root.path()).expect("revert"),
            Some("0.0.1".to_owned())
        );
        assert_eq!(read_state(root.path()).attempting.as_deref(), Some("0.0.1"));
    }

    /// Nothing to step back to is not an error and changes nothing: the
    /// embedded floor is already what a missing bundle falls through to.
    #[test]
    fn revert_with_no_history_is_a_no_op() {
        let root = TempRoot::new("revert-empty");
        install(root.path(), "0.0.1", &[]);
        point_at(root.path(), Some("0.0.1"), None);

        assert_eq!(
            revert(root.path()).expect("revert"),
            Some("0.0.1".to_owned())
        );
        assert_eq!(read_state(root.path()).version.as_deref(), Some("0.0.1"));
        assert!(read_state(root.path()).history.is_empty());
    }

    /// A history entry whose directory has gone is dropped rather than
    /// activated — otherwise the escape hatch would point the store at
    /// nothing and serve the embedded floor while claiming a version.
    #[test]
    fn revert_skips_a_history_entry_that_is_no_longer_on_disk() {
        let root = TempRoot::new("revert-missing");
        install(root.path(), "0.0.2", &[]);
        point_at(root.path(), Some("0.0.2"), Some("0.0.1")); // 0.0.1 never installed

        assert_eq!(
            revert(root.path()).expect("revert"),
            Some("0.0.2".to_owned()),
            "the pointer stays put when the target is gone"
        );
        assert!(
            read_state(root.path()).history.is_empty(),
            "the dead entry is dropped so a second invocation tries the next one"
        );
    }

    /// An in-session activation stamps the sentinel for the bundle it is
    /// about to serve. Without this the whole Stage 4 swap is unwitnessed:
    /// a bundle that wedges the webview after activation would clear
    /// nothing, and the next launch would find no sentinel and conclude it
    /// booted fine.
    #[test]
    fn begin_activation_stamps_the_sentinel_for_what_it_serves() {
        let root = TempRoot::new("activation-stamps");
        install(root.path(), "0.0.2", &[]);
        point_at(root.path(), Some("0.0.2"), None);

        let serving = begin_activation(root.path()).expect("activate");

        assert_eq!(serving.as_deref(), Some("0.0.2"));
        assert_eq!(
            read_state(root.path()).attempting.as_deref(),
            Some("0.0.2"),
            "the sentinel must be set before the reload, not after"
        );
    }

    /// The property the split between launch and activation exists to keep:
    /// a wedged ACTIVATION is caught by the next launch exactly as a wedged
    /// launch is. Nothing clears the sentinel but a booting frontend.
    #[test]
    fn an_activation_that_never_confirms_is_rolled_back_on_the_next_launch() {
        let root = TempRoot::new("activation-wedges");
        install(root.path(), "0.0.1", &[]);
        install(root.path(), "0.0.2", &[]);
        point_at(root.path(), Some("0.0.2"), Some("0.0.1"));

        // Activated, then the webview never comes back.
        begin_activation(root.path()).expect("activate");

        let (outcome, _) = begin_launch(root.path(), 3);
        assert_eq!(
            outcome,
            LaunchOutcome::RolledBack {
                failed: "0.0.2".to_owned(),
                now_serving: Some("0.0.1".to_owned()),
            },
            "a swap that wedged the webview must revert like a launch that did"
        );
    }

    /// The mirror image, and the one that would make the feature useless if
    /// it broke: an activation the frontend DOES confirm leaves nothing
    /// behind, so the next launch simply serves it.
    #[test]
    fn a_confirmed_activation_leaves_no_sentinel() {
        let root = TempRoot::new("activation-confirms");
        install(root.path(), "0.0.2", &[]);
        point_at(root.path(), Some("0.0.2"), None);
        begin_activation(root.path()).expect("activate");

        mark_ready(root.path()).expect("the reloaded bundle confirmed");

        assert_eq!(read_state(root.path()).attempting, None);
        let (outcome, _) = begin_launch(root.path(), 2);
        assert_eq!(outcome, LaunchOutcome::Bundle("0.0.2".to_owned()));
    }

    /// `begin_activation` judges "present" the same way `begin_launch` does.
    /// If the two disagreed, a pointer naming a deleted directory would be
    /// stamped as the sentinel and then roll back a bundle that was never
    /// at fault.
    #[test]
    fn begin_activation_treats_a_missing_directory_as_no_pointer() {
        let root = TempRoot::new("activation-missing");
        point_at(root.path(), Some("0.0.2"), None);

        let serving = begin_activation(root.path()).expect("activate");

        assert_eq!(serving, None, "falls back to the embedded floor");
        assert_eq!(
            read_state(root.path()).attempting,
            None,
            "the embedded floor needs no sentinel — it cannot fail to boot"
        );
    }

    /// Activation must never deliver a rollback verdict of its own. A
    /// rollback judges a PREVIOUS attempt and is reached by finding a
    /// sentinel still set at startup; reaching one here would condemn the
    /// bundle the author is running at that moment.
    #[test]
    fn begin_activation_never_deletes_or_rolls_back() {
        let root = TempRoot::new("activation-no-rollback");
        install(root.path(), "0.0.1", &[]);
        install(root.path(), "0.0.2", &[]);
        point_at(root.path(), Some("0.0.2"), Some("0.0.1"));
        // A sentinel left over from an activation that never confirmed.
        begin_activation(root.path()).expect("stamp once");

        let serving = begin_activation(root.path()).expect("activate again");

        assert_eq!(
            serving.as_deref(),
            Some("0.0.2"),
            "still serving the pointer, not rolled back"
        );
        assert!(
            version_dir(root.path(), "0.0.1")
                .expect("safe")
                .join(INDEX_FILE)
                .is_file(),
            "the previous bundle must survive — only begin_launch prunes"
        );
        assert_eq!(read_state(root.path()).history, vec!["0.0.1".to_owned()]);
    }

    /// Lay down `<root>/<version>/index.html` plus any extra files.
    fn install(root: &Path, version: &str, extra: &[(&str, &str)]) {
        let dir = version_dir(root, version).expect("safe version");
        std::fs::create_dir_all(&dir).expect("create bundle dir");
        std::fs::write(dir.join(INDEX_FILE), "<!doctype html>").expect("write index");
        for (rel, body) in extra {
            let path = dir.join(rel);
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent).expect("create parent");
            }
            std::fs::write(path, body).expect("write extra");
        }
    }

    fn stage(root: &Path, extra: &[(&str, &str)]) {
        let dir = staging_dir(root);
        std::fs::create_dir_all(&dir).expect("create staging");
        std::fs::write(dir.join(INDEX_FILE), "<!doctype html>").expect("write index");
        for (rel, body) in extra {
            std::fs::write(dir.join(rel), body).expect("write extra");
        }
    }

    // ── The pointer ────────────────────────────────────────────────

    /// The embedded floor is what every failure resolves to, so the
    /// failures have to be exhaustive rather than representative: absent,
    /// unparseable, and valid-JSON-wrong-shape are three different code
    /// paths through `serde_json`.
    #[test]
    fn an_unreadable_pointer_reads_as_the_embedded_floor() {
        let root = TempRoot::new("unreadable");

        assert_eq!(read_state(root.path()), BundleState::default(), "absent");

        std::fs::write(super::state_path(root.path()), "{ not json").expect("write");
        assert_eq!(read_state(root.path()), BundleState::default(), "corrupt");

        std::fs::write(super::state_path(root.path()), "[1,2,3]").expect("write");
        assert_eq!(
            read_state(root.path()),
            BundleState::default(),
            "wrong shape"
        );
    }

    #[test]
    fn the_pointer_round_trips() {
        let root = TempRoot::new("roundtrip");
        let state = BundleState {
            version: Some("0.7.1".into()),
            history: vec!["0.7.0".into()],
            installed_at_ms: Some(1_726_000_000_000),
            attempting: None,
        };
        write_state(root.path(), &state).expect("write");
        assert_eq!(read_state(root.path()), state);
    }

    // ── Version-component safety ───────────────────────────────────

    /// `version` reaches this module from `current.json` and, later, from a
    /// downloaded manifest — neither is trusted, and both are joined onto
    /// the store root.
    #[test]
    fn an_unsafe_version_never_becomes_a_directory() {
        for bad in [
            "..",
            ".",
            "",
            "../../etc",
            "a/b",
            "a\\b",
            "staging",
            "0.7.1\0",
            "sub dir",
        ] {
            assert!(!is_safe_component(bad), "{bad:?} should be rejected");
            assert!(
                version_dir(Path::new("/tmp/root"), bad).is_none(),
                "{bad:?} should not resolve to a directory"
            );
        }
        for good in ["0.7.1", "0.7.1-rc.1", "1.0.0+build.2", "v2_0"] {
            assert!(is_safe_component(good), "{good:?} should be accepted");
        }
    }

    // ── The resolver (the security boundary) ───────────────────────

    #[test]
    fn a_bare_path_serves_index_html() {
        let root = TempRoot::new("index");
        install(root.path(), "0.7.1", &[]);
        let dir = version_dir(root.path(), "0.7.1").expect("dir");

        for request in ["/", ""] {
            let resolved = resolve_asset(&dir, request).expect("index should resolve");
            assert!(
                resolved.ends_with(INDEX_FILE),
                "{request:?} -> {resolved:?}"
            );
        }
    }

    #[test]
    fn a_nested_asset_resolves() {
        let root = TempRoot::new("nested");
        install(
            root.path(),
            "0.7.1",
            &[("assets/index-abc123.js", "export {}")],
        );
        let dir = version_dir(root.path(), "0.7.1").expect("dir");

        let resolved = resolve_asset(&dir, "/assets/index-abc123.js").expect("asset resolves");
        assert_eq!(
            std::fs::read_to_string(&resolved).expect("read"),
            "export {}"
        );
    }

    /// Every traversal shape, including the percent-encoded ones — a
    /// decoder that judged the raw text would pass `%2e%2e` straight
    /// through, which is the whole reason the decode happens first.
    #[test]
    fn traversal_is_rejected_in_every_encoding() {
        let root = TempRoot::new("traversal");
        install(root.path(), "0.7.1", &[]);
        std::fs::write(root.path().join("secret.txt"), "no").expect("write secret");
        let dir = version_dir(root.path(), "0.7.1").expect("dir");

        // Precondition: the file really is there to be stolen, so a pass
        // below would be a real escape rather than a missing-file 404.
        assert!(root.path().join("secret.txt").is_file());

        for request in [
            "/../secret.txt",
            "/../../secret.txt",
            "/assets/../../secret.txt",
            "/%2e%2e/secret.txt",
            "/%2E%2E/secret.txt",
            "/..%2fsecret.txt",
            "/%2e%2e%2fsecret.txt",
            "/./../secret.txt",
        ] {
            assert!(
                resolve_asset(&dir, request).is_none(),
                "{request:?} escaped the bundle directory"
            );
        }
    }

    #[test]
    fn an_absolute_or_malformed_request_is_rejected() {
        let root = TempRoot::new("malformed");
        install(root.path(), "0.7.1", &[]);
        let dir = version_dir(root.path(), "0.7.1").expect("dir");

        for request in [
            "//etc/passwd",   // root prefix after trimming
            "/%",             // truncated escape
            "/%zz",           // non-hex escape
            "/%00",           // NUL
            "/index.html%00", // NUL suffix
        ] {
            assert!(
                resolve_asset(&dir, request).is_none(),
                "{request:?} should be rejected"
            );
        }
    }

    #[test]
    fn a_missing_file_resolves_to_none_so_the_caller_falls_back() {
        let root = TempRoot::new("missing");
        install(root.path(), "0.7.1", &[]);
        let dir = version_dir(root.path(), "0.7.1").expect("dir");
        assert!(resolve_asset(&dir, "/assets/not-here.js").is_none());
    }

    #[test]
    fn a_directory_is_not_served_as_a_file() {
        let root = TempRoot::new("isdir");
        install(root.path(), "0.7.1", &[("assets/a.js", "x")]);
        let dir = version_dir(root.path(), "0.7.1").expect("dir");
        assert!(resolve_asset(&dir, "/assets").is_none());
    }

    /// The check no string rule can make: a symlink inside the bundle
    /// pointing out of it. Only the canonicalized-prefix comparison sees
    /// this, which is why that comparison exists.
    #[cfg(unix)]
    #[test]
    fn a_symlink_escaping_the_bundle_is_rejected() {
        let root = TempRoot::new("symlink");
        install(root.path(), "0.7.1", &[]);
        std::fs::write(root.path().join("secret.txt"), "no").expect("write secret");
        let dir = version_dir(root.path(), "0.7.1").expect("dir");

        std::os::unix::fs::symlink(root.path().join("secret.txt"), dir.join("escape.txt"))
            .expect("symlink");

        // Precondition: the link really does resolve to the secret, so a
        // pass below would be a real escape.
        assert_eq!(
            std::fs::read_to_string(dir.join("escape.txt")).expect("read through link"),
            "no"
        );
        assert!(
            resolve_asset(&dir, "/escape.txt").is_none(),
            "a symlink out of the bundle was served"
        );
    }

    #[test]
    fn percent_decode_rejects_a_malformed_escape() {
        assert_eq!(percent_decode("/a%2Fb").as_deref(), Some("/a/b"));
        assert_eq!(percent_decode("/plain.js").as_deref(), Some("/plain.js"));
        assert!(percent_decode("/a%").is_none());
        assert!(percent_decode("/a%2").is_none());
        assert!(percent_decode("/a%gg").is_none());
    }

    // ── Launch, sentinel, rollback ─────────────────────────────────

    #[test]
    fn a_fresh_install_serves_embedded_and_stamps_nothing() {
        let root = TempRoot::new("fresh");
        let (outcome, state) = begin_launch(root.path(), 1);
        assert_eq!(outcome, LaunchOutcome::Embedded);
        assert_eq!(state.attempting, None, "the floor needs no sentinel");
    }

    #[test]
    fn an_installed_bundle_is_served_and_stamped() {
        let root = TempRoot::new("stamped");
        install(root.path(), "0.7.1", &[]);
        write_state(
            root.path(),
            &BundleState {
                version: Some("0.7.1".into()),
                ..BundleState::default()
            },
        )
        .expect("write");

        let (outcome, state) = begin_launch(root.path(), 1);
        assert_eq!(outcome, LaunchOutcome::Bundle("0.7.1".into()));
        assert_eq!(state.attempting.as_deref(), Some("0.7.1"));
        assert_eq!(
            read_state(root.path()).attempting.as_deref(),
            Some("0.7.1"),
            "the sentinel must be on DISK before the webview loads"
        );
    }

    #[test]
    fn a_booting_bundle_clears_its_sentinel() {
        let root = TempRoot::new("ready");
        install(root.path(), "0.7.1", &[]);
        write_state(
            root.path(),
            &BundleState {
                version: Some("0.7.1".into()),
                ..BundleState::default()
            },
        )
        .expect("write");

        begin_launch(root.path(), 1);
        mark_ready(root.path()).expect("mark ready");
        assert_eq!(read_state(root.path()).attempting, None);

        // The next launch sees no sentinel and serves it again.
        let (outcome, _) = begin_launch(root.path(), 2);
        assert_eq!(outcome, LaunchOutcome::Bundle("0.7.1".into()));
    }

    /// The headline rollback path: a sentinel that survived a launch.
    #[test]
    fn a_surviving_sentinel_rolls_back_to_the_previous_bundle() {
        let root = TempRoot::new("rollback");
        install(root.path(), "0.7.0", &[]);
        install(root.path(), "0.7.1", &[]);
        write_state(
            root.path(),
            &BundleState {
                version: Some("0.7.1".into()),
                history: vec!["0.7.0".into()],
                installed_at_ms: Some(1),
                // Stamped by the previous launch and never cleared.
                attempting: Some("0.7.1".into()),
            },
        )
        .expect("write");

        let (outcome, state) = begin_launch(root.path(), 9);
        assert_eq!(
            outcome,
            LaunchOutcome::RolledBack {
                failed: "0.7.1".into(),
                now_serving: Some("0.7.0".into()),
            }
        );
        assert_eq!(state.version.as_deref(), Some("0.7.0"));
        assert!(state.history.is_empty(), "0.7.0 is now active, not spare");
        assert_eq!(
            state.attempting.as_deref(),
            Some("0.7.0"),
            "the bundle we reverted TO is now the one being attempted"
        );
        assert!(
            version_dir(root.path(), "0.7.1").is_some_and(|d| !d.exists()),
            "a bundle that cannot boot should be removed, not kept"
        );
    }

    #[test]
    fn a_surviving_sentinel_with_no_previous_falls_back_to_embedded() {
        let root = TempRoot::new("rollback-floor");
        install(root.path(), "0.7.1", &[]);
        write_state(
            root.path(),
            &BundleState {
                version: Some("0.7.1".into()),
                attempting: Some("0.7.1".into()),
                ..BundleState::default()
            },
        )
        .expect("write");

        let (outcome, state) = begin_launch(root.path(), 9);
        assert_eq!(
            outcome,
            LaunchOutcome::RolledBack {
                failed: "0.7.1".into(),
                now_serving: None,
            }
        );
        assert_eq!(state.version, None);
        assert_eq!(state.attempting, None);
    }

    /// Two failures in a row walk the ladder down to the floor rather than
    /// pinning the author on a second bundle that also cannot boot.
    #[test]
    fn successive_failures_walk_down_to_the_embedded_floor() {
        let root = TempRoot::new("ladder");
        install(root.path(), "0.7.0", &[]);
        install(root.path(), "0.7.1", &[]);
        write_state(
            root.path(),
            &BundleState {
                version: Some("0.7.1".into()),
                history: vec!["0.7.0".into()],
                attempting: Some("0.7.1".into()),
                installed_at_ms: Some(1),
            },
        )
        .expect("write");

        // 0.7.1 failed -> serving 0.7.0, stamped.
        begin_launch(root.path(), 2);
        // 0.7.0 failed too -> the floor.
        let (outcome, state) = begin_launch(root.path(), 3);
        assert_eq!(
            outcome,
            LaunchOutcome::RolledBack {
                failed: "0.7.0".into(),
                now_serving: None,
            }
        );
        assert_eq!(state.version, None);
    }

    /// A pointer naming a directory that is not there (a hand-deleted
    /// bundle — the documented recovery) is the same as no pointer.
    #[test]
    fn a_pointer_to_a_missing_directory_serves_embedded() {
        let root = TempRoot::new("dangling");
        write_state(
            root.path(),
            &BundleState {
                version: Some("0.7.1".into()),
                ..BundleState::default()
            },
        )
        .expect("write");

        let (outcome, state) = begin_launch(root.path(), 1);
        assert_eq!(outcome, LaunchOutcome::Embedded);
        assert_eq!(state.version, None);
        assert_eq!(state.attempting, None);
    }

    // ── Promotion ──────────────────────────────────────────────────

    #[test]
    fn promote_moves_staging_into_place_and_points_at_it() {
        let root = TempRoot::new("promote");
        stage(root.path(), &[("app.js", "console.log(1)")]);

        let state = promote(root.path(), "0.7.1", 42, None).expect("promote");
        assert_eq!(state.version.as_deref(), Some("0.7.1"));
        assert_eq!(state.installed_at_ms, Some(42));
        assert!(!staging_dir(root.path()).exists(), "staging is consumed");

        let dir = version_dir(root.path(), "0.7.1").expect("dir");
        assert_eq!(
            std::fs::read_to_string(dir.join("app.js")).expect("read"),
            "console.log(1)"
        );
        assert_eq!(read_state(root.path()).version.as_deref(), Some("0.7.1"));
    }

    /// The retention budget: the active bundle plus `RETENTION - 1` of
    /// history, and the overflow's directories are deleted rather than
    /// orphaned.
    #[test]
    fn promote_keeps_the_retention_budget_and_deletes_the_rest() {
        let root = TempRoot::new("prune");

        for (index, version) in ["0.7.0", "0.7.1", "0.7.2", "0.7.3"].iter().enumerate() {
            stage(root.path(), &[]);
            promote(root.path(), version, index as u64 + 1, None).expect("promote");
        }

        let state = read_state(root.path());
        assert_eq!(state.version.as_deref(), Some("0.7.3"));
        assert_eq!(
            state.history,
            vec!["0.7.2".to_owned(), "0.7.1".to_owned()],
            "most recent first, capped at RETENTION - 1"
        );
        assert_eq!(state.history.len(), RETENTION - 1);
        assert!(
            version_dir(root.path(), "0.7.0").is_some_and(|d| !d.exists()),
            "the dropped bundle's directory should be deleted, not orphaned"
        );
        for kept in ["0.7.1", "0.7.2", "0.7.3"] {
            assert!(
                version_dir(root.path(), kept).is_some_and(|d| d.exists()),
                "{kept} should still be on disk"
            );
        }
    }

    /// **The pin outranks the retention budget.** Without this exemption a
    /// version the author pinned three updates ago is pruned out from under
    /// them, and the forever-bundle they chose silently vanishes — which is
    /// the one thing a retention policy must not be allowed to do.
    #[test]
    fn promote_never_prunes_the_pinned_version() {
        let root = TempRoot::new("prune-pinned");

        for (index, version) in ["0.7.0", "0.7.1", "0.7.2", "0.7.3"].iter().enumerate() {
            stage(root.path(), &[]);
            promote(root.path(), version, index as u64 + 1, Some("0.7.0")).expect("promote");
        }

        let state = read_state(root.path());
        assert!(
            state.history.contains(&"0.7.0".to_owned()),
            "the pinned version must survive its position in the history: {:?}",
            state.history
        );
        assert!(
            version_dir(root.path(), "0.7.0").is_some_and(|d| d.exists()),
            "the pinned bundle's directory must still be on disk"
        );
        // The exemption ADDS to the budget rather than displacing a recent
        // one: an author who pinned an old version still gets rollback.
        assert!(
            state.history.contains(&"0.7.2".to_owned()),
            "the most recent history entry must not be sacrificed for the pin"
        );
    }

    /// Re-promoting a version already in the history must not leave it in
    /// both places — a picker would show the active bundle as a rollback
    /// target, and the budget would be silently spent twice on one version.
    #[test]
    fn promote_does_not_leave_a_version_in_both_active_and_history() {
        let root = TempRoot::new("prune-dup");

        stage(root.path(), &[]);
        promote(root.path(), "0.7.0", 1, None).expect("first");
        stage(root.path(), &[]);
        promote(root.path(), "0.7.1", 2, None).expect("second");
        // Back to the older one, as a rollback or a pin would.
        stage(root.path(), &[]);
        let state = promote(root.path(), "0.7.0", 3, None).expect("third");

        assert_eq!(state.version.as_deref(), Some("0.7.0"));
        assert!(
            !state.history.contains(&"0.7.0".to_owned()),
            "the active version must not also be history: {:?}",
            state.history
        );
        assert_eq!(state.history, vec!["0.7.1".to_owned()]);
    }

    #[test]
    fn promote_refuses_an_unsafe_version_or_an_empty_staging() {
        let root = TempRoot::new("refuse");

        stage(root.path(), &[]);
        assert!(
            promote(root.path(), "../escape", 1, None).is_err(),
            "unsafe version"
        );

        let _ = std::fs::remove_dir_all(staging_dir(root.path()));
        assert!(
            promote(root.path(), "0.7.1", 1, None).is_err(),
            "no staging"
        );

        // A staging directory with no index.html is a failed extract, not a
        // bundle — promoting it would point the shell at nothing.
        std::fs::create_dir_all(staging_dir(root.path())).expect("mkdir");
        std::fs::write(staging_dir(root.path()).join("stray.txt"), "x").expect("write");
        assert!(
            promote(root.path(), "0.7.1", 1, None).is_err(),
            "no index.html"
        );
    }
}
