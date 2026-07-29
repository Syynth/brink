//! Grammar-coverage completeness gate (#1200, capstone of #1191's native
//! parser test-parity tracking issue).
//!
//! Every `SyntaxKind` the lexer/parser can produce must be exercised by at
//! least one test in the suite, or explicitly, individually exempted with a
//! reason — never with a blanket allow-list. This is a tripwire, not a
//! construct family: a new node/token landing without coverage must fail
//! CI, mechanically, instead of relying on a manual review step catching it
//! (the B0.6 review finding cited in #1191 is exactly the failure mode this
//! gate exists to catch).
//!
//! **Corpus**: this crate's `tests/corpus/` directory (shared with
//! `corpus_roundtrip.rs`) — every `.brink` fixture is parsed and the union
//! of every `SyntaxKind` appearing anywhere in the resulting CSTs (nodes
//! *and* tokens, via `descendants_with_tokens`) is the "exercised" set.
//! Reusing the round-trip corpus rather than inventing a second one keeps
//! one fixture set serving two gates — a new construct's fixture earns
//! both the round-trip check and this completeness check for free.
//!
//! **Classification**: [`classify`] is an *exhaustive* match over every
//! `SyntaxKind` variant (no wildcard arm) — the same idiom this crate's own
//! `SyntaxKind::is_token`/`is_node`/`is_keyword` already use one file up
//! (`src/syntax_kind.rs`). Adding a new variant to the enum is a compile
//! error here until it is explicitly classified, which is the whole point:
//! a hand-maintained allow-list can silently go stale, an exhaustive match
//! cannot.

use std::collections::BTreeSet;
use std::path::Path;

use brink_syntax_native::{SyntaxKind, parse};

