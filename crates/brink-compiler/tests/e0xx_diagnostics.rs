//! E0xx pipeline-level coverage audit (#672, lane A) + hygiene follow-up (#709).
//!
//! One direct, minimal fixture per `DiagnosticCode`, proven to fire through
//! the real pipeline (`brink_compiler::compile_with_options` — parse → HIR
//! lower → analyze → LIR lower → codegen), never through a unit call on the
//! emitting function. Each test asserts the code fires *at the expected
//! span*: the diagnostic's range must start inside the offending construct.
//!
//! Codes deliberately absent here (covered by other pipeline-level suites,
//! kept in one place each per the one-fix-one-commit discipline):
//!
//! - E012, E029, E030, E033 — `tests/driver.rs`
//! - E031, E035, E054, E055, E056, E075, E076, E077, E078 —
//!   `brink-test-harness/tests/tier1_brink.rs`
//! - E051 — `tests/t1b_dialect_gate.rs`
//! - E063, E064, E065, E067 — `tests/tm3_strict_policy.rs`. E066 is *also*
//!   covered there (the general Conflicted-escape case) — the two
//!   `or`-coalescing fixtures near the bottom of this file are a deliberate
//!   exception, native-only (`InfixOp::Coalesce`) and disk-based (their own
//!   `compile_native` helper), kept here per the review finding that added
//!   them (PR #1469/#1460) rather than growing a second disk-based harness
//!   in `tm3_strict_policy.rs` for one code.
//!
//! Codes retired as unreachable (lane-A audit findings + hygiene follow-up
//! #709; see enum docs for rationale):
//!
//! - E011 — RETIRED — the parser always materializes a `FILE_PATH` node
//!   inside `INCLUDE_STMT`; reports E037 on error. Code moved to
//!   `include.rs::lower_include` as an unreachable!(…) branch.
//! - E013 — RETIRED — `parser/divert.rs::path` always creates a `PATH` node;
//!   `ThreadStart::target()` never returns None. Code moved to
//!   `divert.rs::lower_divert` as an unreachable!(…) branch.
//! - E018 — RETIRED — `parser/divert.rs::path` always creates a `PATH` node;
//!   `DivertTargetExpr::target()` never returns None. Code moved to
//!   `expr/references.rs::DivertTargetExpr` as an unreachable!(…) branch.
//! - E019 — RETIRED — the parser only builds a `CHOICE` node after a bullet
//!   token; bullet-less choices cannot exist in the CST. Code moved to
//!   `choice.rs::lower_choice` as an unreachable!(…) branch.
//! - E028 — RETIRED — circular INCLUDE is detected at discovery and surfaces
//!   as `CompileError::CircularInclude`, not a per-construct diagnostic.
//! - E052 — RETIRED (T1c-2, #700) — `#fn(…)` now lowers for real; former
//!   lowering fence is obsolete. Reserved, not reused.
//! - E053 — RETIRED (T1b-2, #570) — T1b brink-extension HIR nodes now lower
//!   for real; former LIR backstop is obsolete. Reserved, not reused.
//! - E060 — emitted when codegen rejects a `Program` violating an invariant
//!   an earlier stage is supposed to guarantee (a compiler bug by
//!   definition). Most `brink_codegen_inkb::CodegenError`s (e.g. #586's
//!   out-of-loop `break`/`continue` backstop) are genuinely not
//!   constructible from source — LIR lowering rejects those shapes first,
//!   non-suppressibly. But #1673's duplicate-`DefinitionId` guard *is*
//!   reachable from source today: the #1504 collision (two files with
//!   root-level weave content) trips it through the ordinary
//!   `brink_compiler::compile` entry point. Covered in
//!   `issue_1504_root_content_identity.rs`
//!   (`included_and_entry_root_weaves_trip_the_duplicate_definition_id_guard`)
//!   rather than duplicated here, since that file already carries the rest
//!   of the #1504 shape's fixture and end-to-end context.
//! - E072 — RETIRED (TM-4c, #666), documented as reserved-not-reused.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::{BTreeMap, HashMap};

use brink_analyzer::{LintLevel, LintPolicy};
use brink_compiler::{AnalysisOptions, DiagnosticCode, Dialect, TypePolicy};
use brink_ir::host_manifest::{
    BaseType, Constraint, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef, TypeRef,
};

// ─── Harness ─────────────────────────────────────────────────────────

fn compile(
    source: &str,
    options: AnalysisOptions,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let files: HashMap<&str, &str> = HashMap::from([("main.ink", source)]);
    brink_compiler::compile_with_options(
        "main.ink",
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
}

fn compile_multifile(
    entry: &str,
    files: &HashMap<&str, &str>,
    options: AnalysisOptions,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    brink_compiler::compile_with_options(
        entry,
        |path| {
            files.get(path).map(|s| (*s).to_string()).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("file not found: {path}"),
                )
            })
        },
        options,
    )
}

fn default_options() -> AnalysisOptions {
    AnalysisOptions::default()
}

fn brink_options() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        ..AnalysisOptions::default()
    }
}

/// The explicit `types = gradual` opt-out knob (#1127, ruled 2026-07-19:
/// the brink dialect's implicit default is now strict) — for fixtures that
/// TEST deliberately-dynamic behavior the strict default would reject.
fn gradual_options() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Gradual),
        ..AnalysisOptions::default()
    }
}

fn strict_options() -> AnalysisOptions {
    AnalysisOptions {
        dialect: Dialect::Brink,
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    }
}

fn errors_of(err: brink_compiler::CompileError) -> Vec<brink_compiler::ResolvedDiagnostic> {
    match err {
        brink_compiler::CompileError::Diagnostics(diags) => diags,
        other => panic!("expected Diagnostics error, got {other:?}"),
    }
}

/// Byte offset of the `n`-th (0-based) occurrence of `needle` in `source`.
fn find_nth(source: &str, needle: &str, n: usize) -> usize {
    let mut from = 0;
    for _ in 0..n {
        let hit = source[from..]
            .find(needle)
            .unwrap_or_else(|| panic!("occurrence of {needle:?} not found"));
        from += hit + needle.len();
    }
    from + source[from..]
        .find(needle)
        .unwrap_or_else(|| panic!("occurrence of {needle:?} not found"))
}

/// Assert some diagnostic with `code` starts inside the `n`-th occurrence of
/// `needle` (inclusive of its end offset — several parser-recovery
/// diagnostics point at the position immediately after the construct).
fn assert_code_at_nth(
    diags: &[brink_compiler::ResolvedDiagnostic],
    code: DiagnosticCode,
    source: &str,
    needle: &str,
    n: usize,
) {
    let pos = find_nth(source, needle, n);
    let end = pos + needle.len();
    assert!(
        diags.iter().any(|d| {
            let start = usize::from(d.range.start());
            d.code == code && pos <= start && start <= end
        }),
        "expected {} starting inside {needle:?} (bytes {pos}..{end}), got: {:?}",
        code.as_str(),
        diags
            .iter()
            .map(|d| format!("{}@{:?}", d.code.as_str(), d.range))
            .collect::<Vec<_>>()
    );
}

/// Compile expecting failure; assert `code` fires at `needle`'s span.
fn assert_error_at(source: &str, options: AnalysisOptions, code: DiagnosticCode, needle: &str) {
    let err = compile(source, options)
        .map(|_| ())
        .expect_err(&format!("expected {} to fail the compile", code.as_str()));
    let diags = errors_of(err);
    assert_code_at_nth(&diags, code, source, needle, 0);
}

/// Compile expecting success; assert `code` fires as a warning at `needle`.
fn assert_warning_at(source: &str, options: AnalysisOptions, code: DiagnosticCode, needle: &str) {
    let out = compile(source, options).unwrap_or_else(|e| {
        panic!(
            "{} is warning-severity — compile must succeed, got {e:?}",
            code.as_str()
        )
    });
    assert_code_at_nth(&out.warnings, code, source, needle, 0);
}

// ─── Containers (E001–E003) ──────────────────────────────────────────

#[test]
fn e001_knot_missing_name() {
    assert_error_at(
        "== ==\nHello\n",
        default_options(),
        DiagnosticCode::E001,
        "== ==",
    );
}

#[test]
fn e002_stitch_missing_name() {
    let source = "== k ==\n= \nHi\n-> END\n";
    assert_error_at(source, default_options(), DiagnosticCode::E002, "= \n");
}

#[test]
fn e003_parameter_missing_name() {
    // A literal where a parameter name must be.
    assert_error_at(
        "== k(3) ==\nHi\n",
        default_options(),
        DiagnosticCode::E003,
        "(3)",
    );
}

// ─── Declarations (E004–E010) ────────────────────────────────────────

#[test]
fn e004_var_missing_name() {
    assert_error_at(
        "VAR = 1\nHi\n",
        default_options(),
        DiagnosticCode::E004,
        "VAR = 1",
    );
}

#[test]
fn e005_var_missing_initializer() {
    assert_error_at(
        "VAR x\nHi\n",
        default_options(),
        DiagnosticCode::E005,
        "VAR x",
    );
}

#[test]
fn e006_const_missing_name() {
    assert_error_at(
        "CONST = 1\nHi\n",
        default_options(),
        DiagnosticCode::E006,
        "CONST = 1",
    );
}

#[test]
fn e007_const_missing_initializer() {
    assert_error_at(
        "CONST c =\nHi\n",
        default_options(),
        DiagnosticCode::E007,
        "CONST c =",
    );
}

#[test]
fn e008_list_missing_name() {
    assert_error_at(
        "LIST = a, b\nHi\n",
        default_options(),
        DiagnosticCode::E008,
        "LIST = a, b",
    );
}

#[test]
fn e009_list_member_missing_name() {
    // `()` where a member name must be.
    assert_error_at(
        "LIST l = a, (), b\nHi\n",
        default_options(),
        DiagnosticCode::E009,
        "()",
    );
}

