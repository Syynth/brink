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
//! - E060 — emitted only when codegen rejects a `Program` violating an
//!   invariant an earlier stage guarantees (a compiler bug by definition);
//!   not constructible from source.
//! - E072 — RETIRED (TM-4c, #666), documented as reserved-not-reused.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::collections::HashMap;

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
    let out = compile(source, default_options()).expect("duplicate knot is a warning");
    // The warning points at the *second* definition's name.
    assert_code_at_nth(&out.warnings, DiagnosticCode::E022, source, "== k ==", 1);
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

// ─── B1b the `as` binding (issue #1475): E140/E141/E142 ────────────────
//
// Native-only, same reasoning as the E066 block above — an `AS_BINDING`
// node exists only in the native grammar, so these reuse `compile_native`.

/// E140 — the v1 whole-condition restriction, caught at HIR lowering when
/// the binding sits on top of a `&&` composition. (The mirror spelling, an
/// operator *after* the binding, is a parse error instead; see
/// `brink-syntax-native`'s `parser::tests::statement`.)
#[test]
fn e140_as_binding_over_a_boolean_composition() {
    let source = "flow main() ~{
  if true && some(1) as n {
    return n;
  }
}
";
    let err = compile_native("as-composed", source, native_strict_options())
        .map(|_| ())
        .expect_err("`as` over a `&&` composition must fail");
    let diags = errors_of(err);
    assert!(
        diags.iter().any(|d| d.code == DiagnosticCode::E140),
        "expected E140, got: {diags:?}"
    );
}

/// E141 — guard-`as` is ruled but rides the `.inkb` v6 Choice record, so it
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
        diags.iter().any(|d| d.code == DiagnosticCode::E141),
        "expected E141, got: {diags:?}"
    );
}

/// E142 — the binding unwraps `Option[T]`; a statically classifiable
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
        diags.iter().any(|d| d.code == DiagnosticCode::E142),
        "expected E142, got: {diags:?}"
    );
}

/// E143 — the binding is immutable by ruling. Every write shape resolves
/// its target through the same LIR choke point, so all three are refused:
/// plain assignment, compound assignment, and an in-place mutator.
#[test]
fn e143_as_binding_is_immutable() {
    for (suffix, write) in [
        ("as-imm-assign", "n = 1;"),
        ("as-imm-compound", "n += 1;"),
        ("as-imm-mutator", "pop(n);"),
    ] {
        let source = format!("flow main() ~{{\n  if some(1) as n {{\n    {write}\n  }}\n}}\n");
        let err = compile_native(suffix, &source, native_strict_options())
            .map(|_| ())
            .unwrap_err();
        let diags = errors_of(err);
        assert!(
            diags.iter().any(|d| d.code == DiagnosticCode::E143),
            "expected E143 for `{write}`, got: {diags:?}"
        );
    }
}