/// What the gate expects of one `SyntaxKind`.
enum Coverage {
    /// Must appear somewhere in the parsed corpus's CST.
    Exercised,
    /// Documented as unreachable by construction — never produced by any
    /// input, so requiring corpus coverage for it would be requiring the
    /// impossible. The `&'static str` is the reason, checked against
    /// `SyntaxKind`'s own doc comments/source at review time, not merely
    /// asserted here.
    ExemptUnreachable(&'static str),
}

/// Classify every `SyntaxKind` variant. **No wildcard arm** — a new variant
/// added to the enum in `src/syntax_kind.rs` must get an arm here before
/// the crate compiles again (mirrors `SyntaxKind::is_token`'s own
/// no-wildcard match). `match_same_arms` is expected, not fixed by merging
/// every `Exercised` arm into one giant `|`-pattern: the section grouping
/// (with its own comments) mirrors `syntax_kind.rs`'s own enum layout
/// one-for-one, which is the point — a reviewer can diff the two side by
/// side to see at a glance which section a new variant landed in and
/// whether it got an arm here.
#[expect(
    clippy::too_many_lines,
    reason = "one arm per SyntaxKind variant, flat classification table"
)]
#[expect(
    clippy::match_same_arms,
    reason = "arms are grouped to mirror syntax_kind.rs's own section layout, not merged for brevity — see fn doc"
)]
fn classify(kind: SyntaxKind) -> Coverage {
    use Coverage::{ExemptUnreachable, Exercised};
    match kind {
        // ── Trivia tokens ─────────────────────────────────────────
        SyntaxKind::WHITESPACE
        | SyntaxKind::NEWLINE
        | SyntaxKind::LINE_COMMENT
        | SyntaxKind::BLOCK_COMMENT
        | SyntaxKind::DOC_COMMENT_OUTER
        | SyntaxKind::DOC_COMMENT_INNER => Exercised,

        // ── Keyword tokens ───────────────────────────────────────
        SyntaxKind::KW_FLOW
        | SyntaxKind::KW_FN
        | SyntaxKind::KW_VAR
        | SyntaxKind::KW_CONST
        | SyntaxKind::KW_LET
        | SyntaxKind::KW_FLAGS
        | SyntaxKind::KW_STRUCT
        | SyntaxKind::KW_EXTERN
        | SyntaxKind::KW_IMPORT
        | SyntaxKind::KW_USE
        | SyntaxKind::KW_MODULE
        | SyntaxKind::KW_RETURN
        | SyntaxKind::KW_REF
        | SyntaxKind::KW_IF
        | SyntaxKind::KW_MATCH
        | SyntaxKind::KW_ELSE
        | SyntaxKind::KW_WHILE
        | SyntaxKind::KW_FOR
        | SyntaxKind::KW_IN
        | SyntaxKind::KW_UNTIL
        | SyntaxKind::KW_BREAK
        | SyntaxKind::KW_CONTINUE
        | SyntaxKind::KW_AS
        | SyntaxKind::KW_OR
        | SyntaxKind::KW_TRUE
        | SyntaxKind::KW_FALSE
        | SyntaxKind::KW_END
        | SyntaxKind::KW_DONE => Exercised,

        // ── Punctuation / operator tokens ────────────────────────
        SyntaxKind::EQ
        | SyntaxKind::PLUS_EQ
        | SyntaxKind::MINUS_EQ
        | SyntaxKind::EQ_EQ
        | SyntaxKind::BANG_EQ
        | SyntaxKind::LT
        | SyntaxKind::GT
        | SyntaxKind::LT_EQ
        | SyntaxKind::GT_EQ
        | SyntaxKind::AMP
        | SyntaxKind::AMP_AMP
        | SyntaxKind::PLUS
        | SyntaxKind::MINUS
        | SyntaxKind::STAR
        | SyntaxKind::SLASH
        | SyntaxKind::PERCENT
        | SyntaxKind::BANG
        | SyntaxKind::QUESTION
        | SyntaxKind::L_PAREN
        | SyntaxKind::R_PAREN
        | SyntaxKind::L_BRACE
        | SyntaxKind::R_BRACE
        | SyntaxKind::L_BRACKET
        | SyntaxKind::R_BRACKET
        | SyntaxKind::PIPE
        | SyntaxKind::COMMA
        | SyntaxKind::DOT
        | SyntaxKind::COLON
        | SyntaxKind::SEMICOLON
        | SyntaxKind::COLON_COLON
        | SyntaxKind::HASH
        | SyntaxKind::TILDE
        | SyntaxKind::BACKSLASH
        | SyntaxKind::AT => Exercised,

        // `STAR_EQ` (`*=`) and `SLASH_EQ` (`/=`) ARE lexed
        // (`lexer/punctuation.rs`) but have no parser owner: `stmt.rs`'s
        // `at_assignment` lookahead only recognizes `=`/`+=`/`-=`, so
        // `x *= 1;`/`x /= 1;` fall out of `assign_stmt` and are recovered
        // as content-ground `TEXT` instead. That is a real, timely grammar
        // gap (compound multiply/divide-assign half-implemented — see the
        // #1200 PR body's gap list, filed as a follow-up), not a reason to
        // treat these as unreachable: they ARE genuinely produced by the
        // pipeline (`tests/corpus/14_grammar_gaps.brink` exercises exactly
        // this), so they are `Exercised`, not exempt.
        SyntaxKind::STAR_EQ | SyntaxKind::SLASH_EQ => Exercised,

        // `CARET` (`^`) is lexed but has no `INFIX_EXPR` precedence entry
        // (`expr.rs`'s `Prec` table only wires `+ - * / %`) — same shape of
        // gap as `STAR_EQ`/`SLASH_EQ` above, same fixture
        // (`14_grammar_gaps.brink`) exercises it via the same content-
        // fallback path. `Exercised`, not exempt, for the same reason.
        SyntaxKind::CARET => Exercised,

        // ── Compound tokens ──────────────────────────────────────
        SyntaxKind::AT_L_BRACKET
        | SyntaxKind::GLUE
        | SyntaxKind::DIVERT
        | SyntaxKind::THREAD
        | SyntaxKind::FAT_ARROW => Exercised,

        // ── Content tokens ───────────────────────────────────────
        SyntaxKind::INTEGER
        | SyntaxKind::FLOAT
        | SyntaxKind::QUOTE
        | SyntaxKind::STRING_TEXT
        | SyntaxKind::STRING_ESCAPE
        | SyntaxKind::IDENT
        | SyntaxKind::ERROR_TOKEN => Exercised,

        // `EOF` is a synthetic sentinel `Parser::current`/`Parser::nth`
        // return when lookahead runs past the end of the real token
        // stream (`parser/mod.rs`) — the lexer never emits it, and
        // `Parser::bump` only ever pushes `self.tokens[self.pos]` into the
        // builder, so it can never become a leaf in any CST. Unreachable
        // by construction, not a coverage gap.
        SyntaxKind::EOF => ExemptUnreachable(
            "synthetic lookahead sentinel; lexer never emits it, bump() never pushes it into the tree (parser/mod.rs)",
        ),

        // ── Node kinds — doc comments ─────────────────────────────
        SyntaxKind::DOC_COMMENT => Exercised,

        // ── Node kinds — top level & declarations ────────────────
        SyntaxKind::SOURCE_FILE
        | SyntaxKind::FLOW_DECL
        | SyntaxKind::FN_DECL
        | SyntaxKind::PARAM_LIST
        | SyntaxKind::PARAM
        | SyntaxKind::VAR_DECL
        | SyntaxKind::CONST_DECL
        | SyntaxKind::FLAGS_DECL
        | SyntaxKind::FLAGS_MEMBER_LIST
        | SyntaxKind::FLAGS_MEMBER
        | SyntaxKind::STRUCT_DECL
        | SyntaxKind::STRUCT_FIELD
        | SyntaxKind::EXTERN_DECL
        | SyntaxKind::USE_DECL
        | SyntaxKind::USE_TREE
        | SyntaxKind::USE_TREE_LIST
        | SyntaxKind::IMPORT_DECL
        | SyntaxKind::MODULE_DECL => Exercised,

        // ── Node kinds — bodies & content ─────────────────────────
        SyntaxKind::BLOCK
        | SyntaxKind::CONTENT_LINE
        | SyntaxKind::TEXT
        | SyntaxKind::INTERPOLATION
        | SyntaxKind::GLUE_NODE
        | SyntaxKind::TAG_LINE
        | SyntaxKind::TAG => Exercised,

        // ── Node kinds — prose block elements ─────────────────────
        SyntaxKind::SCENE_STITCH
        | SyntaxKind::SCENE_HEADING
        | SyntaxKind::SCENE_TITLE
        | SyntaxKind::SCENE_SLUG
        | SyntaxKind::SCENE_BODY
        | SyntaxKind::CUE
        | SyntaxKind::CUE_NAME
        | SyntaxKind::COMPACT_CUE
        | SyntaxKind::PARENTHETICAL => Exercised,

        // ── Node kinds — inline markup (§4, issue #1716) ───────────
        SyntaxKind::SPAN
        | SyntaxKind::SPAN_NAME
        | SyntaxKind::SPAN_ATTR
        | SyntaxKind::SPAN_ATTR_VALUE
        | SyntaxKind::ESCAPE => Exercised,

        // ── Node kinds — choice points ─────────────────────────────
        SyntaxKind::CHOICE_POINT
        | SyntaxKind::CHOICE
        | SyntaxKind::CHOICE_BULLET
        | SyntaxKind::LABEL
        | SyntaxKind::CHOICE_GUARD
        | SyntaxKind::CHOICE_START_CONTENT
        | SyntaxKind::CHOICE_BRACKET_CONTENT
        | SyntaxKind::CHOICE_INNER_CONTENT
        | SyntaxKind::CHOICE_BODY
        | SyntaxKind::ELSE_BRANCH
        | SyntaxKind::SPLICE => Exercised,

        // ── Node kinds — the annotated-brace family ───────────────
        SyntaxKind::CONDITIONAL_BLOCK
        | SyntaxKind::IF_ARM
        | SyntaxKind::MATCH_ARM
        | SyntaxKind::MATCH_PATTERN
        | SyntaxKind::ALTERNATION_BLOCK
        | SyntaxKind::ALTERNATION_MARKER
        | SyntaxKind::ENTRY => Exercised,

        // ── Node kinds — annotations ───────────────────────────────
        SyntaxKind::ANNOTATION_LINE | SyntaxKind::ANNOTATION_ARGS | SyntaxKind::ANNOTATION_ARG => {
            Exercised
        }

        // ── Node kinds — diverts, tunnels, return ─────────────────
        SyntaxKind::DIVERT_STMT
        | SyntaxKind::TUNNEL_CALL
        | SyntaxKind::DIVERT_TARGET
        | SyntaxKind::RETURN_STMT
        | SyntaxKind::RETURN_REDIRECT => Exercised,

        // ── Node kinds — paths ─────────────────────────────────────
        SyntaxKind::PATH | SyntaxKind::PATH_SEGMENT => Exercised,

        // ── Node kinds — the minimal expression grammar ───────────
        SyntaxKind::INTEGER_LIT
        | SyntaxKind::FLOAT_LIT
        | SyntaxKind::STRING_LIT
        | SyntaxKind::BOOLEAN_LIT
        | SyntaxKind::PATH_EXPR
        | SyntaxKind::PAREN_EXPR
        | SyntaxKind::PREFIX_EXPR
        | SyntaxKind::INFIX_EXPR
        | SyntaxKind::CALL_EXPR
        | SyntaxKind::ARG_LIST
        | SyntaxKind::LAMBDA_EXPR
        | SyntaxKind::LAMBDA_PARAMS => Exercised,

        // ── Node kinds — the array/sequence literal (NG-D, #1490) ──
        SyntaxKind::ARRAY_LITERAL => Exercised,

        // ── Node kinds — the construction initializer ─────────────
        SyntaxKind::CONSTRUCT_LITERAL | SyntaxKind::CONSTRUCT_ENTRY => Exercised,

        // ── Node kinds — the code-ground statement layer ──────────
        SyntaxKind::STMT_BLOCK
        | SyntaxKind::LET_STMT
        | SyntaxKind::ASSIGN_STMT
        | SyntaxKind::EXPR_STMT => Exercised,

        // ── Node kinds — the code-ground statement tail ───────────
        SyntaxKind::BREAK_STMT | SyntaxKind::CONTINUE_STMT => Exercised,

        // ── Node kinds — the code-ground control-flow layer ───────
        SyntaxKind::IF_STMT
        | SyntaxKind::ELSE_CLAUSE
        | SyntaxKind::WHILE_STMT
        | SyntaxKind::FOR_STMT
        | SyntaxKind::UNTIL_STMT => Exercised,

        // ── Node kinds — the type-annotation grammar ───────────────
        SyntaxKind::TYPE_ANNOTATION
        | SyntaxKind::TYPE_EXPR
        | SyntaxKind::TYPE_NAME
        | SyntaxKind::TYPE_GENERIC
        | SyntaxKind::TYPE_FN => Exercised,

        // ── Node kind — the `as` binding ────────────────────────────
        SyntaxKind::AS_BINDING => Exercised,

        // ── Error recovery ──────────────────────────────────────────
        SyntaxKind::ERROR => Exercised,

        // Not a real kind — `rowan::Language::kind_to_raw` bounds sentinel
        // only. Never produced, never iterated over by this test (the loop
        // below stops at `__LAST`); this arm exists solely so the match
        // stays exhaustive without a wildcard.
        SyntaxKind::__LAST => {
            ExemptUnreachable("rowan discriminant-space sentinel, not a real kind")
        }
    }
}