#[test]
fn e010_external_missing_name() {
    assert_error_at(
        "EXTERNAL (x)\nHi\n",
        default_options(),
        DiagnosticCode::E010,
        "EXTERNAL (x)",
    );
}

// ─── Control flow / logic lines (E014) ───────────────────────────────

#[test]
fn e014_bare_tilde_logic_line_warns() {
    assert_warning_at("~\nHi\n", default_options(), DiagnosticCode::E014, "~");
}

/// #1162: a `[lints] E014 = "hint"` policy must reach `CompileOutput::
/// warnings` with `ResolvedDiagnostic::severity == Severity::Hint` — the CLI
/// renderer's actual input, not just `brink_analyzer::effective_severity`
/// called in isolation. Must stay non-blocking exactly like the `Warning`
/// it's demoted from (`compile` still returns `Ok`).
#[test]
fn e014_lints_hint_override_reaches_resolved_diagnostic_severity() {
    let mut options = default_options();
    options
        .lints
        .overrides
        .insert("E014".to_owned(), LintLevel::Hint);

    let out = compile("~\nHi\n", options).unwrap_or_else(|e| {
        panic!("[lints] E014 = \"hint\" must stay non-blocking, compile failed: {e:?}")
    });
    let e014 = out
        .warnings
        .iter()
        .find(|d| d.code == DiagnosticCode::E014)
        .unwrap_or_else(|| panic!("expected an E014 diagnostic, got: {:?}", out.warnings));
    assert_eq!(
        e014.severity,
        brink_ir::Severity::Hint,
        "ResolvedDiagnostic::severity must carry the [lints]-resolved Hint tier, not E014's raw \
         Warning default"
    );
}

// ─── Expressions (E015–E017, E020, E021) ─────────────────────────────

#[test]
fn e015_expression_missing_operand() {
    assert_error_at(
        "~ temp x = 1 +\nHi\n",
        default_options(),
        DiagnosticCode::E015,
        "1 +",
    );
}

#[test]
fn e016_unsupported_operator() {
    // The parser accepts `+=`/`-=` as infix operators (Pratt table,
    // `Prec::Assign`) but HIR lowering has no `InfixOp` mapping for them
    // outside a logic-line assignment head.
    assert_error_at(
        "VAR x = 1\n~ temp y = x += 2\nHi\n",
        default_options(),
        DiagnosticCode::E016,
        "x += 2",
    );
}

#[test]
fn e017_struct_field_missing_name() {
    // A struct construction field with no name — the field-init arm of the
    // E017 "missing a name" family.
    assert_error_at(
        "STRUCT P = #{x: int}\n~ temp p = P#{: 1}\nHi\n",
        brink_options(),
        DiagnosticCode::E017,
        ": 1}",
    );
}

#[test]
fn e020_inline_conditional_missing_condition() {
    assert_error_at(
        "{: yes | no}\nHi\n",
        default_options(),
        DiagnosticCode::E020,
        ": yes | no",
    );
}

#[test]
fn e021_inline_sequence_without_branches() {
    // A shuffle annotation with no branches at all.
    assert_error_at("{~}\nHi\n", default_options(), DiagnosticCode::E021, "~");
}

// ─── Cross-file analysis (E022–E027) ─────────────────────────────────

#[test]
fn e022_duplicate_knot_warns() {
    let source = "== k ==\nA\n-> END\n== k ==\nB\n-> END\n-> k\n";
    // E022 itself is still warning-severity — the analyzer accepts a
    // duplicate knot name and keeps going. But both same-named knots still
    // lower to their own container, each addressed by that shared name, so
    // they collide on `DefinitionId` — a second, source-reachable instance
    // of the exact #1504-class landmine (#1504's collision was cross-file
    // and anonymous; this one is same-file and named, but it is still two
    // containers assigned one id). Before #1673 this silently compiled to a
    // broken `StoryData` (the linker's last-write-wins address map dropped
    // one knot's body); now the #1673 codegen-boundary guard trips E060 and
    // the whole compile fails loudly instead. Whether E022 itself should be
    // promoted to a hard error (giving a more precise source span than
    // E060's compiler-internal one) is a separate design question, not
    // decided here.
    let err = compile(source, default_options())
        .map(|_| ())
        .expect_err("duplicate knot names collide on DefinitionId and now trip the #1673 guard");
    let diags = errors_of(err);
    // The warning still points at the *second* definition's name.
    assert_code_at_nth(&diags, DiagnosticCode::E022, source, "== k ==", 1);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E060),
        "expected the #1673 duplicate-DefinitionId guard (E060) alongside E022: {diags:#?}"
    );
}

#[test]
fn e023_duplicate_variable_warns() {
    let source = "VAR x = 1\nVAR x = 2\nHi\n";
    assert_warning_at(source, default_options(), DiagnosticCode::E023, "VAR x = 2");
}

#[test]
fn e024_unresolved_divert_target() {
    assert_error_at(
        "-> nowhere\n",
        default_options(),
        DiagnosticCode::E024,
        "nowhere",
    );
}

#[test]
fn e025_unresolved_variable_reference() {
    assert_error_at(
        "~ temp t = missing\nHi\n",
        default_options(),
        DiagnosticCode::E025,
        "missing",
    );
}

#[test]
fn e026_duplicate_list_item_warns() {
    let source = "LIST l = a, a\nHi\n";
    assert_warning_at(source, default_options(), DiagnosticCode::E026, ", a");
}

#[test]
fn e027_ambiguous_bare_list_item_reference() {
    let source = "LIST l1 = shared\nLIST l2 = shared\n~ temp t = shared\nHi\n";
    let err = compile(source, default_options()).map(|_| ()).unwrap_err();
    let diags = errors_of(err);
    // The ambiguous *reference* is the third `shared`.
    assert_code_at_nth(&diags, DiagnosticCode::E027, source, "shared", 2);
}

#[test]
fn e025_multifile_with_earlier_multibyte_utf8_maintains_byte_offsets() {
    // Regression test for cross-file diagnostic offset tracking when an
    // earlier included file contains multi-byte UTF-8 content (#1056).
    // An included file with UTF-8 multi-byte characters (e.g., "café" or emoji)
    // should not cause byte-offset miscalculation for diagnostics in later files.
    let files: HashMap<&str, &str> = HashMap::from([
        ("helpers.ink", "== café ==\nWelcome to the café.\n-> END\n"),
        ("main.ink", "INCLUDE helpers.ink\n~ temp t = missing\nHi\n"),
    ]);
    let err = compile_multifile("main.ink", &files, default_options())
        .map(|_| ())
        .expect_err("unresolved reference should fail compile");
    let diags = errors_of(err);

    // The diagnostic for the unresolved `missing` reference should exist,
    // and its byte offset within main.ink should point to "missing".
    // The multi-byte content in helpers.ink must not shift offsets in main.ink.
    let main_content = "INCLUDE helpers.ink\n~ temp t = missing\nHi\n";
    let expected_byte_offset = find_nth(main_content, "missing", 0);
    assert!(
        diags.iter().any(|d| {
            let start = usize::from(d.range.start());
            d.code == DiagnosticCode::E025
                && d.path.contains("main.ink")
                && expected_byte_offset <= start
                && start <= expected_byte_offset + "missing".len()
        }),
        "expected E025 for 'missing' at byte offset {} in main.ink, got: {:?}",
        expected_byte_offset,
        diags
            .iter()
            .map(|d| format!("{}@{}:{:?}", d.code.as_str(), d.path, d.range))
            .collect::<Vec<_>>()
    );
}

// ─── Structural validation (E032, E034, E036, E037) ──────────────────

#[test]
fn e032_return_outside_function() {
    assert_error_at(
        "~ return\nHi\n",
        default_options(),
        DiagnosticCode::E032,
        "return",
    );
}

#[test]
fn e034_all_fallback_choice_set_warns() {
    let source = "== k ==\nHi\n* -> done\n== done ==\n-> END\n-> k\n";
    assert_warning_at(source, default_options(), DiagnosticCode::E034, "* -> done");
}

#[test]
fn e036_unmet_brink_expect() {
    assert_error_at(
        "// brink-expect E025\nHello\n",
        default_options(),
        DiagnosticCode::E036,
        "// brink-expect E025",
    );
}

#[test]
fn e037_syntax_error() {
    assert_error_at(
        "{ unclosed\nHi\n",
        default_options(),
        DiagnosticCode::E037,
        "unclosed",
    );
}

// ─── Doc comments (E038, E043) ───────────────────────────────────────

#[test]
fn e038_malformed_doc_tag_warns() {
    let source = "/// @kind bogus\nEXTERNAL ping(x)\nHi\n";
    assert_warning_at(
        source,
        default_options(),
        DiagnosticCode::E038,
        "/// @kind bogus",
    );
}

#[test]
fn e043_inapplicable_doc_tag_warns() {
    // `@param` on a VAR — well-formed, wrong declaration kind.
    let source = "/// @param x {int}\nVAR health = 100\nHi\n";
    assert_warning_at(
        source,
        default_options(),
        DiagnosticCode::E043,
        "/// @param x {int}",
    );
}

// ─── Host manifest (E039–E042) ───────────────────────────────────────

fn manifest_external(name: &str, params: &[(&str, &str)]) -> ManifestExternal {
    ManifestExternal {
        name: name.to_string(),
        params: params
            .iter()
            .map(|(p, ty)| ManifestParam {
                name: (*p).to_string(),
                ty: TypeRef((*ty).to_string()),
            })
            .collect(),
        returns: TypeRef::default(),
        kind: brink_ir::host_manifest::ExternalKind::default(),
        doc: None,
        widgets: vec![],
        path: Vec::new(),
    }
}

#[test]
fn e039_manifest_arity_mismatch() {
    // ink declares 1 param; the registered manifest lists 2.
    let options = AnalysisOptions {
        host_manifest: Some(HostManifest {
            externals: vec![manifest_external("ping", &[("a", "string"), ("b", "int")])],
            types: vec![],
        }),
        ..AnalysisOptions::default()
    };
    assert_error_at(
        "EXTERNAL ping(x)\nHi\n",
        options,
        DiagnosticCode::E039,
        "ping(x)",
    );
}

