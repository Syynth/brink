//! Fetching and verifying an OTA web bundle (`docs/desktop-ota-spec.md`
//! Stage 2).
//!
//! Everything that decides *whether* to install, and everything that decides
//! whether a downloaded archive is trustworthy, lives here as a pure
//! function of its inputs. Only [`fetch_bytes`] touches the network, and it
//! is a thin wrapper the tests never need — which is what lets the
//! `minShellVersion` gate, the hash check, the signature check and the
//! archive-entry rules all be exercised by `cargo test` with no server, no
//! keypair and no disk beyond a temp directory.
//!
//! ## The order is the security property
//!
//! Verification happens **before anything is extracted**: hash, then
//! signature, then unpack. Extracting first and checking afterwards would
//! mean a hostile archive had already written to disk by the time it was
//! rejected — and the `staging/` directory it wrote into is one `rename`
//! away from being served.
//!
//! ## Why `.tar.gz` and not `.tar.zst`
//!
//! The spec drafted `.tar.zst`. `flate2` and `tar` are ALREADY in this
//! crate's dependency graph — `tauri-plugin-updater` pulls both, and ships
//! its own macOS payload as `.app.tar.gz` — while `zstd` is not, and would
//! add a C toolchain dependency (`zstd-sys`) to a crate that duplicates the
//! repo lint policy by hand and carries its own `cargo-deny` gate. The
//! payoff would be perhaps 15% off a ~10 MB download. Gzip is the cheaper
//! trade, and it is the format the sibling channel already uses, so one
//! archive reader covers both.

use std::path::{Component, Path};

/// The OTA manifest, served beside the full-app `latest.json`.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleManifest {
    /// The bundle's version — also its directory name under the store.
    pub version: String,
    /// The oldest shell that may run this bundle. Mandatory: see
    /// [`decide`].
    pub min_shell_version: String,
    /// Where the `.tar.gz` archive lives.
    pub url: String,
    /// Lowercase hex SHA-256 of the archive bytes.
    pub sha256: String,
    /// Base64 minisign signature, in the same encoding
    /// `tauri-plugin-updater` uses for `latest.json`.
    pub signature: String,
    /// Publication timestamp, carried for display only.
    #[serde(default)]
    pub pub_date: Option<String>,
    /// Which stream this bundle belongs to.
    ///
    /// Defaults to stable so an entry written before channels existed reads
    /// as the conservative choice rather than failing the whole index.
    #[serde(default)]
    pub channel: Channel,
}

/// Which stream of bundles an entry belongs to.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Channel {
    #[default]
    Stable,
    Beta,
}

/// Every published bundle, in one file.
///
/// Replaces the per-channel `bundle-latest.json` because one fetch then
/// serves all three jobs a policy can ask for: the newest entry on my
/// channel, the list a picker renders, and the url a pinned version resolves
/// to. At roughly 300 bytes an entry that is cheaper than the two round
/// trips a split design would cost.
///
/// **Trust is unchanged by the merge.** Each entry carries its own archive's
/// signature, checked by [`verify_and_unpack`] against the public key in
/// `tauri.conf.json`, so a tampered index can at worst offer a differently
/// signed valid bundle or one that fails verification. It cannot introduce
/// unsigned code, and therefore needs no signature of its own.
///
/// Growth is bounded on the client by the caller's fetch cap; the publisher
/// caps the entry count when it appends.
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BundleIndex {
    #[serde(default)]
    pub entries: Vec<BundleManifest>,
}

/// What an update policy asks the index for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Target<'a> {
    /// The newest entry on this channel.
    Latest(Channel),
    /// Exactly this version, whatever its channel or ordering.
    Version(&'a str),
}

/// Find the entry a target names, or `None` when the index has nothing for it.
///
/// `Latest` keeps the FIRST of any duplicate-version entries rather than the
/// last, so the answer does not depend on how a publisher happened to order
/// its appends — the same determinism rule the rest of the project applies to
/// iteration order.
pub fn resolve<'i>(index: &'i BundleIndex, target: Target<'_>) -> Option<&'i BundleManifest> {
    match target {
        Target::Version(wanted) => index.entries.iter().find(|entry| entry.version == wanted),
        Target::Latest(channel) => {
            let mut best: Option<(&BundleManifest, semver::Version)> = None;
            for entry in index.entries.iter().filter(|e| e.channel == channel) {
                let Ok(parsed) = semver::Version::parse(&entry.version) else {
                    continue;
                };
                if best.as_ref().is_none_or(|(_, seen)| parsed > *seen) {
                    best = Some((entry, parsed));
                }
            }
            best.map(|(entry, _)| entry)
        }
    }
}

