//! Validation that all `DiagnosticCode` variants have corresponding documentation files
//! and that diagnostic codes are unique (no collision between variants).
//!
//! This test ensures that every error code defined in `brink-ir`'s `DiagnosticCode` enum
//! has a corresponding explanation file in `docs/diagnostics/Exxx.md`, and conversely that
//! every file in `docs/diagnostics/` corresponds to a real `DiagnosticCode` variant.
//!
//! It also validates that every `DiagnosticCode` variant maps to exactly one code string
//! via the `as_str()` method — a uniqueness check that prevents the collision hazard where
//! two concurrent build agents allocate the same code independently. This check is built
//! from [`DiagnosticCode::ALL`], the exhaustive variant list, rather than probed through
//! [`DiagnosticCode::from_str_code`] — `from_str_code` is `str -> Option<Self>`, so probing
//! it can only ever discover a many-to-one mapping in the wrong direction (many code
//! strings to one variant, which is not the hazard we care about); it can never surface
//! two variants returning the *same* code string from `as_str()`, which is.
//!
//! The known-code list used for the doc-file checks is derived from the enum itself (via
//! [`DiagnosticCode::from_str_code`]) rather than hand-copied, so a newly added variant is
//! picked up automatically instead of silently passing a stale list.
//!
//! If this test fails when you add a new diagnostic code, you must:
//! - Add the new variant to [`DiagnosticCode::ALL`],
//! - Add a corresponding `docs/diagnostics/Exxx.md` file,
//! - Ensure no other variant returns the same code string from `as_str()`, and
//! - Not skip a number or delete a variant from the middle of the range — codes are
//!   never reused once assigned (retire in place with a `RETIRED` doc comment instead),
//!   so the contiguity check enforces that the numeric sequence has no gaps.

use brink_ir::DiagnosticCode;
use std::collections::BTreeMap;
use std::path::Path;

/// Every diagnostic code string that currently resolves to a real `DiagnosticCode`
/// variant, discovered by probing `E001..=E999` through `DiagnosticCode::from_str_code`.
fn all_diagnostic_code_strings() -> Vec<String> {
    (1..=999)
        .map(|n| format!("E{n:03}"))
        .filter(|code_str| DiagnosticCode::from_str_code(code_str).is_some())
        .collect()
}

#[test]
fn all_diagnostic_codes_have_documentation() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("Could not find workspace root");
    let docs_dir = workspace_root.join("docs/diagnostics");

    assert!(docs_dir.exists(), "docs/diagnostics directory must exist");

    let known_codes = all_diagnostic_code_strings();
    assert!(
        !known_codes.is_empty(),
        "expected at least one DiagnosticCode variant to be discovered"
    );

    let mut missing_docs = Vec::new();
    for code_str in &known_codes {
        let doc_file = docs_dir.join(format!("{code_str}.md"));
        if !doc_file.exists() {
            missing_docs.push(code_str.clone());
        }
    }

    assert!(
        missing_docs.is_empty(),
        "The following diagnostic codes are missing documentation files:\n  {}",
        missing_docs.join("\n  ")
    );

    // Reverse check: every doc file must correspond to a real DiagnosticCode variant,
    // so an orphaned or misnamed file (e.g. a doc for a retired/nonexistent code) is
    // caught instead of silently accumulating.
    let mut orphaned_docs = Vec::new();
    let entries = std::fs::read_dir(&docs_dir).expect("failed to read docs/diagnostics");
    for entry in entries {
        let entry = entry.expect("failed to read docs/diagnostics entry");
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_string();
        if !known_codes.contains(&stem) {
            orphaned_docs.push(stem);
        }
    }

    assert!(
        orphaned_docs.is_empty(),
        "The following docs/diagnostics files do not correspond to a real DiagnosticCode:\n  {}",
        orphaned_docs.join("\n  ")
    );
}