#[test]
fn e040_unknown_semantic_type() {
    // Policy-conditional: fires only under
    // `SemanticTypeDiagnosticSeverity::Error` (default is Tolerant).
    let options = AnalysisOptions {
        semantic_type_check: brink_analyzer::SemanticTypeDiagnosticSeverity::Error,
        ..AnalysisOptions::default()
    };
    assert_error_at(
        "/// @param x {bogus_type}\nEXTERNAL ping(x)\nHi\n",
        options,
        DiagnosticCode::E040,
        "ping(x)",
    );
}

fn direction_manifest() -> HostManifest {
    HostManifest {
        externals: vec![manifest_external("walk", &[("dir", "direction")])],
        types: vec![SemanticTypeDef {
            name: "direction".to_string(),
            base: BaseType::String,
            constraint: Some(Constraint::Enum {
                values: vec!["north".to_string(), "south".to_string()],
            }),
            values: None,
            widget: None,
        }],
    }
}

#[test]
fn e041_external_argument_type_mismatch() {
    let options = AnalysisOptions {
        host_manifest: Some(direction_manifest()),
        ..AnalysisOptions::default()
    };
    assert_error_at(
        "EXTERNAL walk(dir)\n~ walk(5)\nHi\n",
        options,
        DiagnosticCode::E041,
        "walk(5)",
    );
}

#[test]
fn e042_external_argument_out_of_domain() {
    let options = AnalysisOptions {
        host_manifest: Some(direction_manifest()),
        ..AnalysisOptions::default()
    };
    assert_error_at(
        "EXTERNAL walk(dir)\n~ walk(\"west\")\nHi\n",
        options,
        DiagnosticCode::E042,
        "walk(\"west\")",
    );
}

// ─── Directives `#@…` (E044–E050) ────────────────────────────────────

#[test]
fn e044_unknown_directive() {
    assert_error_at(
        "#@locale\nVAR mood = 0\nHi\n",
        default_options(),
        DiagnosticCode::E044,
        "#@locale",
    );
}

#[test]
fn e045_directive_without_valid_target() {
    assert_error_at(
        "#@local\njust text\n",
        default_options(),
        DiagnosticCode::E045,
        "#@local",
    );
}

#[test]
fn e046_dynamic_directive() {
    assert_error_at(
        "#@{x}\nVAR mood = 0\nHi\n",
        default_options(),
        DiagnosticCode::E046,
        "#@{x}",
    );
}

#[test]
fn e047_directive_mixed_with_plain_tag() {
    assert_error_at(
        "#@local # art.png\nVAR mood = 0\nHi\n",
        default_options(),
        DiagnosticCode::E047,
        "#@local # art.png",
    );
}

#[test]
fn e048_duplicate_directive() {
    let source = "#@local\n#@local\nVAR mood = 0\nHi\n";
    let err = compile(source, default_options()).map(|_| ()).unwrap_err();
    let diags = errors_of(err);
    // The duplicate is the second `#@local`.
    assert_code_at_nth(&diags, DiagnosticCode::E048, source, "#@local", 1);
}

#[test]
fn e049_directive_unsupported_on_target() {
    assert_error_at(
        "#@local\nCONST max = 3\nHi\n",
        default_options(),
        DiagnosticCode::E049,
        "#@local",
    );
}

#[test]
fn e050_directive_with_arguments() {
    assert_error_at(
        "#@local(now)\nVAR mood = 0\nHi\n",
        default_options(),
        DiagnosticCode::E050,
        "#@local(now)",
    );
}

// ─── T1b logic blocks / stdlib (E057–E059) ───────────────────────────

#[test]
fn e057_break_outside_loop() {
    assert_error_at(
        "~ {\n    break\n}\nHi\n",
        brink_options(),
        DiagnosticCode::E057,
        "break",
    );
}

#[test]
fn e058_mutator_arity_mismatch() {
    assert_error_at(
        "VAR arr = 0\n~ {\n    arr = #[]\n    push(arr)\n}\nHi\n",
        brink_options(),
        DiagnosticCode::E058,
        "push(arr)",
    );
}

#[test]
fn e059_weave_nested_in_inline_content() {
    // A nested `*` choice inside a choice's own display-text conditional —
    // structurally cannot hold a child container (#585 backstop family).
    assert_error_at(
        "VAR x = 1\n* Pick {x > 0:\n- true: * nested\n    -> END\n- else: text\n}\n    -> END\n",
        default_options(),
        DiagnosticCode::E059,
        "* nested",
    );
}

// ─── Type annotations (E061; E062 retired by T1c-1 — brink dialect only) ──

#[test]
fn e061_unknown_type_name_in_annotation() {
    assert_error_at(
        "VAR x: Foo = 1\nHi\n",
        brink_options(),
        DiagnosticCode::E061,
        "Foo",
    );
}

#[test]
fn e062_retired_fn_type_annotation_is_legal_since_t1c1() {
    // T1c-1 (#699, docs/t1c-spec.md §4): `fn(T…): R` is a legal type form —
    // E062 is retired (reserved, never reused) and must not fire anywhere.
    let out = compile("VAR f: fn(int): int = 0\nHi\n", brink_options())
        .expect("a fn-type annotation compiles cleanly since T1c-1");
    assert!(
        !out.warnings.iter().any(|d| d.code == DiagnosticCode::E062),
        "E062 is retired and must never fire: {:?}",
        out.warnings
    );
}

#[test]
fn e062_retired_fn_type_annotation_actually_resolves_under_strict() {
    // Absence alone could mean the annotation was silently ignored — prove
    // it resolves to a real checker type: under `types = strict`, `cb`'s
    // ONLY constraint is its `fn(int): int` annotation, so this compiles
    // clean iff the annotation genuinely carries the fn type (otherwise
    // `cb` escapes as Unknown, E065, and the call through it can't check).
    let source = "=== function apply(cb: fn(int): int): int ===\n~ return cb(1)\nHi\n-> END\n";
    let out = compile(source, strict_options());
    assert!(
        out.is_ok(),
        "fn-typed boundary annotation must resolve and check under strict: {:?}",
        out.err()
    );
}

// ─── T1c-2 (#700): the E052 `#fn` lowering fence is retired ───────────

#[test]
fn e052_retired_fn_literal_lowers_without_error() {
    // A well-formed `#fn` creation site under dialect=brink parses, gates,
    // type-checks (T1c-1) AND now lowers for real (T1c-2, #700) — the former
    // E052 lowering fence is gone, so a valid `#fn` compiles clean. An E052
    // (or any diagnostic) would surface as a `Diagnostics` compile error.
    let result = compile(
        "=== function double(x) ===\n~ return x + x\n\n~ temp f = #fn(double, 1)\nHi\n",
        brink_options(),
    );
    if let Err(err) = result {
        let diags = errors_of(err);
        assert!(
            !diags.iter().any(|d| d.code == DiagnosticCode::E052),
            "E052 lowering fence should be retired for #fn, got {diags:?}",
        );
    }
}

// ─── Structs (E068–E071, E073, E074, E084) ───────────────────────────

const POINT_SRC: &str = "STRUCT Point = #{\n    x: float,\n    y: float,\n}\n";

#[test]
fn e068_undeclared_struct_shape() {
    let source = format!("{POINT_SRC}~ temp p = Nope#{{x: 1.0}}\nHi\n");
    assert_error_at(&source, brink_options(), DiagnosticCode::E068, "Nope");
}

#[test]
fn e069_strict_construction_missing_field() {
    let source = format!("{POINT_SRC}~ temp p = Point#{{x: 1.0}}\nHi\n");
    assert_error_at(
        &source,
        strict_options(),
        DiagnosticCode::E069,
        "Point#{x: 1.0}",
    );
}

#[test]
fn e070_strict_construction_extra_field() {
    let source = format!("{POINT_SRC}~ temp p = Point#{{x: 1.0, y: 2.0, z: 3.0}}\nHi\n");
    assert_error_at(&source, strict_options(), DiagnosticCode::E070, "z: 3.0");
}

#[test]
fn e071_strict_construction_mistyped_field() {
    let source = format!("{POINT_SRC}~ temp p = Point#{{x: \"oops\", y: 2.0}}\nHi\n");
    assert_error_at(
        &source,
        strict_options(),
        DiagnosticCode::E071,
        "x: \"oops\"",
    );
}

#[test]
fn e073_unresolved_shape_reaches_lir_when_e068_suppressed() {
    // The non-suppressible LIR backstop: `// brink-disable-all` suppresses
    // the analyzer's E068, so the unresolved shape reaches LIR lowering.
    let source = format!("// brink-disable-all\n{POINT_SRC}~ temp p = Nope#{{x: 1.0}}\nHi\n");
    assert_error_at(
        &source,
        brink_options(),
        DiagnosticCode::E073,
        "Nope#{x: 1.0}",
    );
}

#[test]
fn e074_chained_field_write_projection() {
    let source = "STRUCT Inner = #{v: int}\nSTRUCT Outer = #{inner: Inner}\nVAR o = 0\n~ {\n    o = Outer#{inner: Inner#{v: 1}}\n    o.inner.v = 2\n}\nHi\n";
    assert_error_at(source, brink_options(), DiagnosticCode::E074, "o.inner.v");
}

#[test]
fn e084_duplicate_field_under_gradual() {
    // Policy-independent (issue #675): fires under the default `types =
    // gradual` too, not just strict — the repeated occurrence is the span.
    let source = format!("{POINT_SRC}~ temp p = Point#{{x: 1.0, x: 2.0, y: 3.0}}\nHi\n");
    assert_error_at(&source, brink_options(), DiagnosticCode::E084, "x: 2.0");
}

#[test]
fn e084_duplicate_field_under_strict() {
    let source = format!("{POINT_SRC}~ temp p = Point#{{x: 1.0, x: 2.0, y: 3.0}}\nHi\n");
    assert_error_at(&source, strict_options(), DiagnosticCode::E084, "x: 2.0");
}