/// Why the shell is asking, which decides whether ordering applies.
///
/// **A channel switch is an install, not an update** (RULED 2026-09-15).
/// [`decide`] only installs strictly-newer versions, so moving beta to
/// stable would otherwise be refused forever — a beta `0.2.0-beta.1` sorts
/// above a stable `0.1.9`, and the author would be stranded on beta with no
/// way back. Picking a past version to pin has the same shape.
///
/// The `minShellVersion` gate applies to both, which is why this is one
/// parameter rather than two functions: a second entry point could drift
/// into skipping the gate that keeps a bundle off a shell that cannot run it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Offer only something newer than what is installed.
    Update,
    /// Install exactly what was asked for, ordering notwithstanding.
    Switch,
}

/// Why an update was refused, or that there is nothing to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UpdateDecision {
    /// Nothing newer is on offer.
    UpToDate,
    /// Install it.
    Install,
    /// Refused, with a reason fit to show the author.
    Refuse(String),
}

/// Errors from the verify-and-unpack path.
#[derive(Debug, thiserror::Error)]
pub enum UpdateError {
    #[error("manifest is not valid JSON: {0}")]
    Manifest(String),
    #[error("archive hash does not match the manifest")]
    HashMismatch,
    #[error("archive signature is not valid for this app's key: {0}")]
    BadSignature(String),
    #[error("archive is malformed: {0}")]
    BadArchive(String),
    #[error("archive entry {0:?} is not allowed")]
    UnsafeEntry(String),
    #[error("io error on {path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// Parse a manifest document.
/// Parse the published index, **skipping entries that do not parse**.
///
/// Per-entry rather than all-or-nothing, and that is a deliberate blast-radius
/// choice: every install fetches this one file, so a single malformed entry
/// failing the whole document would take every install offline at once — from
/// one bad publish, with no way to recover except republishing. Dropping the
/// bad entry leaves every other version resolvable.
///
/// What it does NOT do is relax what an entry must contain. `minShellVersion`
/// stays mandatory (no serde default), because it carries the only protection
/// against OTA'd JS calling an IPC command the installed shell lacks — so an
/// entry missing it is not an entry with a default, it is one that must never
/// be offered. It is skipped, not repaired.
///
/// The document itself must still be JSON; that failure is the publisher's
/// and is reported.
pub fn parse_index(text: &str) -> Result<BundleIndex, UpdateError> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct RawIndex {
        #[serde(default)]
        entries: Vec<serde_json::Value>,
    }

    let raw: RawIndex =
        serde_json::from_str(text).map_err(|e| UpdateError::Manifest(e.to_string()))?;
    Ok(BundleIndex {
        entries: raw
            .entries
            .into_iter()
            .filter_map(|entry| serde_json::from_value(entry).ok())
            .collect(),
    })
}

/// Should this manifest be installed on this shell?
///
/// Two gates, and the order matters only for which message the author sees:
///
/// - **`minShellVersion` is mandatory, not advisory.** With the sidecar gone
///   the one remaining native coupling is the IPC surface, and OTA'd JS that
///   calls a `#[tauri::command]` the installed shell does not have is a hard
///   break with no recovery short of a rollback. The manifest is the only
///   place that can catch it, so a bundle asking for a newer shell is
///   refused *and says so* — silently reporting "up to date" would leave the
///   author waiting for an update that will never apply.
/// - **Not newer is not an update.** Equal or older than what is installed
///   is `UpToDate`, which also makes a re-offer after a rollback a no-op
///   rather than a reinstall loop.
///
/// `installed` is `None` when the embedded floor is active, in which case
/// any manifest the shell can run is an install.
pub fn decide(
    manifest: &BundleManifest,
    shell_version: &str,
    installed: Option<&str>,
    intent: Intent,
) -> UpdateDecision {
    let Ok(shell) = semver::Version::parse(shell_version) else {
        return UpdateDecision::Refuse(format!(
            "this app reports an unparseable version ({shell_version}); refusing to \
             evaluate an update against it"
        ));
    };
    let Ok(required) = semver::Version::parse(&manifest.min_shell_version) else {
        return UpdateDecision::Refuse(format!(
            "update {} declares an unparseable minShellVersion ({})",
            manifest.version, manifest.min_shell_version
        ));
    };
    if required > shell {
        return UpdateDecision::Refuse(format!(
            "update {} needs app version {} or newer; this app is {shell_version}. \
             Install the full app update first.",
            manifest.version, manifest.min_shell_version
        ));
    }

    let Ok(offered) = semver::Version::parse(&manifest.version) else {
        return UpdateDecision::Refuse(format!(
            "update declares an unparseable version ({})",
            manifest.version
        ));
    };
    // A SWITCH is not an update and must not be judged by recency: moving
    // from beta to stable, or back to a version the author pinned, goes
    // "backwards" by design. Only the minShellVersion gate above applies.
    if intent == Intent::Switch {
        return if installed == Some(manifest.version.as_str()) {
            UpdateDecision::UpToDate
        } else {
            UpdateDecision::Install
        };
    }

    match installed.map(semver::Version::parse) {
        // An unparseable INSTALLED version means the store is in a state
        // this code did not write. Refuse rather than overwrite it blindly.
        Some(Err(_)) => UpdateDecision::Refuse(
            "the installed bundle has an unrecognisable version; remove it before updating"
                .to_owned(),
        ),
        Some(Ok(current)) if offered <= current => UpdateDecision::UpToDate,
        _ => UpdateDecision::Install,
    }
}