fn collect_brink_files(dir: &Path, files: &mut Vec<std::path::PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries {
        let Ok(entry) = entry else { continue };
        let path = entry.path();
        if path.is_dir() {
            collect_brink_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "brink") {
            files.push(path);
        }
    }
}

/// Every `SyntaxKind` produced anywhere in the parsed corpus, tokens and
/// nodes alike (`descendants_with_tokens`, not `descendants` — a
/// `descendants`-only walk would silently never satisfy a token-kind
/// requirement, since tokens are leaves, not nodes).
///
/// Returns `Result` rather than unwrapping/panicking directly (workspace
/// lint: `unwrap_used`/`expect_used`/`panic` are denied; `clippy.toml`'s
/// `allow-unwrap-in-tests`/`allow-expect-in-tests` only exempt code inside
/// `#[test]`-attributed fn bodies, not plain helpers a test happens to
/// call — same rationale `respell_fixtures.rs` documents for its own
/// helpers).
fn exercised_kinds() -> Result<BTreeSet<SyntaxKind>, String> {
    let manifest_dir = Path::new(env!("CARGO_MANIFEST_DIR"));
    let corpus_dir = manifest_dir.join("tests").join("corpus");
    let corpus_dir = corpus_dir
        .canonicalize()
        .map_err(|e| format!("corpus directory not found: {e}"))?;

    let mut files = Vec::new();
    collect_brink_files(&corpus_dir, &mut files);
    if files.is_empty() {
        return Err(format!("no .brink files found in {}", corpus_dir.display()));
    }

    let mut seen = BTreeSet::new();
    for path in &files {
        let source =
            std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
        let parsed = parse(&source);
        for element in parsed.syntax().descendants_with_tokens() {
            seen.insert(element.kind());
        }
    }
    Ok(seen)
}