// ─── T1e-1 path projections (E097–E099, docs/t1e-spec.md §2/§6, issue #831) ──

const HEAL_SRC: &str = "=== function heal(ref hp: int, k: int) ===\n~ hp = hp + k\n\n";

#[test]
fn e080_ref_projection_root_is_a_temp() {
    // T1e's `ref` extends the existing E080 code (not a new one) to the
    // path-projection grammar — same durable-root obligation as T1c's
    // unmarked ref-argument form.
    let source = format!("{HEAL_SRC}=== main ===\n~ temp t = 1\n~ heal(ref t, 5)\n-> DONE\n");
    assert_error_at(&source, brink_options(), DiagnosticCode::E080, "ref t");
}

#[test]
fn e097_standalone_ref_projection() {
    let source = "VAR gold = 5\n=== main ===\n~ temp r = ref gold\n-> DONE\n";
    assert_error_at(source, brink_options(), DiagnosticCode::E097, "ref gold");
}

#[test]
fn e098_strict_unknown_field_segment() {
    let source = format!(
        "STRUCT NPC = #{{hp: int}}\nVAR npc: NPC = NPC#{{hp: 10}}\n{HEAL_SRC}\
         === main ===\n~ heal(ref npc.mana, 5)\n-> DONE\n"
    );
    assert_error_at(&source, strict_options(), DiagnosticCode::E098, "npc.mana");
}

#[test]
fn real_path_projection_lowers_for_real_no_longer_e099() {
    // T1e-2 (docs/t1e-spec.md §3, tracking #828) replaces the T1e-1 E099
    // lowering fence with real `MakeProjection` emission for a genuine path
    // projection (a real segment, not a bare single-name `ref`) that passes
    // every analyzer check. `npc` here has no statically-known STRUCT shape
    // (gradual mode, no `VAR npc: Shape` annotation), so `.hp` is a runtime
    // by-name segment — exactly the case the old fence used to stop at.
    let source = format!("VAR npc = 5\n{HEAL_SRC}=== main ===\n~ heal(ref npc.hp, 5)\n-> DONE\n");
    // E099 is error-severity (the old fence made `compile` fail); a
    // successful compile alone proves it no longer fires here.
    let out = compile(&source, gradual_options())
        .unwrap_or_else(|e| panic!("a real path-projection ref-argument must lower: {e:?}"));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn e099_bind_ref_argument_always_fences_even_zero_segment() {
    // `bind(f, args…)` (docs/t1c-spec.md §3) is val-only currying at LIR —
    // `BindValue`'s args have no `CallArg`/ref-capture concept at all
    // (unlike ordinary calls/`#fn`, which already have `RefGlobal`). So
    // *any* `ref`-marked bind argument fences here, even the zero-segment
    // case that a plain call/`#fn` would lower for real — there's no
    // "today's unmarked behavior" to fall back to for `bind`, since `bind`
    // never supported ref-argument binding before T1e either. Proves this
    // is a clean, targeted stop, not a silent value-lowering of a `ref`.
    let source = format!(
        "VAR gold = 5\n{HEAL_SRC}=== main ===\n~ temp f = #fn(heal, gold)\n\
         ~ temp g = bind(f, ref gold)\n-> DONE\n"
    );
    // Gradual knob (#1127): the subject is the LIR-layer bind fence, which
    // is only reachable once analysis passes — `#fn` over a `ref`-param
    // function stays Unknown under strict inference (E065 gates lowering
    // first), so this fixture TESTS the dynamic regime's fence.
    assert_error_at(&source, gradual_options(), DiagnosticCode::E099, "ref gold");
}

#[test]
fn ref_marked_bare_var_arg_compiles_clean_through_the_real_pipeline() {
    // The zero-segment case (`ref gold`, no dotted field / `[…]` index) is
    // not a projection at all — it must compile exactly like today's
    // unmarked `ref`-argument form, never hitting E097/E099.
    let source = format!("VAR gold = 5\n{HEAL_SRC}=== main ===\n~ heal(ref gold, 5)\n-> DONE\n");
    let out = compile(&source, brink_options())
        .unwrap_or_else(|e| panic!("a bare single-name `ref` must compile clean: {e:?}"));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

// ─── Computed-callee call attempt (E104, docs/t1c-spec.md §3/§10, #869) ──
//
// A direct call `expr(args…)` where `expr` isn't a bare variable/temp/param
// name — pre-#869 this silently dropped the call entirely (the parser left
// `(args…)` unconsumed, so it resurfaced as prose text on the content
// line). Direct-call syntax is RULED (t1c-spec §3) to a bare-name callee
// only, and dispatch through a computed callee via bare-call sugar
// ("method-call syntax") is explicitly out of T1c (§10) — so every shape
// below is a loud compile error, never a silent no-op. One fixture per
// non-bare-name callee shape the npc-fsm/behavior-tree tier1 corpus
// fixtures found (indexed, dotted field, call-result), plus proof that the
// ratified `call(f, args…)` Explicit form and the bare-name Direct form are
// both untouched.

#[test]
fn e104_indexed_callee() {
    let source = "VAR handlers = #{}\nVAR state = \"x\"\n=== main ===\n\
                  ~ handlers[state](1)\n-> DONE\n";
    assert_error_at(
        source,
        brink_options(),
        DiagnosticCode::E104,
        "handlers[state](1)",
    );
}

#[test]
fn e104_dotted_field_callee() {
    let source = "VAR obj = #{}\n=== main ===\n~ obj.field()\n-> DONE\n";
    assert_error_at(source, brink_options(), DiagnosticCode::E104, "obj.field()");
}

#[test]
fn e104_call_result_callee_dialect_independent() {
    // No brink-only construct in sight (a plain `function`, `return`, two
    // ordinary calls) — proves E104 fires the same under strict-ink
    // (`default_options()`) as under brink, unlike every dialect-gated T1b/
    // T1c construct: a computed callee is invalid syntax in every dialect,
    // not a brink extension strict-ink rejects.
    let source = "=== function get_handler() ===\n~ return 1\n\n\
                  === main ===\n~ get_handler()()\n-> DONE\n";
    assert_error_at(
        source,
        default_options(),
        DiagnosticCode::E104,
        "get_handler()()",
    );
}

#[test]
fn bare_name_direct_call_unaffected_by_e104() {
    let source = "=== function bare(a: int, b: int): int ===\n~ return a + b\n\n\
                  === main ===\n~ bare(1, 2)\n-> DONE\n";
    let out = compile(source, brink_options())
        .unwrap_or_else(|e| panic!("a bare-name direct call must compile clean: {e:?}"));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

#[test]
fn explicit_call_form_unaffected_by_e104() {
    // `call(f, args…)` lowers as an ordinary named call (`Expr::Call(path =
    // "call", …)`), never as the new `CALL_EXPR` shape — the same
    // computed-callee expression that's rejected via bare-call sugar above
    // dispatches correctly through the ratified Explicit form.
    let source = "VAR handlers = #{}\nVAR state = \"x\"\n=== main ===\n\
                  ~ call(handlers[state], 1)\n-> DONE\n";
    let out = compile(source, gradual_options())
        .unwrap_or_else(|e| panic!("call(f, args…) must compile clean: {e:?}"));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

// ─── E113: reserved protocol method names (NS-A3, issue #1109; F6 ruled
// 2026-07-19, docs/stdlib-spec.md §9.6). Hard error under the brink
// dialect only — under strict-ink there is no protocol registry and
// vanilla ink identifiers stay untouched. E114/E115 (protocol impl
// contract/shape validation) are pipeline-covered in
// `brink-analyzer::protocols`' own tests: impl registration is a
// programmatic surface (no source spelling until the code-dialect
// sitting), so no `.ink` fixture can reach them. ─────────────────────

#[test]
fn e113_knot_named_display_is_reserved_in_brink() {
    let source = "== display ==\nHello.\n-> DONE\n";
    assert_error_at(source, brink_options(), DiagnosticCode::E113, "display");
}

#[test]
fn e113_function_named_next_is_reserved_in_brink() {
    let source = "=== function next(x) ===\n~ return x\n=== main ===\nHi.\n-> DONE\n";
    assert_error_at(source, brink_options(), DiagnosticCode::E113, "next");
}

#[test]
fn e113_var_named_compare_is_reserved_in_brink() {
    let source = "VAR compare = 1\n== k ==\n{compare}\n-> DONE\n";
    assert_error_at(source, brink_options(), DiagnosticCode::E113, "compare");
}

#[test]
fn e113_temp_named_display_in_logic_block_is_reserved() {
    let source = "== k ==\n~ {\n    temp display = 1\n}\n-> DONE\n";
    assert_error_at(source, brink_options(), DiagnosticCode::E113, "display");
}

#[test]
fn e113_does_not_fire_under_strict_ink() {
    // Vanilla ink may freely name a VAR `display` — the reservation is a
    // brink-dialect protocol concern (the oracle corpus stays out of
    // reach by construction).
    let source = "VAR display = 1\n== k ==\n{display}\n-> DONE\n";
    let out = compile(source, default_options())
        .unwrap_or_else(|e| panic!("strict-ink must not reserve protocol names: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E113),
        "{:?}",
        out.warnings
    );
}

#[test]
fn e113_list_member_named_next_stays_legal_in_brink() {
    // Deliberate carve-out: LIST members are value-position narrative
    // vocabulary (`next` is plausible domain language), not callables.
    let source = "LIST steps = intro, next, outro\n== k ==\nHi.\n-> DONE\n";
    let out = compile(source, brink_options())
        .unwrap_or_else(|e| panic!("LIST members are not reserved: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E113),
        "{:?}",
        out.warnings
    );
}

// ─── E066 (`or`-coalescing mismatch): review finding on PR #1469/#1460 ──
//
// `InfixOp::Coalesce` is produced only by native `hir::lower_native`
// (B1, issue #1460) — every fixture above compiles through `brink-syntax`
// (the ink/brink-extension frontend, dispatched via a `main.ink`-suffixed
// in-memory entry), which can never reach it. Native discovery
// (`brink_driver::Driver::discover_native`) also reads straight off disk
// (`RealFs`), bypassing this file's `compile()` harness's in-memory
// `read_file` callback entirely — so these two fixtures need their own
// disk-based helper rather than a `main.brink`-suffixed `compile()` call.

/// Compile a native `.brink` entry from disk with explicit analysis
/// options — mirrors `crates/brink-compiler/tests/driver.rs`'s own
/// `compile_and_run_native` helper, minus the run-to-completion step (these
/// fixtures are expected to fail the compile, not execute).
fn compile_native(
    dir_suffix: &str,
    source: &str,
    options: AnalysisOptions,
) -> Result<brink_compiler::CompileOutput, brink_compiler::CompileError> {
    let dir = std::env::temp_dir().join(format!(
        "brink-compiler-e0xx-coalesce-{dir_suffix}-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("main.brink"), source).unwrap();
    let result = brink_compiler::compile_path_with_options(&dir.join("main.brink"), options);
    std::fs::remove_dir_all(&dir).ok();
    result
}

/// `types = strict` explicit (native's own default resolves to `Gradual`
/// today — B0.10's dialect-keyed strict-only wiring has not landed, see
/// `brink-analyzer::strict::native_strict_only_error`'s doc); `is_native`
/// skips `E064`'s ink-only dialect check regardless of `dialect`'s value.
fn native_strict_options() -> AnalysisOptions {
    AnalysisOptions {
        types: Some(TypePolicy::Strict),
        ..AnalysisOptions::default()
    }
}

#[test]
fn e066_coalesce_non_option_left_hand_side() {
    let source = "flow main() {\n  {5 or 9}\n  -> END\n}\n";
    let err = compile_native("left-not-option", source, native_strict_options())
        .map(|_| ())
        .expect_err("a concrete-typed left-hand side must fail under types = strict");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E066),
        "expected E066, got: {diags:?}"
    );
}

#[test]
fn e066_coalesce_mismatched_fallback_type() {
    let source = "flow main() {\n  {some(1) or \"text\"}\n  -> END\n}\n";
    let err = compile_native("mismatch", source, native_strict_options())
        .map(|_| ())
        .expect_err("an int-element Option coalesced against a string fallback must fail");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E066),
        "expected E066, got: {diags:?}"
    );
}

/// Issue #1492: the chain fold reaches every step, not just the innermost.
///
/// Before the analyzer recorded each step's result type, this compiled
/// silently — `structs::classify_expr_ty` returns `None` for an
/// `Expr::Infix` left-hand operand, so the outer `… or "text"` step had no
/// left-hand type to judge and was skipped. Now the inner step's recorded
/// `Option[int]` is fed in, and the mismatch is caught where it always was.
#[test]
fn e066_coalesce_mismatch_at_a_later_chain_step() {
    let source = "flow main() {\n  {some(1) or none or \"text\"}\n  -> END\n}\n";
    let err = compile_native("chain-mismatch", source, native_strict_options())
        .map(|_| ())
        .expect_err("a string fallback on an int-element Option chain must fail");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E066),
        "expected E066, got: {diags:?}"
    );
}

/// The other half of the same fold: a chain whose every step *does* type
/// still compiles. A fold that rejected too eagerly would break this.
#[test]
fn a_well_typed_coalescing_chain_still_compiles_under_strict() {
    let source = concat!(
        "fn maybe() {\n  return some(7);\n}\n",
        "flow main() {\n  {some(1) or maybe() or 99}\n  -> END\n}\n",
    );
    let result = compile_native("chain-ok", source, native_strict_options());
    assert!(
        result.is_ok(),
        "a well-typed chain must compile: {:?}",
        result.map(|_| ()).err()
    );
}

// ─── B1b the `as` binding (issue #1475): E145/E146/E147 ────────────────
//
// Native-only, same reasoning as the E066 block above — an `AS_BINDING`
// node exists only in the native grammar, so these reuse `compile_native`.

/// E145 — the v1 whole-condition restriction, caught at HIR lowering when
/// the binding sits on top of a `&&` composition. (The mirror spelling, an
/// operator *after* the binding, is a parse error instead; see
/// `brink-syntax-native`'s `parser::tests::statement`.)
#[test]
fn e140_as_binding_over_a_boolean_composition() {
    // Both boolean operators, not just `&&`: the native lowering maps `&&`
    // to `InfixOp::And` and `||` to `InfixOp::Or`, and the whole-condition
    // rule refuses either as the bound expression.
    for (suffix, op) in [("as-composed-and", "&&"), ("as-composed-or", "||")] {
        let source =
            format!("flow main() ~{{\n  if true {op} some(1) as n {{\n    return n;\n  }}\n}}\n");
        let Err(err) = compile_native(suffix, &source, native_strict_options()).map(|_| ()) else {
            panic!("`as` over a `{op}` composition must fail");
        };
        let diags = errors_of(err);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E145),
            "expected E145 for `{op}`, got: {diags:?}"
        );
    }
}

