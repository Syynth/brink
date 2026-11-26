//! Parse-clean gate for the hand-respelled `.brink` fixture corpus
//! (`docs/b0-findings.md` NF-5, "the differential method" — recommendation
//! (c), with (b) as the flagship exit gate for B0.7/B0.8).
//!
//! These fixtures live under `tests/tier1-brink-respell/<case>/story.brink`
//! at the repo root (see that directory's `README.md` for what they are and
//! why). This crate is B0.5's grammar skeleton: no HIR lowering exists yet,
//! so **parsing with zero errors is the only claim this test makes** — it
//! is not an episode-identity check (that's B0.7+, per NF-5's rider).
//!
//! Every fixture is walked generically (rather than one `#[test]` per case)
//! so a new fixture directory is picked up automatically without editing
//! this file — the loop still reports which specific fixture failed via
//! the panic message.
//!
//! Helper functions here return `Result` rather than unwrapping/panicking
//! directly (workspace lint: `unwrap_used`/`expect_used`/`panic` are denied;
//! `clippy.toml`'s `allow-unwrap-in-tests`/`allow-expect-in-tests` only
//! exempt code inside `#[test]`-attributed fn bodies, not plain helpers a
//! test happens to call).

use std::fs;
use std::path::{Path, PathBuf};

use brink_syntax_native::{SyntaxKind, parse};

/// `tests/tier1-brink-respell` at the repo root, resolved relative to this
/// crate's manifest dir (`crates/internal/brink-syntax-native`) rather than
/// the process cwd, so `cargo test` works regardless of invocation
/// directory.
fn fixtures_root() -> Result<PathBuf, String> {
    let candidate =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../tests/tier1-brink-respell");
    candidate
        .canonicalize()
        .map_err(|e| format!("tests/tier1-brink-respell must exist at the repo root ({e})"))
}

/// Every `story.brink` found directly under a fixture-root subdirectory,
/// sorted for deterministic test output (the repo's own determinism rule —
/// never depend on directory-read order).
fn fixture_files() -> Result<Vec<PathBuf>, String> {
    let root = fixtures_root()?;
    let entries = fs::read_dir(&root).map_err(|e| format!("reading {}: {e}", root.display()))?;
    let mut files: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|p| p.is_dir())
        .map(|dir| dir.join("story.brink"))
        .filter(|p| p.is_file())
        .collect();
    files.sort();
    Ok(files)
}