#[test]
fn diagnostic_codes_are_unique() {
    // Enumerate the exhaustive variant list and build the inverse map: code string ->
    // every variant whose `as_str()` produces it. Unlike probing `from_str_code` over
    // `E001..E999` (which is `str -> Option<Self>` and can never surface two variants
    // colliding on the same string), this walks `Self -> str` directly, so a genuine
    // collision produces a BTreeMap key with more than one entry.
    let mut code_to_variants: BTreeMap<&'static str, Vec<DiagnosticCode>> = BTreeMap::new();
    for &variant in DiagnosticCode::ALL {
        code_to_variants
            .entry(variant.as_str())
            .or_default()
            .push(variant);
    }

    // Round-trip check: as_str() and from_str_code() must stay synchronized, or the
    // variant becomes unreachable via its own code string.
    for &variant in DiagnosticCode::ALL {
        let code_str = variant.as_str();
        assert_eq!(
            DiagnosticCode::from_str_code(code_str),
            Some(variant),
            "DiagnosticCode variant {variant:?} did not round-trip: \
             from_str_code({code_str}) did not produce {variant:?} back. \
             This indicates inconsistency between as_str() and from_str_code()."
        );
    }

    // ALL must agree with the from_str_code-derived known-code list in size, or ALL is
    // stale (a variant was added to the enum but not to ALL).
    let known_codes = all_diagnostic_code_strings();
    assert_eq!(
        DiagnosticCode::ALL.len(),
        known_codes.len(),
        "DiagnosticCode::ALL has {} entries but from_str_code recognizes {} code strings — \
         ALL is out of sync with the enum. Add the missing variant(s) to ALL.",
        DiagnosticCode::ALL.len(),
        known_codes.len()
    );

    // Now verify no code string maps to multiple variants.
    let mut collisions = Vec::new();
    for (code_str, variants) in &code_to_variants {
        if variants.len() > 1 {
            collisions.push(format!(
                "{code_str} is returned by multiple variants: {variants:?}"
            ));
        }
    }

    assert!(
        collisions.is_empty(),
        "The following diagnostic codes are duplicated (multiple variants return the same code):\n  {}",
        collisions.join("\n  ")
    );

    // Verify no gaps in the numeric sequence: if E001 exists and E167 exists,
    // every code E001..=E167 must also exist. (Gaps are allowed *before* E001
    // or *after* the max, but not in the middle.) Codes are never reused once
    // assigned — retire a code in place with a `RETIRED` doc comment on its variant
    // rather than deleting it from the enum, or this check will fail.
    if let (Some(first_code), Some(last_code)) = (
        code_to_variants.keys().next(),
        code_to_variants.keys().last(),
    ) {
        assert!(
            first_code[1..].chars().all(|c| c.is_ascii_digit()),
            "diagnostic code {first_code} has a non-numeric suffix"
        );
        assert!(
            last_code[1..].chars().all(|c| c.is_ascii_digit()),
            "diagnostic code {last_code} has a non-numeric suffix"
        );
        let first_num: u32 = first_code[1..]
            .parse()
            .expect("already validated as all-ASCII-digit above");
        let last_num: u32 = last_code[1..]
            .parse()
            .expect("already validated as all-ASCII-digit above");

        let mut gaps = Vec::new();
        for num in first_num..=last_num {
            let code_str = format!("E{num:03}");
            if !code_to_variants.contains_key(code_str.as_str()) {
                gaps.push(code_str);
            }
        }

        assert!(
            gaps.is_empty(),
            "Gaps found in the diagnostic code range E{:03}..=E{:03}:\n  {}",
            first_num,
            last_num,
            gaps.join("\n  ")
        );
    }
}