/// E146 — guard-`as` is ruled but rides the `.inkb` v6 Choice record, so it
/// is diagnosed as *not yet supported* rather than half-lowered.
#[test]
fn e141_as_binding_in_a_choice_guard_is_not_yet_supported() {
    let source = "flow main() {
  {?
    * {if some(1) as n} take it
  }
  -> END
}
";
    let err = compile_native("as-guard", source, native_strict_options())
        .map(|_| ())
        .expect_err("guard-`as` must be refused until the v6 Choice record lands");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E146),
        "expected E146, got: {diags:?}"
    );
}

/// E147 — the binding unwraps `Option[T]`; a statically classifiable
/// non-Option condition has nothing to unwrap. The strict-mode twin of the
/// runtime's `AsBindingNotOption` fault.
#[test]
fn e142_as_binding_on_a_non_option_condition() {
    let source = "flow main() {
  {if 5 as n: got {n} else: nope}
  -> END
}
";
    let err = compile_native("as-not-option", source, native_strict_options())
        .map(|_| ())
        .expect_err("`as` over an int condition must fail under types = strict");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E147),
        "expected E147, got: {diags:?}"
    );
}

/// E148 — the binding is immutable by ruling. Every write shape resolves
/// its target through the same LIR choke point, so all five are refused:
/// plain assignment, compound assignment, an in-place mutator, passing the
/// binding by `ref` to a function that writes through it (the review
/// finding this case guards: `ref` bypasses `lower_assign_target` entirely
/// — it hands the callee a raw pointer to the slot — so it needs its own
/// refusal at `lower_ref_path_call_arg`/`lower_ref_projection_arg`), and a
/// UFCS auto-ref onto a frame-local projection rooted at the binding (the
/// #1531-review finding this fifth case guards: issue #1531's frame-local
/// auto-ref recognizer, `blocks::try_lower_frame_local_auto_ref_stmt`,
/// writes back into the receiver's root slot via its own `Assign`, bypassing
/// `lower_assign_target` exactly like the `ref` case above — the same E148
/// as the `-ref` case, but via a different LIR choke point).
#[test]
fn e143_as_binding_is_immutable() {
    for (suffix, write) in [
        ("as-imm-assign", "n = 1;"),
        ("as-imm-compound", "n += 1;"),
        ("as-imm-mutator", "pop(n);"),
        // Native `ref` is a parameter-position marker (`fn bump(ref x)`),
        // not a call-site keyword: an argument at a `ref` parameter's
        // position is auto-ref'd from its bare path, so this reaches the
        // same `lower_ref_path_call_arg` choke point as the ink-dialect's
        // explicit `heal(ref t, 5)` spelling would.
        ("as-imm-ref", "bump(n);"),
    ] {
        let source = format!(
            "fn bump(ref x) {{\n  x = x + 1;\n}}\n\nflow main() ~{{\n  if some(1) as n {{\n    \
             {write}\n  }}\n}}\n"
        );
        let err = compile_native(suffix, &source, native_strict_options())
            .map(|_| ())
            .unwrap_err();
        let diags = errors_of(err);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E148),
            "expected E148 for `{write}`, got: {diags:?}"
        );
    }

    // The fifth shape needs its own source: the `as` binding must be a
    // struct so `n.field` is a genuine one-level-deep frame-local
    // projection (issue #1531), and the write goes through
    // `try_lower_frame_local_auto_ref_stmt`'s UFCS auto-ref RMW splice
    // rather than `bump`'s ordinary `ref` parameter.
    let source = "\
struct Guest {
  hp: int
}

fn heal(ref h: int, amount: int) {
  h = h + amount;
}

flow main() ~{
  if some(Guest { hp: 1 }) as g {
    g.hp.heal(5);
  }
}
";
    let err = compile_native("as-imm-ufcs-projection", source, native_strict_options())
        .map(|_| ())
        .unwrap_err();
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E148),
        "expected E148 for `g.hp.heal(5);`, got: {diags:?}"
    );
}

// ─── `remove`/`remove_at` migration tail (E149, issue #1532) ────────────
//
// The #1501 review's follow-up on #1484's `remove`/`remove_at` split:
// `remove` went map-only with no compatibility shim, so an un-migrated
// `remove(array, i)` call site used to compile clean and only fault at
// runtime (`RuntimeError::NotIndexable`, `remove_on_an_array_faults` in
// `brink-test-harness/tests/tier1_brink.rs`). `infer::body`'s `remove` arm
// already has `Ty::Array` in hand at the call site, so this is now a
// compile error — strict-mode-only, matching every other TM-3 typed check
// in this file (`brink_options()` leaves `types` unset, which resolves to
// the brink dialect's own implicit-strict default, issue #1127).
//
// Issue #1540 closed the global-scope half of this: a `VAR`'s static type
// is still purely declaration-derived, but it is now derived at full `Ty`
// fidelity (`signature.rs::Sig::value_ty`) instead of through the
// `InferredType` downcast that had no `Array`/`Map` representation, so
// `VAR arr = #[…]` reaches `E149`'s `Ty::Array` guard exactly like a
// `temp` does — see `e149_remove_on_a_statically_known_global_array`.
// What is still out of reach is `VAR arr = 0` *reassigned* to an array
// literal (the idiom `remove_on_an_array_faults` uses for the
// runtime-fault twin of this call shape): a declaration-derived type
// cannot see a later assignment, so that global is statically `Int`.