#[test]
fn every_respelled_fixture_parses_with_zero_errors() {
    let files = fixture_files().unwrap();
    assert!(
        !files.is_empty(),
        "expected at least one story.brink fixture under {}",
        fixtures_root().unwrap().display()
    );

    let mut failures = Vec::new();
    for path in &files {
        let source = fs::read_to_string(path).unwrap();
        let parsed = parse(&source);

        // Lossless round-trip is the parser's baseline invariant (mirrors
        // `parser::tests::assert_lossless`) — check it here too, since a
        // fixture that silently dropped text would be a much worse bug
        // than a reported parse error.
        if parsed.syntax().text().to_string() != source {
            failures.push(format!(
                "{}: lossy round-trip (CST text != source)",
                path.display()
            ));
            continue;
        }

        if !parsed.errors().is_empty() {
            failures.push(format!(
                "{}: {} parse error(s): {:?}",
                path.display(),
                parsed.errors().len(),
                parsed.errors()
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "the following respelled fixtures did not parse clean:\n{}",
        failures.join("\n")
    );
}

/// Every fixture directory must carry its provenance manifest (ink source +
/// oracle case pointer) — a `story.brink` with no `manifest.toml` breaks the
/// pairing this corpus exists to hold (NF-5's rider: "respelled cases live
/// beside their ink twins in-tree so drift is reviewable").
#[test]
fn every_fixture_has_a_manifest() {
    for path in fixture_files().unwrap() {
        let manifest = path.with_file_name("manifest.toml");
        assert!(
            manifest.is_file(),
            "{} has no sibling manifest.toml",
            path.display()
        );
    }
}

/// N-1's real proof (issue #1203): the README's N-1 finding names three
/// fixtures whose inline `* [text] -> target` / same-line-in-body diverts
/// used to parse clean but fold into literal `TEXT` — `sticky-choice`,
/// `exhibit-fogg-passage`, and `manual-stitch-v1`. This asserts each one's
/// CST now contains a real `DIVERT_STMT` or `TUNNEL_CALL` node at the
/// inline-divert position, not just "zero parse errors" (which was already
/// true before the fix and is exactly what let the bug hide). The expected
/// counts below were hand-verified against each `story.brink`'s inline
/// (same-line-as-prose) diverts:
///
/// - `sticky-choice`: one, `You eat another donut. -> homers_couch` inside
///   the `+` choice's braced body (`* [Get off the couch] { ... -> END }`'s
///   divert sits on its own line and was already recognized before N-1).
/// - `exhibit-fogg-passage`: two, `* [The wager.] -> know_about_wager` and
///   `* [I was surprised.] -> i_stared`.
/// - `manual-stitch-v1`: three, `* [In first class] -> in_first_class`,
///   `* [I'll go cheap] -> the_orient_express.in_third_class` (also
///   exercises a dotted divert-target path in content position), and the
///   nested stitch's own `* [Move to third class] -> in_third_class`.
#[test]
fn n1_affected_fixtures_parse_inline_diverts_as_divert_nodes() {
    let cases: &[(&str, usize)] = &[
        ("sticky-choice", 1),
        ("exhibit-fogg-passage", 2),
        ("manual-stitch-v1", 3),
    ];

    let root = fixtures_root().unwrap();
    let mut failures = Vec::new();
    for &(case, expected_inline_diverts) in cases {
        let path = root.join(case).join("story.brink");
        let source = fs::read_to_string(&path).unwrap();
        let parsed = parse(&source);

        assert!(
            parsed.errors().is_empty(),
            "{}: expected zero parse errors, got {:?}",
            path.display(),
            parsed.errors()
        );

        // A "real divert node" here means a DIVERT_STMT/TUNNEL_CALL whose
        // own text does NOT start the line it's on — i.e. it followed some
        // other content on the same source line, the exact N-1 shape.
        // Standalone-line diverts (already handled pre-fix) are excluded
        // from the count so this test can't pass by accident on a fixture
        // that has plenty of ordinary statement-position diverts but none
        // of the inline kind.
        let inline_divert_count = parsed
            .syntax()
            .descendants()
            .filter(|n| matches!(n.kind(), SyntaxKind::DIVERT_STMT | SyntaxKind::TUNNEL_CALL))
            .filter(|n| !is_first_on_its_line(n))
            .count();

        if inline_divert_count != expected_inline_diverts {
            failures.push(format!(
                "{}: expected {expected_inline_diverts} inline DIVERT_STMT/TUNNEL_CALL node(s), found {inline_divert_count}",
                path.display()
            ));
        }
    }

    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `true` if `node`'s first token is also the first non-trivia token on
/// its source line (nothing but whitespace precedes it since the last
/// `\n`) — i.e. this is a statement-position construct, not one reached
/// mid content-run.
fn is_first_on_its_line(node: &brink_syntax_native::SyntaxNode) -> bool {
    let Some(start) = node
        .descendants_with_tokens()
        .find_map(rowan::NodeOrToken::into_token)
    else {
        return false;
    };
    let mut prev = start.prev_token();
    while let Some(tok) = prev {
        match tok.kind() {
            SyntaxKind::WHITESPACE => prev = tok.prev_token(),
            SyntaxKind::NEWLINE => return true,
            _ => return false,
        }
    }
    // No previous token at all — this is the very first token in the file.
    true
}