/// Whether `bytes` hashes to `expected` (lowercase hex SHA-256).
///
/// Compared case-insensitively on the hex, which is a formatting question,
/// not a security one — the digest itself is compared in full.
pub fn hash_matches(bytes: &[u8], expected: &str) -> bool {
    use sha2::Digest;
    let actual = sha2::Sha256::digest(bytes);
    hex_lower(&actual).eq_ignore_ascii_case(expected.trim())
}

/// Lowercase hex, without a dependency for sixteen characters.
fn hex_lower(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    bytes
        .iter()
        .fold(String::with_capacity(bytes.len() * 2), |mut out, b| {
            // Writing to a String is infallible; the Result exists for the trait.
            let _ = write!(out, "{b:02x}");
            out
        })
}

/// Verify a minisign signature over `bytes`.
///
/// ⚠ Mirrors `tauri-plugin-updater`'s own `verify_signature` deliberately
/// and exactly: base64-decode BOTH the public key and the signature into
/// their minisign *text* forms first, then `decode` those. The 2026-09-14
/// ruling shares one keypair across both channels, so a different parse here
/// would reject signatures the updater accepts — the two must agree by
/// construction, not by coincidence. `allow_legacy` is `true` for the same
/// reason: to accept exactly what the updater accepts.
pub fn signature_is_valid(bytes: &[u8], signature_b64: &str, pubkey_b64: &str) -> bool {
    use base64::Engine as _;

    let decode = |s: &str| -> Option<String> {
        let raw = base64::engine::general_purpose::STANDARD.decode(s).ok()?;
        String::from_utf8(raw).ok()
    };

    let (Some(pubkey_text), Some(signature_text)) = (decode(pubkey_b64), decode(signature_b64))
    else {
        return false;
    };
    let (Ok(public_key), Ok(signature)) = (
        minisign_verify::PublicKey::decode(&pubkey_text),
        minisign_verify::Signature::decode(&signature_text),
    ) else {
        return false;
    };
    public_key.verify(bytes, &signature, true).is_ok()
}

/// Unpack a verified `.tar.gz` into `dest`, refusing anything that could
/// write outside it.
///
/// ⚠ The archive is attacker-shaped input even after the signature check —
/// a signature proves *who* produced it, never that its contents are
/// well-formed — so every entry is judged on its own:
///
/// - only [`Component::Normal`] path components, which rejects `..`, `.`, a
///   root prefix and a Windows drive prefix;
/// - **regular files and directories only.** A symlink or hardlink entry is
///   refused outright rather than sanitised: `tar`'s own `unpack` follows
///   links, and a link is the one entry type whose target is not the path
///   the entry declares. `bundles::resolve_asset` refuses to *serve* through
///   a symlink as defence in depth, but nothing should have written one.
///
/// `dest` is created if missing and is expected to be the store's
/// `staging/`, which `bundles::promote` then renames into place.
pub fn unpack_verified(archive: &[u8], dest: &Path) -> Result<(), UpdateError> {
    let io = |path: &Path, source: std::io::Error| UpdateError::Io {
        path: path.display().to_string(),
        source,
    };

    std::fs::create_dir_all(dest).map_err(|e| io(dest, e))?;

    let decoder = flate2::read::GzDecoder::new(archive);
    let mut tar = tar::Archive::new(decoder);
    // Belt and braces: `tar`'s own guards stay on, and the per-entry checks
    // below run anyway. Neither is trusted alone.
    tar.set_preserve_permissions(false);
    tar.set_overwrite(true);

    let entries = tar
        .entries()
        .map_err(|e| UpdateError::BadArchive(e.to_string()))?;

    for entry in entries {
        let mut entry = entry.map_err(|e| UpdateError::BadArchive(e.to_string()))?;
        let path = entry
            .path()
            .map_err(|e| UpdateError::BadArchive(e.to_string()))?
            .into_owned();
        let display = path.display().to_string();

        let kind = entry.header().entry_type();
        if !(kind.is_file() || kind.is_dir()) {
            return Err(UpdateError::UnsafeEntry(display));
        }
        if !path.components().all(|c| matches!(c, Component::Normal(_))) {
            return Err(UpdateError::UnsafeEntry(display));
        }

        let target = dest.join(&path);
        if kind.is_dir() {
            std::fs::create_dir_all(&target).map_err(|e| io(&target, e))?;
            continue;
        }
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).map_err(|e| io(parent, e))?;
        }
        let mut out = std::fs::File::create(&target).map_err(|e| io(&target, e))?;
        std::io::copy(&mut entry, &mut out).map_err(|e| io(&target, e))?;
    }
    Ok(())
}