/// E149 — a statically-known array first argument to `remove`.
#[test]
fn e149_remove_on_a_statically_known_array() {
    let source = "=== main ===\n~ {\n    temp arr = #[1, 2, 3]\n    remove(arr, 0)\n}\n-> DONE\n";
    assert_error_at(
        source,
        brink_options(),
        DiagnosticCode::E149,
        "remove(arr, 0)",
    );
}

/// E149 — issue #1540: the same statically-known array, spelled as a global
/// `VAR` with an array-literal default. This is the authoring idiom the
/// issue names, and before the `Sig::value_ty` fix it compiled clean here
/// (the global typed as nothing at all) while the `temp` twin above
/// reported.
#[test]
fn e149_remove_on_a_statically_known_global_array() {
    let source = "VAR arr = #[1, 2, 3]\n=== main ===\n~ remove(arr, 0)\n-> DONE\n";
    assert_error_at(
        source,
        brink_options(),
        DiagnosticCode::E149,
        "remove(arr, 0)",
    );
}

/// E149 — the annotated spelling of the same global (`ty_to_inferred_type`'s
/// gap proper: an `array<T>` annotation had no `InferredType` form, so it
/// was dropped and the *initializer* decided the global's static type).
#[test]
fn e149_remove_on_an_array_annotated_global() {
    let source = "VAR arr: array<int> = #[1, 2, 3]\n=== main ===\n~ remove(arr, 0)\n-> DONE\n";
    assert_error_at(
        source,
        brink_options(),
        DiagnosticCode::E149,
        "remove(arr, 0)",
    );
}

/// A map-typed *global* is untouched by the widening — the same map leg the
/// `temp` fixture below guards, proven at global scope too so #1540's wider
/// `value_ty` cannot start reporting the verb's legal receiver.
#[test]
fn remove_on_a_global_map_unaffected_by_e149() {
    let source = "VAR m = #{\"a\": 1}\n=== main ===\n~ remove(m, \"a\")\n-> DONE\n";
    let out = compile(source, brink_options())
        .unwrap_or_else(|e| panic!("remove(m, k) on a global map must compile clean: {e:?}"));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

/// `remove` on a map is untouched — the map leg this code guards is the
/// verb's actual, unaffected posture.
#[test]
fn remove_on_a_map_unaffected_by_e149() {
    let source = "=== main ===\n~ {\n    temp m = #{\"a\": 1}\n    remove(m, \"a\")\n}\n-> DONE\n";
    let out = compile(source, brink_options())
        .unwrap_or_else(|e| panic!("remove(m, k) on a map must compile clean: {e:?}"));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

// ─── E149 through the UFCS spelling (issue #1540, second symptom) ──────
//
// `infer::body::infer_call` types a multi-segment callee `Ty::Unknown`
// *before* `infer_intrinsic` runs (a UFCS receiver isn't the thing being
// called), so `arr.remove(0)` recorded none of the facts `remove(arr, 0)`
// records and every intrinsic-receiver diagnostic silently stopped at the
// free-call spelling. `ufcs::check_strict` reads the B3a verdict table
// instead, which already carries the receiver's resolved `Ty` beside the
// verb's name.
//
// These are native (`.brink`) fixtures because UFCS is native-only by
// construction (ink's own lowering never builds a multi-segment callee
// path — see `brink-analyzer::ufcs`'s module doc). The receiver comes from
// `keys(m)` rather than an array literal: the native surface has no array
// literal at all today (`construct::ConstructTarget` registers `Map`,
// `Flags` and `Weighted` only), so an array-returning intrinsic is how a
// native author actually gets one.

/// E149 fires on the UFCS spelling of an array `remove`.
#[test]
fn e149_ufcs_remove_on_a_statically_known_array() {
    let source = "fn main() {\n  let m = Map { \"a\": 1 };\n  let ks = keys(m);\n  ks.remove(0);\n  \
                  return 1;\n}\n";
    let err = compile_native("ufcs-remove", source, native_strict_options())
        .map(|_| ())
        .unwrap_err();
    let diags = errors_of(err);
    assert_code_at_nth(&diags, DiagnosticCode::E149, source, "ks.remove(0)", 0);
}

/// The verb's legal receiver is untouched by the UFCS leg: a *map* receiver
/// must stay clean, or the check would refuse `remove`'s actual posture.
#[test]
fn ufcs_remove_on_a_map_unaffected_by_e149() {
    let source = "fn main() {\n  let m = Map { \"a\": 1 };\n  m.remove(\"a\");\n  return 1;\n}\n";
    let out = compile_native("ufcs-remove-map", source, native_strict_options())
        .unwrap_or_else(|e| panic!("`m.remove(k)` on a map must compile clean: {e:?}"));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

/// The migration target itself: `remove_at` on the same array receiver is
/// what the author is being pointed at, so it must compile clean.
#[test]
fn ufcs_remove_at_on_an_array_is_the_clean_migration_target() {
    let source = "fn main() {\n  let m = Map { \"a\": 1 };\n  let ks = keys(m);\n  \
                  ks.remove_at(0);\n  return 1;\n}\n";
    let out = compile_native("ufcs-remove-at", source, native_strict_options())
        .unwrap_or_else(|e| panic!("`ks.remove_at(i)` must compile clean: {e:?}"));
    assert!(out.warnings.is_empty(), "{:?}", out.warnings);
}

/// Under `types = gradual` the check is inert (T1c/TM-3's universal split
/// — the runtime `NotIndexable` fault is the residual backstop, proven at
/// the runtime layer by `tier1_brink.rs`'s `remove_on_an_array_faults`).
#[test]
fn e149_inert_under_gradual_types() {
    let source = "=== main ===\n~ {\n    temp arr = #[1, 2, 3]\n    remove(arr, 0)\n}\n-> DONE\n";
    let out = compile(source, gradual_options())
        .unwrap_or_else(|e| panic!("remove(array, i) must still compile under gradual: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E149),
        "E149 must not fire under types = gradual: {:?}",
        out.warnings
    );
}

// ─── E150 (issue #1551): declared-return-value def falls through ───────
//
// Native-only, same reasoning as the E066/`as`-binding blocks above — a
// declared, non-`void` return-type annotation on a flow/stitch only exists
// in the native grammar, so this reuses `compile_native`. `gate` is the
// return-value producer whose fall-through is under test; `main` just gives
// the file a valid entry point.

/// E150 fires through the real pipeline (parse → HIR lower → analyze) when
/// a `flow` declares a non-`void` return type and its body never reaches a
/// value-returning `return <expr>`.
#[test]
fn e150_value_returning_flow_falling_through() {
    let source = "flow main() {\n  Hi.\n}\n\nflow gate(): int {\n  Onward.\n}\n";
    let err = compile_native("e150", source, native_strict_options())
        .map(|_| ())
        .expect_err("a declared-return-value flow that falls through must fail");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E150),
        "expected E150, got: {diags:?}"
    );
}

/// Negative sibling of [`e150_value_returning_flow_falling_through`], driven
/// through the same real pipeline (issue #1591): `gate`'s own body is empty,
/// so it falls straight through into its nested stitch `compute` — no
/// explicit divert — and the value-returning `return` lives entirely in
/// that stitch. `check_def`'s `E150` fall-through check must read the
/// has-value-return fact merged over `gate`'s stitches
/// (`has_value_return_over_stitches`), not just `gate`'s own (empty) body,
/// or this compiles-clean shape would wrongly fail exactly like the
/// falling-through-with-no-value case above.
///
/// `compute` uses code ground (`~{ }`) for its own body — same posture as
/// `native_value_returning_knot_that_always_returns_is_clean` in
/// `strict.rs` — since a value-carrying `return <expr>;` is a code-ground
/// statement; `gate`'s own body stays prose ground (`{ }`) since it holds
/// only the nested `flow` declaration.
#[test]
fn e150_value_returning_flow_reached_only_through_a_fallthrough_stitch_compiles_clean() {
    let source = "flow main() {\n  Hi.\n}\n\nflow gate(): int {\n  flow compute() ~{\n    return 5;\n  }\n}\n";
    let out = compile_native("e150-stitch-fallthrough", source, native_strict_options())
        .unwrap_or_else(|e| {
            panic!(
                "a value return reached only via a fall-through stitch must compile clean: {e:?}"
            )
        });
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E150),
        "expected no E150, got: {:?}",
        out.warnings
    );
}

// ─── E151 (issue #1219): asymmetric choice-branch dead-end lint ────────
//
// Native-only, same `compile_native` posture as E150/E066 above — the `{?
// … }` choice-point grammar this lint reads only exists on the native
// surface. `AnalysisOptions::default()` suffices throughout: the lint is
// independent of the `types` policy.

/// The issue's own worked example: one choice diverts (`-> riposte`), its
/// sibling falls through with narration and nothing else follows the choice
/// point — a genuine dead end. Warning-severity, so the compile still
/// succeeds; `E151` shows up in `out.warnings`, never `out` (errors).
#[test]
fn e151_mixed_tail_at_a_dead_end_is_flagged() {
    let source = "flow main() {\n  {?\n    * Parry -> riposte\n    * [Dodge] {\n      \
                  You sidestep the blade.\n    }\n  }\n}\n\nflow riposte() {\n}\n";
    let out = compile_native("e151-dead-end", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("a Warning-severity lint must not fail the compile: {e:?}"));
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E151),
        "expected E151, got: {:?}",
        out.warnings
    );
}

