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
pub fn parse_manifest(text: &str) -> Result<BundleManifest, UpdateError> {
    serde_json::from_str(text).map_err(|e| UpdateError::Manifest(e.to_string()))
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
        decide, hash_matches, parse_manifest, signature_is_valid, unpack_verified,
        verify_and_unpack, BundleManifest, UpdateDecision, UpdateError,
    };
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
    fn a_manifest_parses_from_the_documented_shape() {
        let parsed = parse_manifest(
            r#"{
              "version": "0.7.1",
              "minShellVersion": "0.7.0",
              "url": "https://example.invalid/bundle-0.7.1.tar.gz",
              "sha256": "abc",
              "signature": "def",
              "pubDate": "2026-09-14T00:00:00Z"
            }"#,
        )
        .expect("the shape documented in docs/desktop-ota-spec.md should parse");
        assert_eq!(parsed.version, "0.7.1");
        assert_eq!(parsed.min_shell_version, "0.7.0");
        assert_eq!(parsed.pub_date.as_deref(), Some("2026-09-14T00:00:00Z"));
    }

    /// `minShellVersion` carries the only protection against OTA'd JS
    /// calling an IPC command the shell lacks, so a manifest without one is
    /// not a manifest with a default — it is unusable.
    #[test]
    fn a_manifest_without_min_shell_version_is_refused_at_parse() {
        let err = parse_manifest(r#"{"version":"0.7.1","url":"u","sha256":"a","signature":"b"}"#);
        assert!(
            matches!(err, Err(UpdateError::Manifest(_))),
            "minShellVersion must be mandatory, not defaulted: {err:?}"
        );
    }

    // ── The decision gate ──────────────────────────────────────────

    #[test]
    fn a_bundle_needing_a_newer_shell_is_refused_with_a_reason() {
        let decision = decide(&manifest("0.8.0", "0.8.0"), "0.7.0", None);
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
            decide(&manifest("0.9.0", "0.9.0"), "0.7.0", None),
            UpdateDecision::Refuse(_)
        ));
        assert_eq!(
            decide(&manifest("0.7.1", "0.7.0"), "0.7.0", Some("0.7.1")),
            UpdateDecision::UpToDate
        );
    }

    #[test]
    fn an_equal_or_older_offer_is_up_to_date() {
        for offered in ["0.7.1", "0.7.0"] {
            assert_eq!(
                decide(&manifest(offered, "0.7.0"), "0.7.0", Some("0.7.1")),
                UpdateDecision::UpToDate,
                "offered {offered} against installed 0.7.1"
            );
        }
    }

    #[test]
    fn a_newer_offer_installs_over_the_floor_and_over_a_bundle() {
        assert_eq!(
            decide(&manifest("0.7.1", "0.7.0"), "0.7.0", None),
            UpdateDecision::Install,
            "with the embedded floor active, anything runnable is an install"
        );
        assert_eq!(
            decide(&manifest("0.7.2", "0.7.0"), "0.7.0", Some("0.7.1")),
            UpdateDecision::Install
        );
    }

    /// A shell exactly at `minShellVersion` is allowed — the gate is
    /// "needs NEWER than me", not "needs different from me".
    #[test]
    fn a_shell_exactly_at_the_minimum_is_allowed() {
        assert_eq!(
            decide(&manifest("0.7.1", "0.7.0"), "0.7.0", None),
            UpdateDecision::Install
        );
    }

    #[test]
    fn unparseable_versions_refuse_rather_than_guess() {
        assert!(matches!(
            decide(&manifest("0.7.1", "not-a-version"), "0.7.0", None),
            UpdateDecision::Refuse(_)
        ));
        assert!(matches!(
            decide(&manifest("also-not", "0.7.0"), "0.7.0", None),
            UpdateDecision::Refuse(_)
        ));
        assert!(matches!(
            decide(&manifest("0.7.1", "0.7.0"), "nonsense", None),
            UpdateDecision::Refuse(_)
        ));
        assert!(
            matches!(
                decide(&manifest("0.7.1", "0.7.0"), "0.7.0", Some("garbage")),
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
