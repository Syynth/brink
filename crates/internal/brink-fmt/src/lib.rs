//! Source code formatter for inkle's ink narrative scripting language.
//!
//! Parses the input with `brink_syntax::parse`, lowers to HIR for structural
//! nesting information, then walks the CST to classify each source line and
//! reformats according to consistent rules. HIR provides the correct
//! indentation depth for every source line.
//!
//! The pipeline is three stages, one module each:
//! - [`depth`] — build a line → indentation-depth map from the lowered HIR.
//! - [`classify`] — walk the CST and tag every physical line with a
//!   [`classify::LineKind`], carrying whatever CST node the renderer needs.
//! - [`render`] — walk the classified lines and emit the formatted source.
//!
//! [`format`] is a thin orchestrator that runs the three stages in order.

use brink_ir::hir;

mod classify;
mod depth;
mod render;

// ── Public API ──────────────────────────────────────────────────────

/// How to indent nested constructs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IndentStyle {
    Tabs,
    Spaces(u32),
}

/// Configuration for the formatter.
#[derive(Debug, Clone)]
pub struct FormatConfig {
    pub indent: IndentStyle,
}

impl Default for FormatConfig {
    /// The indent width comes from [`brink_project_config::DEFAULT_INDENT`],
    /// never from a constant of this crate's own (ruled 2026-08-27,
    /// "everything that indents reads the same setting").
    ///
    /// This crate used to default to two spaces while the editor's
    /// `indentUnit` used four — the exact disagreement the ruling exists to
    /// kill, and one an author could not diagnose, since a formatter writing
    /// two spaces under guides drawn every four looks like a rendering
    /// glitch rather than a config mismatch. Any component reintroducing its
    /// own default recreates that bug.
    fn default() -> Self {
        Self {
            indent: IndentStyle::Spaces(u32::from(brink_project_config::DEFAULT_INDENT)),
        }
    }
}

impl FormatConfig {
    /// Build from a parsed `brink.toml`, applying [`Self::default`]'s width
    /// when `[project] indent` is unset.
    ///
    /// The character is not configurable yet — `[project] indent` is a
    /// SIZE, and tabs-vs-spaces is deliberately a separate question
    /// (#3149's own body says so), so this always produces
    /// [`IndentStyle::Spaces`].
    #[must_use]
    pub fn from_project_config(config: &brink_project_config::ProjectConfig) -> Self {
        match config.indent {
            Some(width) => Self {
                indent: IndentStyle::Spaces(u32::from(width)),
            },
            None => Self::default(),
        }
    }
}