/// The precision-critical exclusion: content follows the choice point (the
/// dissolved gather, `docs/native-surface-charter.md` §5), so the
/// undiverted `[Dodge]` branch converges there by design — ordinary
/// asymmetric-weave reconvergence, not a dead end. Must never fire.
#[test]
fn e151_reconverging_into_the_dissolved_gather_is_not_flagged() {
    let source = "flow main() {\n  {?\n    * Parry -> riposte\n    * Dodge\n  }\n  \
                  You catch your breath.\n}\n\nflow riposte() {\n}\n";
    let out = compile_native("e151-gather", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("must compile clean: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "reconvergence into the dissolved gather must not fire E151, got: {:?}",
        out.warnings
    );
}

/// Every branch diverts — fully symmetric, no fingerprint of a forgotten
/// `->`. Must never fire.
#[test]
fn e151_all_branches_diverging_is_not_flagged() {
    let source = "flow main() {\n  {?\n    * Parry -> riposte\n    * Dodge -> riposte\n  }\n}\n\n\
                  flow riposte() {\n}\n";
    let out = compile_native("e151-all-diverge", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("must compile clean: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "a fully symmetric diverging choice set must not fire E151, got: {:?}",
        out.warnings
    );
}

/// No branch diverts — "a menu that ends" (decision-log 2026-07-22 item 4's
/// own wording), the ordinary implicit-DONE shape this whole ruling exists
/// to make silent. Must never fire.
#[test]
fn e151_all_branches_falling_through_is_not_flagged() {
    let source = "flow main() {\n  {?\n    * Wait\n    * Look\n  }\n}\n";
    let out = compile_native("e151-all-unit", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("must compile clean: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "a symmetric menu-that-ends choice set must not fire E151, got: {:?}",
        out.warnings
    );
}

// ─── E151 review follow-up: the four false-positive/false-negative shapes
// a literal-last-statement `diverges()` missed (PR #1575 review finding 1),
// each proven end to end through the real pipeline, not just the unit-level
// `terminates()` coverage in `native_choice_dead_end.rs` itself.

/// (a) G-1 label absorption: the diverting branch's `-> combat` ends up one
/// level down inside a trailing `Stmt::LabeledBlock` once `(again)` labels
/// the content line that precedes it. Must not be told to add `->` — it
/// already has one.
#[test]
fn e151_label_absorbed_divert_is_not_flagged() {
    let source = "flow main() {\n  {?\n    * [Talk] {\n      (again) You talk.\n      \
                  -> combat\n    }\n    * Fight -> combat\n  }\n}\n\nflow combat() {\n}\n";
    let out = compile_native("e151-label-absorbed", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("must compile clean: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "a divert wrapped in a trailing label-absorbed block must not be flagged, got: {:?}",
        out.warnings
    );
}

/// (b) An inline divert before a braced body (`* [Talk] -> combat { … }`)
/// lowers the divert *first*, not last. Must not be flagged (and must not
/// contradict the E033 "unreachable code after divert" this shape
/// independently earns).
#[test]
fn e151_leading_divert_before_a_braced_body_is_not_flagged() {
    let source = "flow main() {\n  {?\n    * [Talk] -> combat {\n      You talk.\n    }\n    \
                  * Fight -> combat\n  }\n}\n\nflow combat() {\n}\n";
    let out = compile_native("e151-leading-divert", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("must compile clean: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "a divert preceding a braced body must not be flagged, got: {:?}",
        out.warnings
    );
}

/// (c) Every arm of a trailing conditional diverges (with an explicit
/// `else`) — a terminator in substance, even though it's not itself a
/// `Divert`/`Return` statement. Must not be flagged.
#[test]
fn e151_all_arms_diverging_conditional_is_not_flagged() {
    let source = "flow main() {\n  {?\n    * [Talk] {\n      {if true {\n        \
                  -> combat\n      } else {\n        -> combat\n      }}\n    }\n    \
                  * Fight -> combat\n  }\n}\n\nflow combat() {\n}\n";
    let out = compile_native(
        "e151-all-arms-conditional",
        source,
        AnalysisOptions::default(),
    )
    .unwrap_or_else(|e| panic!("must compile clean: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "a conditional whose every arm diverges must not be flagged, got: {:?}",
        out.warnings
    );
}

/// (d) The canonical nested-menu shape: a choice's own body ends in a
/// further `{? … }` choice point. That inner point is checked entirely on
/// its own (both its choices divert, so it's clean) — it must not make the
/// *outer* choice look like a dead end just because it isn't a `Divert`.
#[test]
fn e151_trailing_nested_choice_point_is_not_a_dead_end() {
    let source = "flow main() {\n  {?\n    * [Talk] {\n      Hello.\n      {?\n        \
                  * Ask -> combat\n        * Leave -> combat\n      }\n    }\n    \
                  * Fight -> combat\n  }\n}\n\nflow combat() {\n}\n";
    let out = compile_native("e151-nested-menu", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("must compile clean: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "a trailing nested choice point must not make the outer choice a dead end, got: {:?}",
        out.warnings
    );
}

/// The `[lints]` control plane, end to end (not just the unit-level
/// `effective_severity` mechanism `brink-analyzer::strict`'s own tests
/// already exercise generically over E014/E022): `E151 = "deny"` must
/// promote this specific lint from `Warning` to a real compile `Error`
/// through the actual pipeline.
#[test]
fn e151_denied_through_the_lints_control_plane_becomes_an_error() {
    let source = "flow main() {\n  {?\n    * Parry -> riposte\n    * [Dodge] {\n      \
                  You sidestep the blade.\n    }\n  }\n}\n\nflow riposte() {\n}\n";
    let options = AnalysisOptions {
        lints: LintPolicy {
            overrides: BTreeMap::from([("E151".to_owned(), LintLevel::Deny)]),
            deny_warnings: false,
        },
        ..AnalysisOptions::default()
    };
    let err = compile_native("e151-denied", source, options)
        .map(|_| ())
        .expect_err("`[lints] E151 = deny` must promote the lint to a compile error");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E151),
        "expected E151 among errors, got: {diags:?}"
    );
}

// ─── E152 (issue #582, companion to #580): `contains(m, needle)` static
// key-domain warning ──────────────────────────────────────────────────────
//
// Strict-mode-only (`brink_analyzer::contains_domain`'s own module doc) —
// every fixture here uses `strict_options()` explicitly. Warning-severity,
// so the compile still succeeds; `E152` shows up in `out.warnings`, never
// `out` (errors), unless promoted through `[lints]`.

/// The issue's own worked shape: a float needle against a statically
/// map-typed receiver can never match — always `false` at runtime.
#[test]
fn e152_float_needle_against_a_map_literal_is_flagged() {
    let source = "=== main ===\n~ temp x = contains(#{1: \"a\"}, 3.5)\n-> DONE\n";
    let out = compile(source, strict_options())
        .unwrap_or_else(|e| panic!("a Warning-severity lint must not fail the compile: {e:?}"));
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E152),
        "expected E152, got: {:?}",
        out.warnings
    );
}

/// A global `VAR`'s declaration-derived static type (issue #1540's
/// full-fidelity `Sig::value_ty`) makes the receiver provably a map and the
/// needle provably out of domain — the exact "far more cases" reach this
/// issue's re-scoping note called out.
#[test]
fn e152_global_map_var_with_out_of_domain_needle_is_flagged() {
    let source =
        "VAR scores = #{1: \"a\"}\n=== main ===\n~ temp x = contains(scores, #[1, 2])\n-> DONE\n";
    let out = compile(source, strict_options())
        .unwrap_or_else(|e| panic!("a Warning-severity lint must not fail the compile: {e:?}"));
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E152),
        "expected E152, got: {:?}",
        out.warnings
    );
}

/// An in-domain needle (`int`) is never flagged.
#[test]
fn e152_in_domain_needle_is_not_flagged() {
    let source = "=== main ===\n~ temp x = contains(#{1: \"a\"}, 2)\n-> DONE\n";
    let out = compile(source, strict_options()).unwrap_or_else(|e| panic!("must compile: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E152),
        "an in-domain needle must never fire E152, got: {:?}",
        out.warnings
    );
}

/// The precision-critical exclusion this pass' own module doc leads with:
/// an `Array` receiver has no key-domain restriction at all (structural
/// element containment against any type), so a float needle against one
/// must never be flagged.
#[test]
fn e152_array_receiver_is_never_flagged() {
    let source = "=== main ===\n~ temp x = contains(#[1, 2], 3.5)\n-> DONE\n";
    let out = compile(source, strict_options()).unwrap_or_else(|e| panic!("must compile: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E152),
        "an array receiver must never fire E152, got: {:?}",
        out.warnings
    );
}

/// An author-defined `contains` knot shadows the builtin (T1b-surface-spec
/// §5's shadowing ruling) — a resolved call is ordinary and never checked.
#[test]
fn e152_author_defined_contains_shadowing_the_builtin_is_not_flagged() {
    let source = "=== function contains(a: int, b: int) ===\n~ return true\n\
                  === main ===\n~ temp x = contains(#{1: \"a\"}, 3.5)\n-> DONE\n";
    let out = compile(source, strict_options()).unwrap_or_else(|e| panic!("must compile: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E152),
        "a shadowed `contains` must never fire E152, got: {:?}",
        out.warnings
    );
}

/// Gradual mode gets no static signal at all (the module doc's
/// inference-substrate note): the whole-project `InferenceResult` this pass
/// needs is only ever computed under `types = strict`. The runtime's own
/// total `false` return (#580) is the sole, correct residual.
#[test]
fn e152_gradual_mode_never_fires_the_static_warning() {
    let source = "=== main ===\n~ temp x = contains(#{1: \"a\"}, 3.5)\n-> DONE\n";
    let out = compile(source, gradual_options()).unwrap_or_else(|e| panic!("must compile: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E152),
        "gradual mode must never fire E152, got: {:?}",
        out.warnings
    );
}

/// The `[lints]` control plane, end to end: `E152 = "deny"` must promote
/// this specific warning from `Warning` to a real compile `Error` through
/// the actual pipeline — mirrors `e151_denied_through_the_lints_control_plane_becomes_an_error`.
#[test]
fn e152_denied_through_the_lints_control_plane_becomes_an_error() {
    let source = "=== main ===\n~ temp x = contains(#{1: \"a\"}, 3.5)\n-> DONE\n";
    let options = AnalysisOptions {
        lints: LintPolicy {
            overrides: BTreeMap::from([("E152".to_owned(), LintLevel::Deny)]),
            deny_warnings: false,
        },
        ..strict_options()
    };
    let err = compile(source, options)
        .map(|_| ())
        .expect_err("`[lints] E152 = deny` must promote the warning to a compile error");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E152),
        "expected E152 among errors, got: {diags:?}"
    );
}

// ─── E153 / E154 / E155 (issue #1161): the `@[allow(Exxx, …)]` source-level
// suppression annotation ─────────────────────────────────────────────────
//
// Native-only (the `@[…]` channel's `allow` tenant lowers in
// `brink_ir::hir::lower_native::annotation`), so every fixture here uses the
// disk-based `compile_native` helper, like the `or`-coalescing pair above.
//
// The E151 dead-end lint is the workhorse warning: it is native, it fires
// from a compact fixture, and `e151_denied_through_the_lints_control_plane_
// becomes_an_error` above already pins its `[lints]` behaviour, so these
// tests can state the *interaction* ruling against a known baseline.

/// The E151 fixture from `e151_denied_…` above, parameterised on what sits
/// above `flow main()`. Without an annotation it warns; with the right
/// `@[allow]` it does not.
fn e151_source(annotation: &str) -> String {
    format!(
        "{annotation}flow main() {{\n  {{?\n    * Parry -> riposte\n    * [Dodge] {{\n      \
         You sidestep the blade.\n    }}\n  }}\n}}\n\nflow riposte() {{\n}}\n"
    )
}

/// Baseline, so the suppression tests below cannot pass vacuously: with no
/// annotation this exact fixture *does* warn.
#[test]
fn allow_baseline_the_unannotated_fixture_warns() {
    let out = compile_native(
        "allow-baseline",
        &e151_source(""),
        AnalysisOptions::default(),
    )
    .unwrap_or_else(|e| panic!("must compile: {e:?}"));
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E151),
        "the unannotated fixture must warn, got: {:?}",
        out.warnings
    );
}

/// The headline behaviour: `@[allow(E151)]` above the declaration removes
/// the warning from the compile output entirely.
#[test]
fn allow_annotation_suppresses_the_warning_in_its_scope() {
    let out = compile_native(
        "allow-suppresses",
        &e151_source("@[allow(E151)]\n"),
        AnalysisOptions::default(),
    )
    .unwrap_or_else(|e| panic!("must compile: {e:?}"));
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "`@[allow(E151)]` must suppress the lint, got: {:?}",
        out.warnings
    );
}