/// Verify and unpack in the one order that is safe.
///
/// Hash first (cheap, catches a truncated or corrupted download), signature
/// second (catches a substituted one), unpack last. Nothing reaches the disk
/// until both pass.
pub fn verify_and_unpack(
    archive: &[u8],
    manifest: &BundleManifest,
    pubkey_b64: &str,
    dest: &Path,
) -> Result<(), UpdateError> {
    if !hash_matches(archive, &manifest.sha256) {
        return Err(UpdateError::HashMismatch);
    }
    if !signature_is_valid(archive, &manifest.signature, pubkey_b64) {
        return Err(UpdateError::BadSignature(format!(
            "bundle {}",
            manifest.version
        )));
    }
    unpack_verified(archive, dest)
}

#[cfg(test)]
mod tests {
    use super::{
        decide, hash_matches, parse_index, resolve, signature_is_valid, unpack_verified,
        verify_and_unpack, BundleIndex, BundleManifest, Channel, Intent, Target, UpdateDecision,
        UpdateError,
    };
    /// Build an index entry.
    fn entry(version: &str, channel: Channel) -> BundleManifest {
        BundleManifest {
            channel,
            version: version.to_owned(),
            min_shell_version: "0.8.0".to_owned(),
            url: format!("https://example.invalid/{version}.tar.gz"),
            sha256: "00".repeat(32),
            signature: "sig".to_owned(),
            pub_date: None,
        }
    }

    fn index(entries: &[(&str, Channel)]) -> BundleIndex {
        BundleIndex {
            entries: entries.iter().map(|(v, c)| entry(v, *c)).collect(),
        }
    }

    /// Latest-for-a-channel ignores the other channel entirely. Without
    /// this, a beta published after a stable would be offered to every
    /// stable install — the failure this whole field exists to prevent.
    #[test]
    fn latest_is_per_channel_and_ignores_the_other_stream() {
        let published = index(&[
            ("0.1.0", Channel::Stable),
            ("0.2.0-beta.1", Channel::Beta),
            ("0.1.9", Channel::Stable),
        ]);

        assert_eq!(
            resolve(&published, Target::Latest(Channel::Stable)).map(|m| m.version.as_str()),
            Some("0.1.9")
        );
        assert_eq!(
            resolve(&published, Target::Latest(Channel::Beta)).map(|m| m.version.as_str()),
            Some("0.2.0-beta.1")
        );
    }

    /// An entry whose version is not semver must not sink the whole channel.
    /// Skipping it leaves the rest resolvable; propagating would make one bad
    /// publish take every install offline.
    #[test]
    fn an_unparseable_entry_is_skipped_rather_than_poisoning_the_channel() {
        let published = index(&[
            ("not-a-version", Channel::Stable),
            ("0.1.0", Channel::Stable),
        ]);
        assert_eq!(
            resolve(&published, Target::Latest(Channel::Stable)).map(|m| m.version.as_str()),
            Some("0.1.0")
        );
    }

    /// Targeting a version finds it whatever its channel — that is what
    /// makes a pin a pin, and what lets an author pin a beta.
    #[test]
    fn a_targeted_version_resolves_across_channels_or_not_at_all() {
        let published = index(&[("0.1.0", Channel::Stable), ("0.2.0-beta.1", Channel::Beta)]);

        assert_eq!(
            resolve(&published, Target::Version("0.2.0-beta.1")).map(|m| m.version.as_str()),
            Some("0.2.0-beta.1")
        );
        assert!(resolve(&published, Target::Version("9.9.9")).is_none());
    }