/// Format an entire ink source string. Returns the formatted source.
#[must_use]
pub fn format(source: &str, config: &FormatConfig) -> String {
    let parse = brink_syntax::parse(source);
    let root = parse.syntax();

    // Lower to HIR to get structural nesting information.
    let file_id = brink_ir::FileId(0);
    let tree = parse.tree();
    let (hir_file, _, _) = hir::lower(file_id, &tree);

    // Build a depth map from HIR: line number → indentation depth.
    let line_starts = depth::build_line_starts(source);
    let depth_map = depth::build_depth_map(source, &line_starts, &hir_file);

    let lines = classify::classify_lines(source, &root, parse.errors(), &depth_map);
    render::render(source, &lines, config)
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
#[expect(clippy::panic)]
mod tests {
    use super::*;
    use brink_syntax::syntax_kind::SyntaxKind;

    /// Two-space indent, pinned rather than taken from
    /// [`FormatConfig::default`].
    ///
    /// The tests below are about STRUCTURE — "a knot body is one level in",
    /// "choice content is one level inside its choice" — and merely happen
    /// to spell the width out in their expected strings. Reading the
    /// default here would make all 17 of them restate it, so a ruling on
    /// the default (2026-08-27 moved it from 2 to 4) would churn every one
    /// of them while proving nothing about the default itself.
    ///
    /// The default has its own test instead:
    /// `the_default_indent_comes_from_the_project_config`.
    fn fmt(source: &str) -> String {
        format(
            source,
            &FormatConfig {
                indent: IndentStyle::Spaces(2),
            },
        )
    }

    #[test]
    fn the_default_indent_comes_from_the_project_config() {
        // Asserted against the SHIPPING constant, not a literal 4 — a test
        // that restates the value cannot detect that value being wrong, it
        // only proves this crate agrees with its own copy. Ruled
        // 2026-08-27: there is one place the width comes from.
        assert_eq!(
            FormatConfig::default().indent,
            IndentStyle::Spaces(u32::from(brink_project_config::DEFAULT_INDENT)),
            "the formatter must not keep an indent default of its own"
        );
    }

    #[test]
    fn from_project_config_honours_an_explicit_indent() {
        let (config, _) =
            brink_project_config::parse_str("[project]\nindent = 8\n").expect("valid config");
        assert_eq!(
            FormatConfig::from_project_config(&config).indent,
            IndentStyle::Spaces(8)
        );
    }

    #[test]
    fn from_project_config_falls_back_to_the_default_when_unset() {
        let (config, _) = brink_project_config::parse_str("[project]\nentry = \"main.ink\"\n")
            .expect("valid config");
        assert_eq!(
            FormatConfig::from_project_config(&config).indent,
            FormatConfig::default().indent
        );
    }

    fn fmt_tabs(source: &str) -> String {
        format(
            source,
            &FormatConfig {
                indent: IndentStyle::Tabs,
            },
        )
    }

    #[test]
    fn trailing_whitespace_stripped() {
        let input = "Hello world   \nSecond line\t\n";
        let result = fmt(input);
        for line in result.lines() {
            assert_eq!(line, line.trim_end());
        }
    }

    #[test]
    fn knot_header_normalized() {
        assert_eq!(fmt("===myknot===\n"), "=== myknot ===\n");
        assert_eq!(fmt("===  myknot  ===\n"), "=== myknot ===\n");
        assert_eq!(fmt("=== myknot ===\n"), "=== myknot ===\n");
    }

    #[test]
    fn function_knot_header() {
        let input = "=== function  add(a, b) ===\n~ return a + b\n";
        let result = fmt(input);
        assert!(result.starts_with("=== function add(a, b) ===\n"));
    }

    #[test]
    fn stitch_header_normalized() {
        // Standalone stitch at root level — parser promotes to knot, but the
        // CST node is still STITCH_HEADER, so the formatter uses stitch format.
        assert_eq!(fmt("=  mystitch\n"), "= mystitch\n");
        // Inside a knot, stitch headers are indented.
        let input = "=== myknot ===\n= mystitch\nContent\n";
        let result = fmt(input);
        assert!(result.contains("  = mystitch\n"));
    }

    #[test]
    fn choice_formatting() {
        let input = "*  Hello\n";
        let result = fmt(input);
        assert_eq!(result, "* Hello\n");
    }

    #[test]
    fn gather_formatting() {
        let input = "-  gathered\n";
        let result = fmt(input);
        assert_eq!(result, "- gathered\n");
    }

    #[test]
    fn logic_line_formatting() {
        let input = "~   x = 5\n";
        let result = fmt(input);
        assert_eq!(result, "~ x = 5\n");
    }

    #[test]
    fn blank_lines_collapsed() {
        let input = "Hello\n\n\n\nWorld\n";
        let result = fmt(input);
        assert_eq!(result, "Hello\n\nWorld\n");
    }

    #[test]
    fn blank_before_knot() {
        let input = "Hello\n=== knot ===\n";
        let result = fmt(input);
        assert_eq!(result, "Hello\n\n=== knot ===\n");
    }

    #[test]
    fn single_trailing_newline() {
        let input = "Hello\n\n\n";
        let result = fmt(input);
        assert!(result.ends_with('\n'));
        assert!(!result.ends_with("\n\n"));
    }

    #[test]
    fn declaration_no_indent() {
        let input = "VAR x = 5\nCONST y = 10\n";
        let result = fmt(input);
        assert!(result.contains("VAR x = 5\n"));
        assert!(result.contains("CONST y = 10\n"));
    }

    #[test]
    fn empty_input() {
        assert_eq!(fmt(""), "");
    }

    #[test]
    fn comment_preserved() {
        let input = "// This is a comment\nHello\n";
        let result = fmt(input);
        assert!(result.contains("// This is a comment\n"));
    }

    #[test]
    fn content_trimmed() {
        let input = "  Hello world  \n";
        let result = fmt(input);
        assert_eq!(result, "Hello world\n");
    }

    #[test]
    fn choice_with_brackets() {
        let input = "*  \"What's that?\"[he asked.]\n";
        let result = fmt(input);
        assert_eq!(result, "* \"What's that?\"[he asked.]\n");
    }

    #[test]
    fn sticky_choice() {
        let input = "+  Sticky option\n";
        let result = fmt(input);
        assert_eq!(result, "+ Sticky option\n");
    }

    #[test]
    fn include_declaration() {
        let input = "INCLUDE other.ink\n";
        let result = fmt(input);
        assert_eq!(result, "INCLUDE other.ink\n");
    }

    #[test]
    fn import_qualified_normalizes_spacing() {
        assert_eq!(fmt("IMPORT   quest_3\n"), "IMPORT quest_3\n");
    }

    #[test]
    fn import_bare_normalizes_spacing() {
        assert_eq!(
            fmt("IMPORT {  ambush ,  guard_talk  AS gt  } FROM quest_3\n"),
            "IMPORT { ambush, guard_talk AS gt } FROM quest_3\n"
        );
    }

    #[test]
    fn import_block_stays_at_column_zero() {
        // A leading IMPORT block below a `#@module` header keeps col-0 depth;
        // the knot body still indents. (fmt inserts a blank line before the
        // knot header — existing behavior.)
        let input =
            "#@module(town)\nIMPORT { ambush } FROM quest_3\nIMPORT quest_4\n=== hub ===\nHi.\n";
        assert_eq!(
            fmt(input),
            "#@module(town)\nIMPORT { ambush } FROM quest_3\nIMPORT quest_4\n\n=== hub ===\n  Hi.\n"
        );
    }

    #[test]
    fn import_formatting_is_idempotent() {
        let once = fmt("IMPORT {  a , b AS c } FROM  m\n");
        assert_eq!(fmt(&once), once);
        assert_eq!(once, "IMPORT { a, b AS c } FROM m\n");
    }

    #[test]
    fn malformed_import_is_left_verbatim() {
        // No closing brace — bail to verbatim rather than corrupt it.
        assert_eq!(fmt("IMPORT { a FROM m\n"), "IMPORT { a FROM m\n");
    }

    #[test]
    fn knot_body_indented() {
        let input = "=== myknot ===\nHello from knot\n* A choice\n";
        let result = fmt(input);
        assert_eq!(result, "=== myknot ===\n  Hello from knot\n  * A choice\n");
    }

    #[test]
    fn stitch_in_knot_indented() {
        let input = "=== myknot ===\n= mystitch\nContent here\n";
        let result = fmt(input);
        // Stitch header at depth 1, content at depth 2.
        assert!(result.contains("  = mystitch\n"));
        assert!(result.contains("    Content here\n"));
    }

    #[test]
    fn choice_content_indented_in_knot() {
        let input = "=== myknot ===\n* Choice\n  After choice\n";
        let result = fmt(input);
        // Choice at depth 1 (knot body), content after choice at depth 2.
        assert_eq!(result, "=== myknot ===\n  * Choice\n    After choice\n");
    }

    #[test]
    fn idempotent() {
        let input =
            "=== knot ===\n\n  Hello world\n\n  * Choice one\n  * Choice two\n\n  - Gathered\n";
        let first = fmt(input);
        let second = fmt(&first);
        assert_eq!(first, second, "formatting should be idempotent");
    }

    // ── TM-2 inline type annotations (docs/typed-mode-spec.md §3) ───────
    //
    // The knot-header/declaration/logic-line renderers are single-line
    // token-collapsing passes over the raw physical-line text (see
    // `format_knot_header`/`format_logic`/`LineKind::Declaration`'s own
    // docs) — a `:` is just another non-whitespace token to them, so
    // annotations format "for free" through the exact same code path
    // exercised for every other knot header/declaration/logic line. These
    // tests pin that down explicitly rather than relying on it implicitly.

    #[test]
    fn param_and_return_type_annotations_canonicalize() {
        // #642: type annotations should render as `name: type` (no space before,
        // one space after), regardless of source spacing. Mixed annotation
        // spacing: `hp:int` (no space), `amount:  int` (multiple spaces).
        assert_eq!(
            fmt("===function heal(hp:int,amount:  int)  :  int===\n~ return hp\n"),
            "=== function heal(hp: int,amount: int): int ===\n  ~ return hp\n"
        );
    }

    #[test]
    fn stitch_return_type_annotation_canonicalizes_too() {
        // #1509: a stitch header's `: type` return clause is a single-line
        // token-collapsing pass same as a knot header's (see the module
        // note above) — no dedicated stitch formatting code needed, but
        // this pins the "for free" claim down for the stitch case
        // specifically.
        assert_eq!(
            fmt("=== camp ===\n=fire(logs:int)  :  int\n~ return logs\n"),
            "=== camp ===\n\n  = fire(logs: int): int\n    ~ return logs\n"
        );
    }

    #[test]
    fn var_type_annotation_formats_verbatim_modulo_trailing_whitespace() {
        assert_eq!(fmt("VAR gold: int = 100   \n"), "VAR gold: int = 100\n");
    }

    #[test]
    fn const_type_annotation_formats_verbatim_modulo_trailing_whitespace() {
        // #641: CONST mirrors VAR — same single-line declaration renderer,
        // no dedicated formatting code.
        assert_eq!(
            fmt("CONST speed: float = 0.5   \n"),
            "CONST speed: float = 0.5\n"
        );
    }

    #[test]
    fn temp_ascription_canonicalizes_annotations() {
        // Issue #858: before the single-line `~ expr` retokenize fix, this
        // line's *inner* spacing (`temp   name:string=who`) passed through
        // untouched — only the outer `~` prefix got trimmed. Now it goes
        // through the same `join_token_text` joiner multi-line `~ { … }`
        // block statements use: runs of existing whitespace collapse to one
        // space (`temp   name` -> `temp name`).
        // Issue #642: type annotations also get canonicalized to `name: type`
        // (no space before, one space after).
        assert_eq!(
            fmt("=== knot ===\n~   temp   name:string=who\n"),
            "=== knot ===\n  ~ temp name: string=who\n"
        );
    }

    #[test]
    fn type_annotations_are_idempotent() {
        for input in [
            // Already formatted correctly
            "=== function heal(hp: int,amount: int): int ===\n~ return hp\n",
            // Missing spaces after colons
            "=== function heal(hp:int,amount:int):int ===\n~ return hp\n",
            // VAR with canonical spacing
            "VAR gold: int = 100\n",
            // VAR without spaces after colon
            "VAR gold:int=100\n",
            // Temp with canonical spacing
            "=== knot ===\n~ temp name: string=who\n",
            // Temp without spaces after colon
            "=== knot ===\n~temp name:string=who\n",
            // Complex types
            "VAR w: List<Weathers> = 0\nVAR m: Map<string,int> = 0\n",
            // Function type
            "VAR cb: fn(int,int): bool = 0\n",
            // CONST
            "CONST speed: float = 0.5\n",
        ] {
            let first = fmt(input);
            let second = fmt(&first);
            assert_eq!(
                first, second,
                "type-annotated formatting should be idempotent for {input:?}"
            );
        }
    }

    #[test]
    fn type_annotations_mixed_spacing_fixture() {
        // Comprehensive fixture with mixed annotation spacing: no spaces,
        // single spaces, and multiple spaces (PR #640 compatibility test).
        let input = "\
=== function greet(name:string,age:  int):string ===
  VAR greeting:string=\"Hello\"
  CONST max_age:int=120
  ~ temp result:string=greeting

=== helper(x: int, y:int ): int ===
  ~ return x + y

STRUCT Point=#{x:int,y: float}
";
        let output = fmt(input);
        // Re-formatting should be idempotent
        let output2 = fmt(&output);
        assert_eq!(
            output, output2,
            "mixed spacing fixture should be idempotent"
        );
    }

    // ── #642 review fix: string-literal values must be byte-preserved ───
    // The declaration path's colon canonicalization is now retokenized
    // through `join_token_text` (like logic lines and struct fields)
    // instead of the character-based, string-unaware `collapse_whitespace`,
    // which previously mutated string contents containing a colon or
    // internal whitespace (regression caught in review of PR #971).

    #[test]
    fn var_string_initializer_with_colon_no_following_space_is_preserved() {
        // Character-based collapse_whitespace saw the `:` inside the string
        // and inserted a space after it: `"time 12:30"` -> `"time 12: 30"`.
        assert_eq!(
            fmt("VAR msg = \"time 12:30\"\n"),
            "VAR msg = \"time 12:30\"\n"
        );
    }

    #[test]
    fn const_string_initializer_url_colon_is_preserved() {
        // Same corruption broke URLs: `"http://x.com"` -> `"http: //x.com"`.
        assert_eq!(
            fmt("CONST url = \"http://x.com\"\n"),
            "CONST url = \"http://x.com\"\n"
        );
    }

    #[test]
    fn var_string_initializer_internal_multiple_spaces_are_preserved() {
        // Character-based collapse_whitespace ran its whitespace-run
        // collapsing over the whole line, including inside the string
        // literal: `"Monsieur  Fogg"` -> `"Monsieur Fogg"`.
        assert_eq!(
            fmt("VAR name = \"Monsieur  Fogg\"\n"),
            "VAR name = \"Monsieur  Fogg\"\n"
        );
    }

    #[test]
    fn var_string_initializer_colon_no_space_variant_is_preserved() {
        assert_eq!(
            fmt("VAR title = \"Chapter:One\"\n"),
            "VAR title = \"Chapter:One\"\n"
        );
    }

    // ── #984: raw-text declaration fallback proved unreachable, removed ─
    // `render()`'s `LineKind::Declaration` VAR/CONST/LIST arm used to fall
    // back to a character-based `collapse_whitespace` pass whenever
    // `line.cst_node` was `None`. That fallback is now an `unreachable!()`:
    // `classify_node`'s `VAR_DECL | CONST_DECL | LIST_DECL` arm sets `kind`
    // and `cst_node` together, unconditionally, and the parser always
    // wraps a `VAR`/`CONST`/`LIST` line in its node (bracketed by
    // `start_node`/`finish_node`) even when the body has a parse error —
    // so `cst_node` is never `None` for these lines. These cases exercise
    // malformed declarations specifically to prove `fmt()` never panics
    // (i.e. never hits the `unreachable!()`) even on error-recovered input.

    #[test]
    fn malformed_var_decl_missing_initializer_does_not_panic() {
        // `VAR x =` with nothing after `=` (still followed by a newline) is
        // a parse error, but the parser still emits a VAR_DECL node — the
        // CST-retokenizing path must handle it without panicking.
        let _ = fmt("VAR x =\n");
    }

    #[test]
    fn malformed_const_decl_missing_value_does_not_panic() {
        let _ = fmt("CONST x =\n");
    }

    #[test]
    fn malformed_list_decl_missing_members_does_not_panic() {
        let _ = fmt("LIST x =\n");
    }

    #[test]
    fn malformed_var_decl_with_string_literal_does_not_corrupt_and_does_not_panic() {
        // Malformed trailing content after a valid string-literal
        // initializer: the CST path must still preserve the string
        // byte-for-byte and must not panic.
        let output = fmt("VAR msg = \"time 12:30\" +\n");
        assert!(
            output.contains("\"time 12:30\""),
            "string literal must survive malformed declaration formatting: {output:?}"
        );
    }

    // ── T1b `~ { … }` blocks: indentation-aware reformatting ────────────
    // (docs/t1b-surface-spec.md §2, ruled acceptance criteria on #573)

    #[test]
    fn block_at_root_reindents_nesting() {
        // Flat, unindented input — the formatter must reindent it, not
        // pass it through (that was the superseded T1b-1 placeholder, #569).
        let input = "~ {\ntemp x = 0\nif x > 0 {\nx = x - 1\n}\n}\n";
        let expected = "~ {\n    temp x = 0\n    if x > 0 {\n        x = x - 1\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_with_while_and_for_reindents_nesting() {
        let input =
            "~ {\nwhile x > 0 {\nx = x - 1\n}\nfor item in list {\ntotal = total + item\n}\n}\n";
        let expected = "~ {\n    while x > 0 {\n        x = x - 1\n    }\n    for item in list {\n        total = total + item\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_collapses_messy_spacing_and_reindents() {
        // Ragged original indentation/spacing is normalized: 4 spaces per
        // nesting level, single space between tokens — but token content
        // (identifiers, literals) is never altered.
        let input = "~ {\n    temp x   =   0  \nif x > 0 {\n\tx = x - 1\n}\n}\n";
        let expected = "~ {\n    temp x = 0\n    if x > 0 {\n        x = x - 1\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_preserves_string_literal_internal_spacing() {
        // `join_token_text` must only collapse whitespace *between* tokens —
        // never characters inside a STRING_TEXT token's own content.
        let input = "~ {\ntemp msg = \"hello   world\"\n}\n";
        let expected = "~ {\n    temp msg = \"hello   world\"\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_inside_knot_indents_relative_to_knot() {
        // The `~ {` line's own depth still comes from the surrounding
        // structure (knot body = depth 1, 2-space `FormatConfig::default()`
        // step) — the block's *internal* nesting is a separate, fixed
        // 4-space step layered on top of that outer indent.
        let input =
            "=== start ===\nContent\n~ {\ntemp x = 0\nif x > 0 {\nx = x - 1\n}\n}\nMore content\n";
        let expected = "=== start ===\n  Content\n  ~ {\n      temp x = 0\n      if x > 0 {\n          x = x - 1\n      }\n  }\n  More content\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn single_line_logic_line_still_reformatted() {
        // Only the T1b multi-line block form goes through the block
        // renderer — ordinary `~` logic lines keep normal behavior.
        assert_eq!(fmt("~   x = 5\n"), "~ x = 5\n");
    }

    #[test]
    fn single_line_logic_collapses_messy_operator_spacing() {
        // Issue #858: a single-line `~ expr` retokenizes through the CST
        // now, same as a `~ { … }` block statement — ragged whitespace
        // around operators collapses to one space, matching
        // `block_collapses_messy_spacing_and_reindents`'s expectation for
        // the equivalent block-form input (`temp x   =   0`).
        assert_eq!(fmt("~ temp x   =   0\n"), "~ temp x = 0\n");
        assert_eq!(fmt("~ x   =   x  +  1\n"), "~ x = x + 1\n");
    }

    #[test]
    fn single_line_logic_ref_path_normalizes_like_block_form() {
        // Issue #858: the `ref lvalue-path` zero-space convention around
        // `.`/`[`/`]` (T1e, issue #850) already applied to `~ { … }` block
        // statements (`block_ref_path_mixed_field_and_index_argument_normalizes_spacing`)
        // now applies to the single-line form too, since both render
        // through the same `join_token_text` joiner.
        assert_eq!(
            fmt("~ heal(ref  party[ leader ] . hp,   5)\n"),
            "~ heal(ref party[leader].hp, 5)\n"
        );
    }

    #[test]
    fn single_line_logic_retokenize_is_idempotent() {
        let input = "~   temp   name : string  =  who\n";
        let once = fmt(input);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn single_line_logic_preserves_string_literal_internal_spacing() {
        // The joiner emits token text byte-for-byte — a string literal's
        // own internal spacing must never be touched, single-line or block
        // (mirrors `block_preserves_string_literal_internal_spacing`).
        assert_eq!(
            fmt("~   temp msg   =   \"hello   world\"\n"),
            "~ temp msg = \"hello   world\"\n"
        );
    }

    #[test]
    fn block_does_not_disturb_surrounding_lines() {
        let input = "Before\n~ {\ntemp x = 0\n}\nAfter\n";
        let expected = "Before\n~ {\n    temp x = 0\n}\nAfter\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_one_statement_per_line_even_when_source_is_compact() {
        // "One statement per line" is a hard rule — even a single-physical-
        // line block body gets expanded to one statement per rendered line.
        let input = "~ {\ntemp x = 0\nx = x + 1\nx = x + 1\n}\n";
        let expected = "~ {\n    temp x = 0\n    x = x + 1\n    x = x + 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_else_if_chain_braces_stay_on_statement_line() {
        let input = "~ {\nif a {\nx = 1\n} else if b {\nx = 2\n} else {\nx = 3\n}\n}\n";
        let expected = "~ {\n    if a {\n        x = 1\n    } else if b {\n        x = 2\n    } else {\n        x = 3\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_leading_comment_attaches_to_following_statement_depth() {
        let input = "~ {\n// explain x\ntemp x = 0\nif x > 0 {\n// explain the decrement\nx = x - 1\n}\n}\n";
        let expected = "~ {\n    // explain x\n    temp x = 0\n    if x > 0 {\n        // explain the decrement\n        x = x - 1\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_trailing_comment_stays_on_its_statement_line() {
        let input = "~ {\ntemp x = 0 // starts at zero\nx = x + 1\n}\n";
        let expected = "~ {\n    temp x = 0 // starts at zero\n    x = x + 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_trailing_comment_before_closing_brace() {
        // A comment with nothing following it in the block stays at the
        // block's own depth (there is no "following statement" to attach
        // to), not the parent's depth of the closing `}`.
        let input = "~ {\ntemp x = 0\n// done\n}\n";
        let expected = "~ {\n    temp x = 0\n    // done\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_blank_lines_preserved_between_statements() {
        let input = "~ {\ntemp x = 0\n\ntemp y = 1\n}\n";
        let expected = "~ {\n    temp x = 0\n\n    temp y = 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_multiple_consecutive_blank_lines_collapse_to_one() {
        let input = "~ {\ntemp x = 0\n\n\n\ntemp y = 1\n}\n";
        let expected = "~ {\n    temp x = 0\n\n    temp y = 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_blank_line_before_closing_brace_preserved() {
        let input = "~ {\ntemp x = 0\n\n}\n";
        let expected = "~ {\n    temp x = 0\n\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_idempotent_after_reindenting_messy_input() {
        let input = "~ {\n  temp x=0\n  if x>0{\nx = x - 1\n     }\n\n\n  temp y = 1\n}\n";
        let first = fmt(input);
        let second = fmt(&first);
        assert_eq!(first, second, "block formatting should be idempotent");
    }

    #[test]
    fn block_break_continue_lossless() {
        let input = "~ {\ntemp i = 0\nwhile true {\ni = i + 1\nif i > 10 {\nbreak\n}\nif i mod 2 == 0 {\ncontinue\n}\n}\n}\n";
        let expected = "~ {\n    temp i = 0\n    while true {\n        i = i + 1\n        if i > 10 {\n            break\n        }\n        if i mod 2 == 0 {\n            continue\n        }\n    }\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn block_parse_error_indented_first_line_keeps_indent() {
        // A knot-nested (indented) malformed block must stay verbatim
        // INCLUDING the first line's leading indentation — the span anchors
        // to the physical line start, not the `~` token offset.
        let input =
            "=== knot ===\n  ~ {\n      temp y = 1\n      if y > 0 // note\n      { y = 2 }\n  }\n";
        let out = fmt(input);
        assert!(
            out.contains("\n  ~ {\n"),
            "first line of the verbatim block must keep its leading indent, got:\n{out}"
        );
        assert_eq!(fmt(&out), out, "verbatim bail-out must stay idempotent");
    }

    fn tier1_brink_fixture(name: &str) -> String {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../tests/tier1-brink")
            .join(name)
            .join("story.ink");
        std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("failed to read fixture {}: {e}", path.display()))
    }

    #[test]
    fn tier1_brink_fixtures_already_formatted_round_trip_unchanged() {
        // These fixtures are already hand-written in the target style (4
        // spaces per block nesting level) — formatting them must be a no-op,
        // and re-formatting the result must be idempotent.
        for name in [
            "if-else-chain",
            "while-loop",
            "for-in-array",
            "break-continue",
            "stdlib-mutator-nested-lvalue",
            "nested-index-assignment",
        ] {
            let source = tier1_brink_fixture(name);
            let first = fmt(&source);
            assert_eq!(first, source, "fixture {name} should round-trip unchanged");
            let second = fmt(&first);
            assert_eq!(first, second, "fixture {name} should format idempotently");
        }
    }

    #[test]
    fn tier1_brink_fixtures_idempotent_from_deindented_input() {
        // Strip the fixtures' own indentation and confirm the formatter
        // still converges to a fixed point (and does so in one pass).
        for name in [
            "if-else-chain",
            "while-loop",
            "for-in-array",
            "break-continue",
        ] {
            let source = tier1_brink_fixture(name);
            let stripped: String = source
                .lines()
                .map(str::trim_start)
                .collect::<Vec<_>>()
                .join("\n");
            let stripped = format!("{stripped}\n");
            let first = fmt(&stripped);
            let second = fmt(&first);
            assert_eq!(
                first, second,
                "fixture {name} should converge to a fixed point"
            );
        }
    }

    // ── #603: parse errors inside `~ { … }` blocks bail to verbatim ─────
    // `render_logic_block` assumes a well-formed CST subtree; mid-edit or
    // otherwise malformed blocks must instead pass through byte-for-byte
    // (the pre-#602 `~ { … }` behavior) rather than being corrupted.

    #[test]
    fn block_parse_error_comment_before_brace_stays_verbatim() {
        // Repro (a): a trailing `//` comment between the `if` condition and
        // its opening `{` produces a parse error (the grammar treats the
        // real `{` on the next line as an unexpected token, wrapping it in
        // an `ERROR` node) — `header_expr_text` used to inline the comment
        // right before the ` {`, commenting the brace itself out. Verbatim
        // pass-through must leave the source untouched.
        let input = "~ {\nif x>0 // note\n{\nx = 1\n}\n}\n";
        assert_eq!(
            fmt(input),
            input,
            "malformed block must pass through verbatim"
        );
    }

    #[test]
    fn block_parse_error_multiline_call_stays_verbatim_and_idempotent() {
        // Repro (b): a multi-line call missing a comma is a parse error
        // (ERROR node wraps the unexpected token) — the old code injected
        // spurious blank lines and wasn't idempotent.
        let input = "~ {\nfoo(\n  1,\n  2\n\nbar()\n}\n";
        let first = fmt(input);
        assert_eq!(first, input, "malformed block must pass through verbatim");
        let second = fmt(&first);
        assert_eq!(first, second, "verbatim pass-through must be idempotent");
    }

    #[test]
    fn block_parse_error_lone_else_stays_verbatim() {
        // Repro (c): a lone `else` with no preceding `if {` on the same
        // construct is a parse error (ERROR node wraps the stray `else`
        // keyword) — the old code mangled it into a bare statement line
        // with mismatched braces.
        let input = "~ {\nif x {\nelse\n}\n}\n";
        assert_eq!(
            fmt(input),
            input,
            "malformed block must pass through verbatim"
        );
    }

    #[test]
    fn block_parse_error_missing_closing_brace_stays_verbatim() {
        // A missing expected token (here: the block's own closing `}`) is
        // recorded as a `ParseError` with a zero-length range at EOF but
        // does *not* insert an `ERROR` CST node — `subtree_has_parse_error`
        // must catch this via `Parse::errors()`, not by scanning for `ERROR`
        // nodes alone.
        let input = "~ {\ntemp x = 0\n";
        assert_eq!(
            fmt(input),
            input,
            "malformed block must pass through verbatim"
        );
    }

    #[test]
    fn block_well_formed_still_reindents_alongside_malformed_sibling() {
        // A parse error in one `~ { … }` block must not disable reindenting
        // for a well-formed block elsewhere in the same file.
        let input = "~ {\nif x {\nelse\n}\n}\nContent\n~ {\ntemp y   =   1\n}\n";
        let expected = "~ {\nif x {\nelse\n}\n}\nContent\n~ {\n    temp y = 1\n}\n";
        assert_eq!(fmt(input), expected);
    }

    #[test]
    fn tabs_indent_knot() {
        let input = "=== myknot ===\nContent\n";
        let result = fmt_tabs(input);
        assert_eq!(result, "=== myknot ===\n\tContent\n");
    }

    #[test]
    fn intercept_start_knot() {
        // Lines 74-103 of TheIntercept.ink — exercises knot body indentation,
        // gathers, choices at multiple depths, content in choice bodies, logic
        // lines, diverts, comments, and blank line handling.
        let input = "\
=== start === \n\
\n\
//  Intro\n\
\t- \tThey are keeping me waiting. \n\
\t\t*\tHut 14[]. The door was locked after I sat down. \n\
\t\tI don't even have a pen to do any work. There's a copy of the morning's intercept in my pocket, but staring at the jumbled letters will only drive me mad. \n\
\t\tI am not a machine, whatever they say about me.\n\
\n\
\t- (opts)\n\
\t\t{|I rattle my fingers on the field table.|}\n\
 \t\t* \t(think) [Think] \n\
 \t\t\tThey suspect me to be a traitor. They think I stole the component from the calculating machine. They will be searching my bunk and cases. \n\
\t\t\tWhen they don't find it, {plan:then} they'll come back and demand I talk. \n\
\t\t\t-> opts\n\
 \t\t*\t(plan) [Plan]\n\
 \t\t\t{not think:What I am is|I am} a problem\u{2014}solver. Good with figures, quick with crosswords, excellent at chess. \n\
 \t\t\tBut in this scenario \u{2014} in this trap \u{2014} what is the winning play?\n\
 \t\t\t* * \t(cooperate) [Co\u{2014}operate] \n\
\t \t\t\t\tI must co\u{2014}operate. My credibility is my main asset. To contradict myself, or another source, would be fatal. \n\
\t \t\t\t\tI must simply hope they do not ask the questions I do not want to answer.\n\
\t\t \t\t\t~ lower(forceful)\n\
\t \t\t* * \t[Dissemble] \n\
\t\t \t\t\tMisinformation, then. Just as the war in Europe is one of plans and interceptions, not planes and bombs. \n\
\t\t \t\t\tMy best hope is a story they prefer to the truth. \n\
\t\t \t\t\t~ raise(forceful)\n\
\t \t\t* * \t(delay) [Divert] \n\
\t\t \t\t\tAvoidance and delay. The military machine never fights on a single front. If I move slowly enough, things will resolve themselves some other way, my reputation intact.\n\
\t\t \t\t\t~ raise(evasive)\n\
\t\t*\t[Wait]\t\t\n\
\t- \t-> waited\n";

        // NOTE: The first gather `- They are keeping me waiting.` and its
        // following `* Hut 14[]` choice are siblings in the HIR (not parent-
        // child), so the choice is at knot-body depth (1) rather than inside
        // the gather body (depth 2). The `- (opts)` continuation gather
        // correctly indents its body content because the HIR models it as a
        // ChoiceSet continuation block.
        let i1 = "  ";
        let i2 = "    ";
        let i3 = "      ";
        let i4 = "        ";
        let expected = [
            "=== start ===",
            "",
            &format!("{i1}//  Intro"),
            &format!("{i1}- They are keeping me waiting."),
            &format!("{i1}* Hut 14[]. The door was locked after I sat down."),
            &format!("{i2}I don't even have a pen to do any work. There's a copy of the morning's intercept in my pocket, but staring at the jumbled letters will only drive me mad."),
            &format!("{i2}I am not a machine, whatever they say about me."),
            "",
            &format!("{i1}- (opts)"),
            &format!("{i2}{{|I rattle my fingers on the field table.|}}"),
            &format!("{i2}* (think) [Think]"),
            &format!("{i3}They suspect me to be a traitor. They think I stole the component from the calculating machine. They will be searching my bunk and cases."),
            &format!("{i3}When they don't find it, {{plan:then}} they'll come back and demand I talk."),
            &format!("{i3}-> opts"),
            &format!("{i2}* (plan) [Plan]"),
            &format!("{i3}{{not think:What I am is|I am}} a problem\u{2014}solver. Good with figures, quick with crosswords, excellent at chess."),
            &format!("{i3}But in this scenario \u{2014} in this trap \u{2014} what is the winning play?"),
            &format!("{i3}** (cooperate) [Co\u{2014}operate]"),
            &format!("{i4}I must co\u{2014}operate. My credibility is my main asset. To contradict myself, or another source, would be fatal."),
            &format!("{i4}I must simply hope they do not ask the questions I do not want to answer."),
            &format!("{i4}~ lower(forceful)"),
            &format!("{i3}** [Dissemble]"),
            &format!("{i4}Misinformation, then. Just as the war in Europe is one of plans and interceptions, not planes and bombs."),
            &format!("{i4}My best hope is a story they prefer to the truth."),
            &format!("{i4}~ raise(forceful)"),
            &format!("{i3}** (delay) [Divert]"),
            &format!("{i4}Avoidance and delay. The military machine never fights on a single front. If I move slowly enough, things will resolve themselves some other way, my reputation intact."),
            &format!("{i4}~ raise(evasive)"),
            &format!("{i2}* [Wait]"),
            &format!("{i1}- -> waited"),
            "",  // trailing newline
        ].join("\n");

        let result = fmt(input);
        assert_eq!(result, expected);
    }

    // ── TM-4b structs: block-style formatting (docs/typed-mode-spec.md §6) ──
    //
    // Single-line structs format to a single line with canonical spacing
    // (`field: type` with single space after colon). Multiline structs
    // format like blocks: proper field indentation + trailing comma on
    // each field. Both formats are idempotent.

    #[test]
    fn struct_decl_single_line_normalizes_spacing() {
        // Input with irregular spacing; should normalize to `field: type` with
        // single space after colon, no trailing comma on single-line form.
        let input = "STRUCT Point = #{x:float,y:  float}\n";
        let expected = "STRUCT Point = #{x: float, y: float}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "single-line struct should normalize spacing"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_formats_with_indentation_and_trailing_commas() {
        // Multiline struct should be formatted with field indentation and
        // trailing commas. Input indentation is normalized.
        let input = "STRUCT Point = #{\nx: float,\ny: float,\n}\n";
        let expected = "STRUCT Point = #{\n  x: float,\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "multiline struct should format with proper indentation and trailing commas"
        );
        assert_eq!(
            fmt(&once),
            once,
            "formatting twice must be a no-op (idempotent)"
        );
    }

    #[test]
    fn struct_decl_multiline_is_idempotent() {
        let input = "STRUCT Point = #{\n  x: float,\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, input,
            "properly formatted multiline struct should round-trip"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_with_complex_types() {
        // Fields with complex types (arrays, maps, nested generics) should
        // format correctly with proper type text reconstruction.
        let input = "STRUCT Data = #{\nvalues: Array<int>,\nmapping: Map<string, float>,\n}\n";
        let expected =
            "STRUCT Data = #{\n  values: Array<int>,\n  mapping: Map<string, float>,\n}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting is idempotent");
    }

    #[test]
    fn struct_decl_followed_by_ordinary_content_formats_normally() {
        // The `STRUCT_DECL` formats as a unit; everything after it still
        // goes through the ordinary formatter rules (blank line before knot
        // header, body content indented one level).
        let input = "STRUCT Point = #{\nx: float,\ny: float,\n}\n=== main ===\nHello.\n-> DONE\n";
        let expected =
            "STRUCT Point = #{\n  x: float,\n  y: float,\n}\n\n=== main ===\n  Hello.\n  -> DONE\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting is idempotent");
    }

    // `skip_struct_body_trivia` (brink-syntax parser/declaration.rs) bumps
    // `LINE_COMMENT`/`BLOCK_COMMENT` tokens as direct children of
    // `STRUCT_DECL`, not as children of any `STRUCT_FIELD_DECL`. The
    // multiline and single-line renderers both need to walk the node's own
    // children (like `render_stmt_block` does for logic blocks) rather than
    // iterating fields alone, or these comments are silently dropped.

    #[test]
    fn struct_decl_multiline_preserves_trailing_same_line_comment() {
        let input = "STRUCT Point = #{\n    x: float, // the x coord\n    y: float,\n}\n";
        let expected = "STRUCT Point = #{\n  x: float, // the x coord\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "a same-line trailing comment must stay attached to its field"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_preserves_leading_comment() {
        let input = "STRUCT Point = #{\n    // header comment\n    x: float,\n    y: float,\n}\n";
        let expected = "STRUCT Point = #{\n  // header comment\n  x: float,\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "a standalone comment before the first field must be preserved"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_preserves_interleaved_comment() {
        let input = "STRUCT Point = #{\n    x: float,\n    // between fields\n    y: float,\n}\n";
        let expected = "STRUCT Point = #{\n  x: float,\n  // between fields\n  y: float,\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "a standalone comment between fields must be preserved"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    // Drives `render_struct_decl` directly against the parsed `STRUCT_DECL`
    // node to exercise the renderer in isolation from line classification.
    // (The full `fmt()` pipeline reaches it too since
    // `comment_handled_by_construct` leaves in-brace comments to the
    // STRUCT_DECL arm — see
    // `struct_decl_single_line_with_inline_block_comment_is_normalized`.)
    fn render_struct_decl_only(source: &str) -> String {
        let parsed = brink_syntax::parse(source);
        let node = parsed
            .syntax()
            .children()
            .find(|c| c.kind() == SyntaxKind::STRUCT_DECL)
            .expect("source must parse to a single STRUCT_DECL");
        render::render_struct_decl(
            &node,
            "",
            &FormatConfig {
                indent: IndentStyle::Spaces(2),
            },
        )
    }

    #[test]
    fn struct_decl_single_line_preserves_leading_interleaved_and_trailing_comments() {
        let leading = render_struct_decl_only("STRUCT Point = #{/* lead */ x: float, y: float}\n");
        assert_eq!(leading, "STRUCT Point = #{/* lead */ x: float, y: float}\n");

        let interleaved =
            render_struct_decl_only("STRUCT Point = #{x: float, /* mid */ y: float}\n");
        assert_eq!(
            interleaved,
            "STRUCT Point = #{x: float /* mid */, y: float}\n"
        );

        let trailing =
            render_struct_decl_only("STRUCT Point = #{x: float, y: float /* trail */}\n");
        assert_eq!(
            trailing,
            "STRUCT Point = #{x: float, y: float /* trail */}\n"
        );
    }

    // ── Inline block comments must not pre-empt construct classification ──
    //
    // `mark_block_comments` used to mark ANY physical line containing a
    // `BLOCK_COMMENT` token anywhere in its subtree as a pure
    // `LineKind::BlockComment` line, which made `classify_node` skip the
    // line's real construct (`STRUCT_DECL`, `LOGIC_LINE`) entirely — the
    // line rendered verbatim instead of through the construct's own
    // comment-aware renderer. These tests pin the fixed behavior: a
    // single-line block comment nested inside a comment-aware construct is
    // that construct's business.

    #[test]
    fn struct_decl_single_line_with_inline_block_comment_is_normalized() {
        // Extra whitespace proves the line goes through
        // `format_struct_decl_single_line` rather than the verbatim
        // block-comment path (which would leave it untouched).
        let input = "STRUCT   Point =  #{x:   float, /* mid */ y: float}\n";
        let expected = "STRUCT Point = #{x: float /* mid */, y: float}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn struct_decl_multiline_with_block_comment_on_opening_line_keeps_field_indent() {
        // The comment on the `#{` line used to mark the whole first line as
        // a block-comment line, skipping the STRUCT_DECL arm — the fields
        // lost their indentation entirely.
        let input = "STRUCT Point = #{ /* c */\n    x: float,\n}\n";
        let expected = "STRUCT Point = #{\n  /* c */\n  x: float,\n}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn logic_line_with_trailing_block_comment_is_normalized() {
        let input = "~x = 5 /* foo */\n";
        let expected = "~ x = 5 /* foo */\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn logic_block_with_comment_on_opening_line_keeps_body_indent() {
        // Same failure mode as the struct case, for T1b `~ { … }` blocks:
        // the comment after `{` used to skip the LOGIC_LINE arm and the
        // block body lost its indentation.
        let input = "~ { /* c */\n    x = 5\n}\n";
        let expected = "~ {\n    /* c */\n    x = 5\n}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    // ── Comments outside the STMT_BLOCK on a `~ { … }` line ──
    //
    // A comment that is a direct child of the `LOGIC_LINE` itself — between
    // `~` and `{`, or trailing after the closing `}` — sits outside the
    // `STMT_BLOCK` the body is rebuilt from. It used to be silently dropped
    // (`render_logic_block` walked only the `STMT_BLOCK`). It is now emitted
    // on the header/closing line.

    #[test]
    fn logic_block_trailing_block_comment_after_close_is_preserved() {
        let input = "~ {\n    x = 5\n} /* c */\n";
        let expected = "~ {\n    x = 5\n} /* c */\n";
        let once = fmt(input);
        assert_eq!(once, expected, "trailing block comment must not be dropped");
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn logic_block_trailing_line_comment_after_close_is_preserved() {
        let input = "~ {\n    x = 5\n} // c\n";
        let expected = "~ {\n    x = 5\n} // c\n";
        let once = fmt(input);
        assert_eq!(once, expected, "trailing line comment must not be dropped");
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn logic_block_multiple_trailing_comments_after_close_are_preserved() {
        let input = "~ {\n    x = 5\n} /* a */ /* b */\n";
        let expected = "~ {\n    x = 5\n} /* a */ /* b */\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn logic_block_leading_comment_between_tilde_and_brace_is_preserved() {
        // The comment sits before the `{`; it used to mark the whole opening
        // line as a block-comment line, skipping classification and
        // de-indenting the body to column 0.
        let input = "~ /* c */ {\n    x = 5\n}\n";
        let expected = "~ /* c */ {\n    x = 5\n}\n";
        let once = fmt(input);
        assert_eq!(
            once, expected,
            "leading comment must be kept, body re-indented"
        );
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn logic_block_leading_and_trailing_comments_both_preserved() {
        let input = "~ /* lead */ {\n    x = 5\n} /* trail */\n";
        let expected = "~ /* lead */ {\n    x = 5\n} /* trail */\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn logic_block_single_line_with_trailing_comment_expands_and_keeps_comment() {
        // A single-line `~ { … }` block always expands to multi-line (like
        // the no-comment case `~ { x = 5 }` → `~ {\n    x = 5\n}\n`); the
        // trailing comment rides along on the closing line rather than
        // pinning the whole line to a verbatim single-line form.
        let input = "~ { x = 5 } /* c */\n";
        let expected = "~ {\n    x = 5\n} /* c */\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn logic_block_trailing_comment_preserved_inside_knot_with_indent() {
        // The block is nested one level inside a knot, so the trailing
        // comment must ride the closing `}`'s indented line.
        let input = "== k ==\n~ {\n    x = 5\n} /* c */\n-> DONE\n";
        let expected = "=== k ===\n  ~ {\n      x = 5\n  } /* c */\n  -> DONE\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    // ── T1e path-ref argument formatting (docs/t1e-spec.md §2, issue #850)
    //
    // A `ref lvalue-path` argument (`ref npc.hp`, `ref inventory[idx]`,
    // `ref party[leader].hp`) is ordinary expression syntax as far as the
    // CST is concerned — `REF_EXPR` wrapping a `PATH`/`INDEX_EXPR` chain —
    // so it already flows through the same token-joining machinery every
    // other `~ { … }` block statement does (`join_token_text`'s
    // whitespace-run-collapses-to-one-space rule). These lock that in as a
    // deliberate, tested contract rather than an untested coincidence: a
    // `ref`-marked path argument inside a multi-statement block reformats
    // exactly like any other call argument, canonical single-space spacing
    // throughout, one statement per line.

    #[test]
    fn block_ref_path_field_argument_normalizes_spacing() {
        let input = "~ {\ntemp x =   0\nheal(ref  npc.hp,   5)\n}\n";
        let expected = "~ {\n    temp x = 0\n    heal(ref npc.hp, 5)\n}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn block_ref_path_index_argument_normalizes_spacing() {
        let input = "~ {\nbump(ref  inventory[ idx ],   5)\n}\n";
        let expected = "~ {\n    bump(ref inventory[idx], 5)\n}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn block_ref_path_mixed_field_and_index_argument_normalizes_spacing() {
        // `ref party[leader].hp` — the t1e-spec §2 grammar example itself
        // (index segment followed by a field segment).
        let input = "~ {\nheal(ref  party[ leader ] . hp,   5)\n}\n";
        let expected = "~ {\n    heal(ref party[leader].hp, 5)\n}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    #[test]
    fn block_ref_path_through_fn_value_normalizes_spacing() {
        let input = "~ {\ntemp healer =   #fn( heal ,  ref  npc.hp )\n}\n";
        let expected = "~ {\n    temp healer = #fn( heal , ref npc.hp )\n}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }

    // Lines where the comment is NOT inside a comment-aware construct's
    // handled region keep the verbatim block-comment treatment — classifying
    // them would drop the comment (renderers only walk the construct node)
    // or disturb parser-fragmented lines.

    #[test]
    fn block_comment_lines_outside_comment_aware_constructs_stay_verbatim() {
        // Standalone banner (single- and multi-line).
        for src in [
            "/* banner */\nHello\n",
            "/* line one\n   line two */\nHello\n",
            // Leading comment before a construct on the same line: the
            // comment is a direct child of SOURCE_FILE, outside the
            // construct node — classification would drop it.
            "/* c */ ~ x = 5\n",
            "/* c */ STRUCT Point = #{x: float}\n",
            // Comment outside a struct's braces: the struct renderers only
            // walk the region between `{` and `}`.
            "STRUCT Point /* c */ = #{x: float}\n",
            // Multi-line comment inside a plain logic line: the Logic arm
            // only renders the first physical line.
            "~ x = 5 + /* multi\nline */ 6\n",
            // Content line split by the parser around the comment.
            "Hello /* hi */ world\n",
        ] {
            let once = fmt(src);
            assert_eq!(once, src, "must stay verbatim: {src:?}");
            assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
        }
    }

    #[test]
    fn struct_decl_empty_multiline_collapses_to_single_line() {
        // An empty struct body has no fields, so `is_multiline` (which
        // requires at least one field) is always false — an empty struct
        // written across multiple lines collapses to the canonical
        // single-line empty form. This is intentional: there is no field
        // content to justify a multiline layout.
        let input = "STRUCT Empty = #{\n}\n";
        let expected = "STRUCT Empty = #{}\n";
        let once = fmt(input);
        assert_eq!(once, expected);
        assert_eq!(fmt(&once), once, "formatting twice must be a no-op");
    }
}