/// Scoping is real, not a file-wide switch: the same annotation on the
/// *sibling* declaration leaves `main`'s warning standing.
#[test]
fn allow_on_a_sibling_declaration_does_not_suppress() {
    let source = "flow main() {\n  {?\n    * Parry -> riposte\n    * [Dodge] {\n      \
                  You sidestep the blade.\n    }\n  }\n}\n\n@[allow(E151)]\nflow riposte() {\n}\n";
    let out = compile_native("allow-sibling", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("must compile: {e:?}"));
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E151),
        "an `@[allow]` on another declaration must not reach this one, got: {:?}",
        out.warnings
    );
}

/// **The ruling (issue #1161's open question (b)): a source-level `allow`
/// beats a project-level `deny`.**
/// `e151_denied_through_the_lints_control_plane_becomes_an_error` above
/// proves this exact fixture fails to compile under `[lints] E151 = "deny"`;
/// adding `@[allow(E151)]` makes it compile again. The annotation is the
/// more specific, deliberately-authored, reviewable statement, and
/// `brink.toml` has no way to name one declaration.
#[test]
fn a_source_allow_beats_a_project_lints_deny() {
    let options = AnalysisOptions {
        lints: LintPolicy {
            overrides: BTreeMap::from([("E151".to_owned(), LintLevel::Deny)]),
            deny_warnings: false,
        },
        ..AnalysisOptions::default()
    };
    let out = compile_native(
        "allow-beats-deny",
        &e151_source("@[allow(E151)]\n"),
        options,
    )
    .unwrap_or_else(|e| panic!("`@[allow(E151)]` must survive `[lints] E151 = deny`, got: {e:?}"));
    // A successful compile already proves no `Error` survived (the pipeline
    // returns `CompileError::Diagnostics` otherwise); this pins that the
    // lint is not merely demoted back to a warning either.
    assert!(
        out.warnings.iter().all(|d| d.code != DiagnosticCode::E151),
        "the suppressed lint must appear nowhere, got: {:?}",
        out.warnings
    );
}

/// The same ruling against the blanket knob: `deny-warnings = true` is the
/// `-D warnings` equivalent, and a source `allow` still wins.
#[test]
fn a_source_allow_beats_deny_warnings() {
    let options = AnalysisOptions {
        lints: LintPolicy {
            overrides: BTreeMap::new(),
            deny_warnings: true,
        },
        ..AnalysisOptions::default()
    };
    compile_native(
        "allow-beats-deny-warnings",
        &e151_source("@[allow(E151)]\n"),
        options,
    )
    .unwrap_or_else(|e| {
        panic!("`@[allow(E151)]` must survive `[lints] deny-warnings = true`, got: {e:?}")
    });
}

/// A misspelled code is `E153` and fails the compile — issue #1161's open
/// question (c). A typo'd suppression that silently did nothing would be the
/// worst outcome the directive could have.
#[test]
fn e153_unknown_code_in_an_allow_annotation() {
    let err = compile_native(
        "e153-unknown-code",
        &e151_source("@[allow(E1511)]\n"),
        AnalysisOptions::default(),
    )
    .map(|_| ())
    .expect_err("a misspelled code in `@[allow(…)]` must be a hard error");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E153),
        "expected E153 among errors, got: {diags:?}"
    );
}

/// `E154`: errors are not suppressible — issue #1161's open question (a).
/// `E103` (effects exceedance) is a real `Error`-severity code, so naming it
/// is rejected rather than silently granting a way to ship broken code.
#[test]
fn e154_non_suppressible_code_in_an_allow_annotation() {
    let err = compile_native(
        "e154-error-code",
        &e151_source("@[allow(E103)]\n"),
        AnalysisOptions::default(),
    )
    .map(|_| ())
    .expect_err("an error-severity code in `@[allow(…)]` must be a hard error");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E154),
        "expected E154 among errors, got: {diags:?}"
    );
}

/// `E155`: the annotation parses but names no code at all.
#[test]
fn e155_allow_annotation_with_no_codes() {
    let err = compile_native(
        "e155-empty-allow",
        &e151_source("@[allow()]\n"),
        AnalysisOptions::default(),
    )
    .map(|_| ())
    .expect_err("`@[allow()]` must be a hard error");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E155),
        "expected E155 among errors, got: {diags:?}"
    );
}

// ─── E157 (issue #1674): anonymous-container state lint, wired end to end ──
//
// `brink_analyzer::check_anonymous_stateful` itself is unit-tested directly
// in `brink-analyzer/src/anonymous_stateful.rs`; these fixtures instead pin
// the `brink-db` wiring (`lower_file`/`lower_native_file`, the same seam
// `E151`/`E156` use) through the real pipeline — the reachability the PR
// body claims — plus the `[lints]` deny-promotion `E151` already covers
// above, mirrored for the first `Info`-base code.

/// Ink frontend: an unlabeled once-only choice reaches `out.warnings` at
/// `E157`'s default `Info` severity with no config needed — `Info`, like
/// `Warning`, partitions into `warnings`, never `errors`
/// (`partition_diagnostics` only routes `Severity::Error` there).
#[test]
fn e157_ink_unlabeled_once_only_choice_reaches_out_warnings() {
    let source = "=== main ===\n* [pick] -> DONE\n";
    let out = compile(source, default_options())
        .unwrap_or_else(|e| panic!("an Info-severity lint must not fail the compile: {e:?}"));
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E157),
        "expected E157, got: {:?}",
        out.warnings
    );
}

/// Native frontend: the same shape, through `lower_native_file`.
#[test]
fn e157_native_unlabeled_once_only_choice_reaches_out_warnings() {
    let source = "flow main() {\n  {?\n    * [Look] You look around.\n  }\n}\n";
    let out = compile_native("e157-native-unlabeled", source, AnalysisOptions::default())
        .unwrap_or_else(|e| panic!("an Info-severity lint must not fail the compile: {e:?}"));
    assert!(
        out.warnings.iter().any(|d| d.code == DiagnosticCode::E157),
        "expected E157, got: {:?}",
        out.warnings
    );
}

/// The `[lints]` control plane, end to end: `E157 = "deny"` must promote
/// this lint from its `Info` default to a real compile `Error` through the
/// actual pipeline — the same proof `e151_denied_through_the_lints_control_
/// plane_becomes_an_error` gives for a `Warning`-base code, substantiating
/// that `effective_severity`/`validate_lint_code`'s widening past
/// `Warning`-only actually reaches `E157` and isn't merely accepted by the
/// config parser.
#[test]
fn e157_denied_through_the_lints_control_plane_becomes_an_error() {
    let source = "flow main() {\n  {?\n    * [Look] You look around.\n  }\n}\n";
    let options = AnalysisOptions {
        lints: LintPolicy {
            overrides: BTreeMap::from([("E157".to_owned(), LintLevel::Deny)]),
            deny_warnings: false,
        },
        ..AnalysisOptions::default()
    };
    let err = compile_native("e157-denied", source, options)
        .map(|_| ())
        .expect_err("`[lints] E157 = deny` must promote the lint to a compile error");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E157),
        "expected E157 among errors, got: {diags:?}"
    );
}