    /// **The case that would strand someone on beta forever.**
    ///
    /// `Intent::Update` only installs strictly-newer versions, and a beta
    /// sorts ABOVE the stable it precedes — so switching back to stable
    /// reads as a downgrade and is refused. `Intent::Switch` exists for
    /// exactly this, and the minShellVersion gate still applies to it.
    #[test]
    fn switching_back_to_stable_is_refused_as_an_update_and_allowed_as_a_switch() {
        let stable = entry("0.1.9", Channel::Stable);
        let installed_beta = Some("0.2.0-beta.1");

        assert_eq!(
            decide(&stable, "0.8.0", installed_beta, Intent::Update),
            UpdateDecision::UpToDate,
            "as an update this looks like a downgrade, and the author is stuck"
        );
        assert_eq!(
            decide(&stable, "0.8.0", installed_beta, Intent::Switch),
            UpdateDecision::Install,
            "as a switch it is exactly what was asked for"
        );
    }

    /// A switch still cannot put a bundle on a shell that cannot run it.
    /// This is why `Intent` is a parameter rather than a second function:
    /// a separate entry point could drift into skipping this gate.
    #[test]
    fn a_switch_still_obeys_min_shell_version() {
        let needs_newer = BundleManifest {
            min_shell_version: "9.9.9".to_owned(),
            ..entry("0.1.0", Channel::Stable)
        };
        assert!(matches!(
            decide(&needs_newer, "0.8.0", None, Intent::Switch),
            UpdateDecision::Refuse(_)
        ));
    }

    /// Re-switching to what is already active is a no-op, not a reinstall.
    #[test]
    fn switching_to_the_active_version_is_up_to_date() {
        let same = entry("0.1.0", Channel::Stable);
        assert_eq!(
            decide(&same, "0.8.0", Some("0.1.0"), Intent::Switch),
            UpdateDecision::UpToDate
        );
    }