#[test]
fn every_syntax_kind_is_exercised_or_explicitly_exempt() {
    let seen = exercised_kinds().unwrap();

    let mut missing = Vec::new();
    let mut stale_exemptions = Vec::new();

    let mut raw = 0u16;
    loop {
        if raw == SyntaxKind::__LAST as u16 {
            break;
        }
        // SAFETY-free: reuse the crate's own bounds-checked conversion via
        // rowan, exactly like `syntax_kind.rs`'s own tests do, rather than
        // reaching for `transmute` a second time in this crate.
        let kind = <brink_syntax_native::NativeLanguage as rowan::Language>::kind_from_raw(
            rowan::SyntaxKind(raw),
        );
        raw += 1;

        match classify(kind) {
            Coverage::Exercised => {
                if !seen.contains(&kind) {
                    missing.push(kind);
                }
            }
            Coverage::ExemptUnreachable(_reason) => {
                if seen.contains(&kind) {
                    stale_exemptions.push(kind);
                }
            }
        }
    }

    assert!(
        stale_exemptions.is_empty(),
        "these SyntaxKinds are classified ExemptUnreachable in \
         tests/coverage.rs::classify but a corpus fixture now produces \
         them — the exemption is stale; reclassify as Exercised (the \
         corpus already proves it, no new fixture needed):\n{stale_exemptions:#?}"
    );

    assert!(
        missing.is_empty(),
        "the grammar-coverage completeness gate (#1200) found {} \
         SyntaxKind variant(s) with no test coverage anywhere in \
         tests/corpus/. Each one is either a genuine missing test (add a \
         `.brink` fixture, or extend an existing one, that reaches it) or \
         a SyntaxKind that is unreachable by construction (classify it \
         `ExemptUnreachable` in this file's `classify` function, with a \
         comment naming the exact reason — never a blanket allow):\n{missing:#?}",
        missing.len()
    );
}