/// Diagnostic codes whose defining mechanism is reachable **only** from the
/// native `.brink` surface — never from ink source, under any dialect. The
/// fence's info string names the surface a sample must be written in, and no
/// existing gate checks that here: `docs/diagnostics/**/*.md` is not part of
/// the mdBook book (it is absent from `docs/book/src/SUMMARY.md`), and
/// `book_fences.rs` (BW-5) only walks `docs/book/src/**/*.md` — per its own
/// taxonomy table, an ```` ```ink ```` fence there must compile and an
/// ```` ```brink ```` fence is skipped as "unruled future syntax", but
/// neither rule ever reaches this directory. This test is the only check
/// over these docs' fence content. An ```` ```ink ```` fence on a
/// native-only code's doc is still wrong on its own terms: it claims a
/// surface the diagnostic can never actually be reached from, so once #1623
/// fills the placeholder in with a real example, a stray `ink` tag would
/// make the illustration lie about which surface it demonstrates.
///
/// This list is re-derived from the enum's own doc comments and
/// `docs/directive-annotations-spec.md` §5c/§5d directly — **not** carried
/// over from issue #1836's filing-time list, which named nine codes
/// (`E132`, `E153`–`E155`, `E159`–`E163`) and predates `E130`, `E145`,
/// `E146`, `E156`, `E158`, and `E164`–`E172`, all of which landed
/// native-only afterward:
///
/// - `E130` — a native `flow` nested more than two levels deep; raised only
///   from `hir::lower_native::container`, whose `title()` reads "native:
///   `flow` nested more than two levels deep is not yet supported" — `flow`
///   is a native-surface container with no ink counterpart.
/// - `E132` — the native file-level `@[was("…")]` rename record; ink's own
///   `@[…]` channel recognizes only `effects` (§5b).
/// - `E145`/`E146` — the `as` binding channel (`if EXPR as name { … }` and
///   the choice-guard form respectively); `AS_BINDING` is a
///   `brink-syntax-native` grammar node with no counterpart in
///   `brink-syntax`, and both are raised only from
///   `hir::lower_native::control_flow` / `hir::lower_native::choice`.
/// - `E153`/`E154`/`E155` — the `@[allow(…)]` suppression channel: §5d says
///   "Native surface only" in as many words.
/// - `E156` — a lambda body assigning to a captured binding; `LAMBDA_EXPR`
///   exists only in `brink-syntax-native` (zero occurrences anywhere in
///   `brink-syntax`), and the check is raised only from
///   `hir::lower_native::lambda::check_capture_writes`.
/// - `E158` — a lambda recursing through its own not-yet-bound name, from
///   the same native-only lambda-lifting pass as `E156`.
/// - `E159`/`E160` — `@[element(…)]`'s declaration-surface checks; `element`
///   is one of the native-only recognized names §5c adds beyond `effects`.
/// - `E161`/`E162`/`E163` — `@[style(…)]`, the same native-only channel as
///   `element`.
/// - `E164`/`E165` — the inline-markup vocabulary checks; wired outside the
///   ink/brink dialect branch in `brink_analyzer::lib::per_file_diagnostics`
///   with a doc comment saying markup spans are "a native-grammar
///   construct" and the pass is "inert for ink source by construction (no
///   `ContentPart::Span` can exist there)".
/// - `E166`–`E171` — the `@[element(claims = "…")]` / `@[element(…,
///   block)]` dispatch family, the same native-only annotation channel as
///   `E159`/`E160`.
/// - `E172` — raised only by `hir::lower_native::body::lower_tag`, i.e. only
///   while lowering a `.brink` file.
/// - `E173` — the required-markup-attribute check (issue #1780/#1997), the
///   same native-only markup channel as `E164`/`E165`; raised from the same
///   `brink_analyzer::markup_check::SpanWalker`.
/// - `E174` — a lambda's own written param/return annotation disagreeing
///   with its body-derived type (issue #1994); raised only from
///   `infer::body::InferPass::infer_lambda`, and `LAMBDA_EXPR` exists only
///   in `brink-syntax-native`, same posture as `E156`/`E158`.
///
/// Codes intentionally **excluded** despite living in the same numeric
/// neighborhood: `E157` (the unnamed-once-only-choice / unnamed-sequence
/// visit-count lint) is a *dialect* concern, not a *surface* one — its
/// knot/`*`-choice example is genuinely reachable from ink source, which is
/// exactly what `E157.md`'s ```` ```ink ```` fences correctly demonstrate.
const NATIVE_ONLY_CODES: &[&str] = &[
    "E130", "E132", "E145", "E146", "E153", "E154", "E155", "E156", "E158", "E159", "E160", "E161",
    "E162", "E163", "E164", "E165", "E166", "E167", "E168", "E169", "E170", "E171", "E172", "E173",
    "E174",
];

/// Every fenced code block's info string (the text right after the opening
/// ` ``` `), in document order. Deliberately simpler than `book_fences.rs`'s
/// extractor (no indent handling, no execution markers, no body capture) —
/// this check only ever needs the tag, never the contents.
fn fence_info_strings(markdown: &str) -> Vec<String> {
    let mut infos = Vec::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if let Some(info) = trimmed.strip_prefix("```") {
            if in_fence {
                in_fence = false;
            } else {
                infos.push(info.trim().to_owned());
                in_fence = true;
            }
        }
    }
    infos
}

#[test]
fn native_only_diagnostics_never_use_ink_fences() {
    let manifest_dir = env!("CARGO_MANIFEST_DIR");
    let workspace_root = Path::new(manifest_dir)
        .parent()
        .and_then(|p| p.parent())
        .and_then(|p| p.parent())
        .expect("Could not find workspace root");
    let docs_dir = workspace_root.join("docs/diagnostics");

    let mut violations = Vec::new();
    for code in NATIVE_ONLY_CODES {
        assert!(
            DiagnosticCode::from_str_code(code).is_some(),
            "{code} in NATIVE_ONLY_CODES is not a real DiagnosticCode — fix the typo"
        );

        let doc_file = docs_dir.join(format!("{code}.md"));
        let read_failure_msg = format!("failed to read {}", doc_file.display());
        let markdown = std::fs::read_to_string(&doc_file).expect(&read_failure_msg);
        for info in fence_info_strings(&markdown) {
            if info == "ink" || info.starts_with("ink,") {
                violations.push(format!(
                    "{code}.md has a `{info}` fence, but {code} is native-only \
                     (see NATIVE_ONLY_CODES's doc comment for why) — use `brink` instead"
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "The following docs/diagnostics files use the wrong fence dialect:\n  {}",
        violations.join("\n  ")
    );
}