    /// An index written before channels existed reads as stable rather than
    /// failing, so the first published index need not be rewritten.
    #[test]
    fn an_entry_without_a_channel_defaults_to_stable() {
        let text = r#"{"entries":[{"version":"0.1.0","minShellVersion":"0.8.0",
            "url":"https://example.invalid/a.tar.gz","sha256":"00","signature":"s"}]}"#;
        let parsed = parse_index(text).expect("index should parse");
        assert_eq!(
            parsed.entries.first().map(|e| e.channel),
            Some(Channel::Stable)
        );
    }

    use std::io::Write as _;
    use std::path::{Path, PathBuf};

    /// A REAL Ed25519 keypair in minisign's own file encoding, wrapped the
    /// way `tauri-plugin-updater` wraps them (base64 of the file text).
    ///
    /// Generated once with Node's `crypto.sign(null, …, ed25519)` over
    /// [`FIXTURE_PAYLOAD`], in the legacy (`Ed`, non-prehashed) form the
    /// updater accepts with `allow_legacy: true`. Checked in rather than
    /// signed at test time because nothing in this crate can SIGN — and a
    /// suite that only ever proved signatures are *rejected* would pass
    /// just as happily against a `signature_is_valid` that returned `false`
    /// unconditionally. The positive case is what makes the negatives mean
    /// something.
    const FIXTURE_PAYLOAD: &[u8] = b"brink ota signature fixture v1\n";
    const FIXTURE_PUBKEY_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IGJyaW5rIHRlc3Qga2V5ClJXUUJBZ01FQlFZSENLQW9qMEpPQmVhWkswWGpVWUNVcnd0S1BkMzBXdlpsbDBNVVFXbUZQcHlyCg==";
    const FIXTURE_SIG_B64: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIGJyaW5rIHRlc3Qga2V5ClJXUUJBZ01FQlFZSENFK1VYMXlzaFpqWHdQQlJCMjZ4N1crNThsamlsY3NQbTFPOXZ5RlM3TWgvM0drU3JGb3RlK0NrYnJRM1NGSVpGQVdEQTdweWQ4dDdMVVozY1AzaWtBST0KdHJ1c3RlZCBjb21tZW50OiBicmluayBvdGEgZml4dHVyZQp3QjE1a0R2bWthNHJXbVJta0hzbEZXMmNMbm9oOEFkOFdVa1pzaHRXLzBBSUMwUmVCNzEyUzJrVXp3RHNLU3FrbXY3ZU5RbXpIWUVsek9YWEhRQitDZz09Cg==";

    struct TempRoot(PathBuf);

    impl TempRoot {
        fn new(name: &str) -> Self {
            let dir =
                std::env::temp_dir().join(format!("brink-update-{name}-{}", std::process::id()));
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

    fn manifest(version: &str, min_shell: &str) -> BundleManifest {
        BundleManifest {
            channel: Channel::Stable,
            version: version.to_owned(),
            min_shell_version: min_shell.to_owned(),
            url: "https://example.invalid/bundle.tar.gz".to_owned(),
            sha256: String::new(),
            signature: String::new(),
            pub_date: None,
        }
    }

    /// Build a `.tar.gz` from `(path, body)` pairs.
    fn targz(entries: &[(&str, &str)]) -> Vec<u8> {
        let mut builder = tar::Builder::new(Vec::new());
        for (name, body) in entries {
            let body = body.as_bytes();
            let mut header = tar::Header::new_gnu();
            header.set_size(body.len() as u64);
            header.set_mode(0o644);
            header.set_cksum();
            builder
                .append_data(&mut header, name, body)
                .expect("append entry");
        }
        let tar_bytes = builder.into_inner().expect("finish tar");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).expect("gzip");
        encoder.finish().expect("finish gzip")
    }

    fn sha256_hex(bytes: &[u8]) -> String {
        use sha2::Digest as _;
        super::hex_lower(&sha2::Sha256::digest(bytes))
    }

    // ── Manifest ───────────────────────────────────────────────────

    #[test]
    fn an_entry_parses_from_the_documented_shape() {
        let parsed = parse_index(
            r#"{ "entries": [{
              "version": "0.7.1",
              "minShellVersion": "0.7.0",
              "url": "https://example.invalid/bundle-0.7.1.tar.gz",
              "sha256": "abc",
              "signature": "def",
              "pubDate": "2026-09-14T00:00:00Z",
              "channel": "beta"
            }] }"#,
        )
        .expect("the shape documented in docs/desktop-ota-spec.md should parse");
        let entry = parsed.entries.first().expect("one entry");
        assert_eq!(entry.version, "0.7.1");
        assert_eq!(entry.min_shell_version, "0.7.0");
        assert_eq!(entry.pub_date.as_deref(), Some("2026-09-14T00:00:00Z"));
        assert_eq!(entry.channel, Channel::Beta);
    }

    /// `minShellVersion` carries the only protection against OTA'd JS
    /// calling an IPC command the shell lacks, so an entry without one is
    /// not an entry with a default — it is one that must never be offered.
    ///
    /// It is DROPPED rather than repaired, and the rest of the index still
    /// resolves: every install fetches this one file, so failing the whole
    /// document over one bad entry would take every install offline from a
    /// single bad publish.
    #[test]
    fn an_entry_without_min_shell_version_is_dropped_without_taking_the_index_with_it() {
        let parsed = parse_index(
            r#"{ "entries": [
              {"version":"0.7.1","url":"u","sha256":"a","signature":"b"},
              {"version":"0.7.0","minShellVersion":"0.8.0","url":"u","sha256":"a","signature":"b"}
            ] }"#,
        )
        .expect("a well-formed document with one bad entry still parses");

        assert_eq!(
            parsed.entries.len(),
            1,
            "the entry missing minShellVersion must not survive"
        );
        assert_eq!(
            parsed.entries.first().map(|e| e.version.as_str()),
            Some("0.7.0")
        );
    }

    /// The document itself failing is the publisher's problem and is
    /// reported rather than silently read as an empty index — an empty index
    /// looks exactly like "you are up to date", forever.
    #[test]
    fn a_malformed_index_document_is_an_error_not_an_empty_one() {
        assert!(matches!(
            parse_index("{ not json"),
            Err(UpdateError::Manifest(_))
        ));
    }

    // ── The decision gate ──────────────────────────────────────────

    #[test]
    fn a_bundle_needing_a_newer_shell_is_refused_with_a_reason() {
        let decision = decide(&manifest("0.8.0", "0.8.0"), "0.7.0", None, Intent::Update);
        assert!(
            matches!(decision, UpdateDecision::Refuse(_)),
            "a bundle requiring a newer shell must be refused, got {decision:?}"
        );
        let UpdateDecision::Refuse(reason) = decision else {
            unreachable!("just asserted above")
        };
        // The author has to be able to act on it: both versions, and what to do.
        assert!(reason.contains("0.8.0"), "{reason}");
        assert!(reason.contains("0.7.0"), "{reason}");
        assert!(reason.to_lowercase().contains("full app"), "{reason}");
    }

    /// Refusing is NOT the same as being up to date, and conflating them
    /// would leave the author waiting forever for an update that can never
    /// apply.
    #[test]
    fn a_refusal_is_distinct_from_up_to_date() {
        assert!(matches!(
            decide(&manifest("0.9.0", "0.9.0"), "0.7.0", None, Intent::Update),
            UpdateDecision::Refuse(_)
        ));
        assert_eq!(
            decide(
                &manifest("0.7.1", "0.7.0"),
                "0.7.0",
                Some("0.7.1"),
                Intent::Update
            ),
            UpdateDecision::UpToDate
        );
    }

    #[test]
    fn an_equal_or_older_offer_is_up_to_date() {
        for offered in ["0.7.1", "0.7.0"] {
            assert_eq!(
                decide(
                    &manifest(offered, "0.7.0"),
                    "0.7.0",
                    Some("0.7.1"),
                    Intent::Update
                ),
                UpdateDecision::UpToDate,
                "offered {offered} against installed 0.7.1"
            );
        }
    }

    #[test]
    fn a_newer_offer_installs_over_the_floor_and_over_a_bundle() {
        assert_eq!(
            decide(&manifest("0.7.1", "0.7.0"), "0.7.0", None, Intent::Update),
            UpdateDecision::Install,
            "with the embedded floor active, anything runnable is an install"
        );
        assert_eq!(
            decide(
                &manifest("0.7.2", "0.7.0"),
                "0.7.0",
                Some("0.7.1"),
                Intent::Update
            ),
            UpdateDecision::Install
        );
    }

    /// A shell exactly at `minShellVersion` is allowed — the gate is
    /// "needs NEWER than me", not "needs different from me".
    #[test]
    fn a_shell_exactly_at_the_minimum_is_allowed() {
        assert_eq!(
            decide(&manifest("0.7.1", "0.7.0"), "0.7.0", None, Intent::Update),
            UpdateDecision::Install
        );
    }

    #[test]
    fn unparseable_versions_refuse_rather_than_guess() {
        assert!(matches!(
            decide(
                &manifest("0.7.1", "not-a-version"),
                "0.7.0",
                None,
                Intent::Update
            ),
            UpdateDecision::Refuse(_)
        ));
        assert!(matches!(
            decide(
                &manifest("also-not", "0.7.0"),
                "0.7.0",
                None,
                Intent::Update
            ),
            UpdateDecision::Refuse(_)
        ));
        assert!(matches!(
            decide(
                &manifest("0.7.1", "0.7.0"),
                "nonsense",
                None,
                Intent::Update
            ),
            UpdateDecision::Refuse(_)
        ));
        assert!(
            matches!(
                decide(
                    &manifest("0.7.1", "0.7.0"),
                    "0.7.0",
                    Some("garbage"),
                    Intent::Update
                ),
                UpdateDecision::Refuse(_)
            ),
            "an unrecognisable INSTALLED version must not be silently overwritten"
        );
    }

    // ── Hash ───────────────────────────────────────────────────────

    #[test]
    fn the_hash_check_accepts_the_real_digest_and_nothing_else() {
        let bytes = b"some archive bytes";
        let digest = sha256_hex(bytes);
        assert!(hash_matches(bytes, &digest));
        assert!(
            hash_matches(bytes, &digest.to_uppercase()),
            "hex case is formatting"
        );
        assert!(!hash_matches(bytes, &sha256_hex(b"different bytes")));
        assert!(!hash_matches(bytes, ""));
        assert!(!hash_matches(bytes, "deadbeef"));
    }

    // ── Signature ──────────────────────────────────────────────────

    /// The positive case, against a real Ed25519 signature in minisign's
    /// encoding. Without this the rejection tests below would pass against
    /// a function that rejected everything.
    #[test]
    fn a_real_signature_verifies() {
        assert!(
            signature_is_valid(FIXTURE_PAYLOAD, FIXTURE_SIG_B64, FIXTURE_PUBKEY_B64),
            "a genuine minisign signature over the fixture payload should verify"
        );
    }

    #[test]
    fn a_signature_over_different_bytes_is_rejected() {
        assert!(!signature_is_valid(
            b"tampered payload",
            FIXTURE_SIG_B64,
            FIXTURE_PUBKEY_B64
        ));
    }

    #[test]
    fn malformed_signature_material_is_rejected_rather_than_erroring() {
        for (sig, key) in [
            ("not base64 at all!!", FIXTURE_PUBKEY_B64),
            (FIXTURE_SIG_B64, "not base64 at all!!"),
            ("", FIXTURE_PUBKEY_B64),
            (FIXTURE_SIG_B64, ""),
            // Valid base64 of text that is not a minisign document.
            ("aGVsbG8gd29ybGQ=", FIXTURE_PUBKEY_B64),
            (FIXTURE_SIG_B64, "aGVsbG8gd29ybGQ="),
        ] {
            assert!(
                !signature_is_valid(FIXTURE_PAYLOAD, sig, key),
                "should reject sig={sig:.16?} key={key:.16?}"
            );
        }
    }

    // ── Unpacking ──────────────────────────────────────────────────

    #[test]
    fn a_well_formed_archive_unpacks() {
        let root = TempRoot::new("unpack");
        let archive = targz(&[
            ("index.html", "<!doctype html>"),
            ("assets/app-abc123.js", "export {}"),
        ]);

        unpack_verified(&archive, root.path()).expect("a well-formed archive should unpack");
        assert_eq!(
            std::fs::read_to_string(root.path().join("index.html")).expect("index"),
            "<!doctype html>"
        );
        assert_eq!(
            std::fs::read_to_string(root.path().join("assets/app-abc123.js")).expect("asset"),
            "export {}"
        );
    }

    /// A signature proves WHO built the archive, never that its contents are
    /// well-formed — so every entry is judged on its own even after the
    /// signature passes.
    /// Build a `.tar.gz` holding ONE entry whose name is written straight
    /// into the raw header field.
    ///
    /// `Builder::append_data` refuses a `..` or absolute path itself, so a
    /// hostile archive cannot be produced through the safe API — which is
    /// the point: a real attacker is not using `tar::Builder` either.
    fn targz_raw_name(name: &str, kind: tar::EntryType) -> Vec<u8> {
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_mode(0o644);
        header.set_entry_type(kind);
        let bytes = name.as_bytes();
        assert!(bytes.len() < 100, "fixture name must fit the v7 name field");
        header.as_old_mut().name[..bytes.len()].copy_from_slice(bytes);
        header.set_cksum();

        let mut builder = tar::Builder::new(Vec::new());
        builder
            .append(&header, std::io::empty())
            .expect("append raw-named entry");
        let tar_bytes = builder.into_inner().expect("finish tar");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).expect("gzip");
        encoder.finish().expect("finish gzip")
    }

    #[test]
    fn an_entry_escaping_the_destination_is_refused() {
        for name in ["../escape.txt", "a/../../escape.txt", "/abs/escape.txt"] {
            let root = TempRoot::new("escape");
            let archive = targz_raw_name(name, tar::EntryType::Regular);
            let result = unpack_verified(&archive, root.path());
            assert!(
                matches!(result, Err(UpdateError::UnsafeEntry(_))),
                "{name:?} should be refused, got {result:?}"
            );
        }
    }

    /// A link entry is the one whose target is not the path it declares, and
    /// `tar`'s own `unpack` follows links. Refused outright rather than
    /// sanitised.
    #[test]
    fn a_symlink_entry_is_refused() {
        let root = TempRoot::new("symlink-entry");

        let mut builder = tar::Builder::new(Vec::new());
        let mut header = tar::Header::new_gnu();
        header.set_size(0);
        header.set_entry_type(tar::EntryType::Symlink);
        header
            .set_link_name("/etc/passwd")
            .expect("set link target");
        builder
            .append_data(&mut header, "escape.txt", std::io::empty())
            .expect("append symlink entry");
        let tar_bytes = builder.into_inner().expect("finish tar");
        let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
        encoder.write_all(&tar_bytes).expect("gzip");
        let archive = encoder.finish().expect("finish gzip");

        let result = unpack_verified(&archive, root.path());
        assert!(
            matches!(result, Err(UpdateError::UnsafeEntry(_))),
            "a symlink entry should be refused, got {result:?}"
        );
        assert!(
            !root.path().join("escape.txt").exists(),
            "nothing should have been written"
        );
    }

    // ── The order, which is the whole security property ────────────

    #[test]
    fn a_bad_hash_stops_before_anything_is_written() {
        let root = TempRoot::new("bad-hash");
        let archive = targz(&[("index.html", "<!doctype html>")]);
        let mut m = manifest("0.7.1", "0.7.0");
        m.sha256 = sha256_hex(b"not this archive");
        m.signature = FIXTURE_SIG_B64.to_owned();

        let result = verify_and_unpack(&archive, &m, FIXTURE_PUBKEY_B64, root.path());
        assert!(
            matches!(result, Err(UpdateError::HashMismatch)),
            "{result:?}"
        );
        assert!(
            !root.path().join("index.html").exists(),
            "a rejected archive must not have reached the disk"
        );
    }

    /// The case the ordering exists for: the bytes are intact (hash passes)
    /// but they are not ours. Nothing may be extracted.
    #[test]
    fn a_bad_signature_stops_before_anything_is_written() {
        let root = TempRoot::new("bad-sig");
        let archive = targz(&[("index.html", "<!doctype html>")]);
        let mut m = manifest("0.7.1", "0.7.0");
        m.sha256 = sha256_hex(&archive);
        m.signature = FIXTURE_SIG_B64.to_owned(); // valid document, wrong payload

        let result = verify_and_unpack(&archive, &m, FIXTURE_PUBKEY_B64, root.path());
        assert!(
            matches!(result, Err(UpdateError::BadSignature(_))),
            "an archive the key did not sign must be refused: {result:?}"
        );
        assert!(
            !root.path().join("index.html").exists(),
            "a signature failure must stop BEFORE extraction — a hostile archive that \
             reached staging/ is one rename away from being served"
        );
    }
}
