//! Semantic token classification — core moved from `brink-ide` (#3064
//! B4) so `brink-db`'s per-segment queries can call it; `brink-ide`
//! keeps the `AnalysisResult`-taking wrappers and re-exports this
//! module's types from its old paths.

use std::collections::BTreeMap;

use crate::SymbolKind;
use brink_syntax::{SyntaxKind, SyntaxNode};
use rowan::TextRange;

use crate::hir::projection::range_key;
use crate::line_index::LineIndex;

// ── Token type names (indices into this array) ──────────────────────

/// Token type names for the semantic token legend.
pub fn token_type_names() -> &'static [&'static str] {
    &[
        "namespace",  // 0  knots
        "function",   // 1  stitches, externals
        "variable",   // 2  variables
        "string",     // 3  string content
        "number",     // 4  numeric literals
        "keyword",    // 5  VAR, CONST, LIST, INCLUDE, etc.
        "operator",   // 6  ->, <-, ~, etc.
        "comment",    // 7  // and /* */
        "enum",       // 8  list names
        "enumMember", // 9  list items
        "parameter",  // 10 function/knot params
        "decorator",  // 11 tags (#)
        "label",      // 12 labels, gather names
        "struct",     // 13 STRUCT declarations (TM-4b)
        "property",   // 14 struct-field segments of a dotted field access
        "marker", // 15 narrative structure sigils: choice bullets, gather dashes, weave brackets
        "divert", // 16 flow movement: ->, ->->, <-, glue
        "halt",   // 17 END / DONE
        "escape", // 18 the `\` of an escape — an authoring mark, not the text it protects
    ]
}

/// Token modifier names for the semantic token legend.
pub fn token_modifier_names() -> &'static [&'static str] {
    &[
        "declaration", // 1 << 0
        "definition",  // 1 << 1
        "readonly",    // 1 << 2
        "deprecated",  // 1 << 3 (future use)
    ]
}

// ── Token type indices ─────────────────────────────────────────────

pub const TT_NAMESPACE: u32 = 0;
pub const TT_FUNCTION: u32 = 1;
pub const TT_VARIABLE: u32 = 2;
pub const TT_STRING: u32 = 3;
pub const TT_NUMBER: u32 = 4;
pub const TT_KEYWORD: u32 = 5;
pub const TT_OPERATOR: u32 = 6;
pub const TT_COMMENT: u32 = 7;
pub const TT_ENUM: u32 = 8;
pub const TT_ENUM_MEMBER: u32 = 9;
pub const TT_PARAMETER: u32 = 10;
pub const TT_DECORATOR: u32 = 11;
pub const TT_LABEL: u32 = 12;
pub const TT_STRUCT: u32 = 13;
pub const TT_PROPERTY: u32 = 14;
pub const TT_MARKER: u32 = 15;
pub const TT_DIVERT: u32 = 16;
pub const TT_HALT: u32 = 17;
pub const TT_ESCAPE: u32 = 18;

// ── Modifier bitmasks ──────────────────────────────────────────────

pub const MOD_DECLARATION: u32 = 1 << 0;
pub const MOD_READONLY: u32 = 1 << 2;

// ── Raw token (absolute position) ──────────────────────────────────

/// A semantic token with absolute line/column position.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RawToken {
    pub line: u32,
    pub start_char: u32,
    pub length: u32,
    pub token_type: u32,
    pub modifiers: u32,
}

/// A delta-encoded semantic token.
#[derive(Debug, Clone)]
pub struct DeltaToken {
    pub delta_line: u32,
    pub delta_start: u32,
    pub length: u32,
    pub token_type: u32,
    pub token_modifiers: u32,
}

// ── Classification result ──────────────────────────────────────────

pub struct Classification {
    pub token_type: u32,
    pub modifiers: u32,
}

// ── Token classification ───────────────────────────────────────────

#[expect(
    clippy::too_many_lines,
    reason = "flat classifier dispatch — one arm per token family"
)]
pub fn classify_token(
    token: &brink_syntax::SyntaxToken,
    resolution_index: &BTreeMap<(u32, u32), SymbolKind>,
) -> Option<Classification> {
    let kind = token.kind();

    // The escape's backslash is the ONE thing inside an `ESCAPE` that is
    // not prose (#3142): it is an authoring mark, so it gets its own token
    // type and the editor dims it, while the character it protects carries
    // no token at all and inherits the surrounding narrative colour. The
    // arm sits ABOVE the prose carve-out below, which would otherwise
    // swallow it along with everything else inside the escape.
    //
    // A `\` that is NOT part of an escape stays unhighlighted, as it was
    // when this kind lived in the skip list below.
    if kind == SyntaxKind::BACKSLASH {
        return token
            .parent()
            .is_some_and(|p| p.kind() == SyntaxKind::ESCAPE)
            .then_some(Classification {
                token_type: TT_ESCAPE,
                modifiers: 0,
            });
    }

    // Skip tokens we never highlight
    if matches!(
        kind,
        SyntaxKind::WHITESPACE
            | SyntaxKind::NEWLINE
            | SyntaxKind::EOF
            | SyntaxKind::ERROR_TOKEN
            | SyntaxKind::L_PAREN
            | SyntaxKind::R_PAREN
            | SyntaxKind::COMMA
            | SyntaxKind::DOT
            | SyntaxKind::COLON
            | SyntaxKind::DOLLAR
    ) {
        return None;
    }

    // A leaf whose direct parent is `TEXT` is literal prose, whatever kind
    // the shared lexer tagged it as. `text_content` (`parser/content.rs`)
    // bumps every non-structural token straight into a flat `TEXT` node —
    // no wrapping `IDENTIFIER`/`PATH`/etc — and `{expr}` interpolation
    // always breaks the run and lowers as a *sibling* node before
    // `text_content` starts (`mixed_content`'s `L_BRACE` dispatch), so a
    // `TEXT` child is never anything else. #275 first carved this out for
    // keyword lexemes alone (the word "and" in "cats and dogs"); #2293
    // generalizes it to every kind — a hyphen in "well-known", a stray
    // quote mark in dialogue, a plain word — none of it is code, so none
    // of it gets a token. Mirrors the native classifier's
    // `is_prose_run_container` carve-out below, which the same review
    // thread (#2280/#2286) already established for `.brink` files.
    // ESCAPE joins TEXT for the same reason, and it is not covered by it:
    // the escaped character's direct parent is the `ESCAPE` node, not `TEXT`,
    // so `\*` slipped past this carve-out and the `*` was classified below as
    // an expression-position operator — painting a literal asterisk in the
    // machinery colour inside a run of prose (#3142). An escape exists
    // precisely to say "this character is text", so nothing inside one is
    // ever code.
    if token
        .parent()
        .is_some_and(|p| matches!(p.kind(), SyntaxKind::TEXT | SyntaxKind::ESCAPE))
    {
        return None;
    }

    // Content-logic delimiters read as CODE, not prose (author feedback,
    // 2026-08-25): the `{`/`}` around alternatives, conditionals, and
    // interpolations — and the `|` separating alternative branches — used
    // to carry no token at all, so they rendered in the dialogue/action
    // text color and visually merged into the prose around them. Classify
    // them as operators, the same family the delimited code already uses.
    // Placed AFTER the `TEXT`-parent carve-out above, so an escaped or
    // prose-absorbed brace/pipe stays uncolored prose.
    if matches!(
        kind,
        SyntaxKind::L_BRACE | SyntaxKind::R_BRACE | SyntaxKind::PIPE
    ) {
        return Some(Classification {
            token_type: TT_OPERATOR,
            modifiers: 0,
        });
    }

    // Narrative structure markers (theme ruling 2026-08-25): the choice
    // bullets, gather dashes, and weave brackets get their own token type —
    // they are wayfinding sigils, not operators, and the Manuscript/Inky
    // themes color them apart from logic. Position decides: the SAME
    // lexemes in expression position stay operators below.
    let parent_kind = token.parent().map(|p| p.kind());
    if matches!(kind, SyntaxKind::STAR | SyntaxKind::PLUS)
        && parent_kind == Some(SyntaxKind::CHOICE_BULLETS)
    {
        return Some(Classification {
            token_type: TT_MARKER,
            modifiers: 0,
        });
    }
    if kind == SyntaxKind::MINUS && parent_kind == Some(SyntaxKind::GATHER_DASHES) {
        return Some(Classification {
            token_type: TT_MARKER,
            modifiers: 0,
        });
    }
    if matches!(kind, SyntaxKind::L_BRACKET | SyntaxKind::R_BRACKET) {
        // Weave brackets in a choice line are markers; any other bracket
        // keeps the old skipped-entirely behavior.
        return if matches!(
            parent_kind,
            Some(
                SyntaxKind::CHOICE
                    | SyntaxKind::CHOICE_START_CONTENT
                    | SyntaxKind::CHOICE_BRACKET_CONTENT
                    | SyntaxKind::CHOICE_INNER_CONTENT
            )
        ) {
            Some(Classification {
                token_type: TT_MARKER,
                modifiers: 0,
            })
        } else {
            None
        };
    }

    // Header equals-runs take the definition's own color (they read as
    // one mark with the name — previously they fell to the operator arm
    // and split the header into two colors).
    if matches!(kind, SyntaxKind::EQ | SyntaxKind::EQ_EQ) {
        if parent_kind == Some(SyntaxKind::KNOT_HEADER) {
            return Some(Classification {
                token_type: TT_NAMESPACE,
                modifiers: 0,
            });
        }
        if parent_kind == Some(SyntaxKind::STITCH_HEADER) {
            return Some(Classification {
                token_type: TT_FUNCTION,
                modifiers: 0,
            });
        }
    }

    // Flow movement gets its own type (vs. general operators): diverts,
    // tunnels, threads, and glue are the "keeps going" marks.
    if matches!(
        kind,
        SyntaxKind::DIVERT | SyntaxKind::THREAD | SyntaxKind::TUNNEL_ONWARDS | SyntaxKind::GLUE
    ) {
        return Some(Classification {
            token_type: TT_DIVERT,
            modifiers: 0,
        });
    }

    // The halt words: -> END / -> DONE stop output where every other
    // divert continues it. Checked before the generic keyword arm.
    if matches!(kind, SyntaxKind::KW_END | SyntaxKind::KW_DONE) {
        return Some(Classification {
            token_type: TT_HALT,
            modifiers: 0,
        });
    }

    // Direct mappings by SyntaxKind
    if kind == SyntaxKind::LINE_COMMENT || kind == SyntaxKind::BLOCK_COMMENT {
        return Some(Classification {
            token_type: TT_COMMENT,
            modifiers: 0,
        });
    }

    if kind.is_keyword() {
        return Some(Classification {
            token_type: TT_KEYWORD,
            modifiers: 0,
        });
    }

    if kind == SyntaxKind::INTEGER || kind == SyntaxKind::FLOAT {
        return Some(Classification {
            token_type: TT_NUMBER,
            modifiers: 0,
        });
    }

    if matches!(
        kind,
        SyntaxKind::STRING_TEXT | SyntaxKind::STRING_ESCAPE | SyntaxKind::QUOTE
    ) {
        return Some(Classification {
            token_type: TT_STRING,
            modifiers: 0,
        });
    }

    if matches!(
        kind,
        SyntaxKind::TILDE
            | SyntaxKind::EQ
            | SyntaxKind::EQ_EQ
            | SyntaxKind::BANG_EQ
            | SyntaxKind::LT
            | SyntaxKind::GT
            | SyntaxKind::LT_EQ
            | SyntaxKind::GT_EQ
            | SyntaxKind::PLUS
            | SyntaxKind::MINUS
            | SyntaxKind::STAR
            | SyntaxKind::SLASH
            | SyntaxKind::PERCENT
            | SyntaxKind::CARET
            | SyntaxKind::BANG
            | SyntaxKind::QUESTION
            | SyntaxKind::BANG_QUESTION
            | SyntaxKind::AMP
            | SyntaxKind::AMP_AMP
            | SyntaxKind::PLUS_EQ
            | SyntaxKind::MINUS_EQ
    ) {
        return Some(Classification {
            token_type: TT_OPERATOR,
            modifiers: 0,
        });
    }

    if kind == SyntaxKind::HASH {
        return Some(Classification {
            token_type: TT_DECORATOR,
            modifiers: 0,
        });
    }

    // IDENT classification — context-dependent
    if kind == SyntaxKind::IDENT {
        return classify_ident(token, resolution_index);
    }

    None
}

fn classify_ident(
    token: &brink_syntax::SyntaxToken,
    resolution_index: &BTreeMap<(u32, u32), SymbolKind>,
) -> Option<Classification> {
    let parent = token.parent()?;
    let parent_kind = parent.kind();

    // IDENT directly in LIST_MEMBER_ON / LIST_MEMBER_OFF (no intermediate IDENTIFIER)
    if parent_kind == SyntaxKind::LIST_MEMBER_ON || parent_kind == SyntaxKind::LIST_MEMBER_OFF {
        return Some(Classification {
            token_type: TT_ENUM_MEMBER,
            modifiers: MOD_DECLARATION,
        });
    }

    // IDENT inside IDENTIFIER node — check grandparent
    if parent_kind == SyntaxKind::IDENTIFIER
        && let Some(grandparent) = parent.parent()
    {
        let gp_kind = grandparent.kind();
        return match gp_kind {
            SyntaxKind::KNOT_HEADER => Some(Classification {
                token_type: TT_NAMESPACE,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::STITCH_HEADER | SyntaxKind::EXTERNAL_DECL => Some(Classification {
                token_type: TT_FUNCTION,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::KNOT_PARAM_DECL => Some(Classification {
                token_type: TT_PARAMETER,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::LABEL => Some(Classification {
                token_type: TT_LABEL,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::VAR_DECL | SyntaxKind::TEMP_DECL => Some(Classification {
                token_type: TT_VARIABLE,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::CONST_DECL => Some(Classification {
                token_type: TT_VARIABLE,
                modifiers: MOD_DECLARATION | MOD_READONLY,
            }),
            SyntaxKind::LIST_DECL => Some(Classification {
                token_type: TT_ENUM,
                modifiers: MOD_DECLARATION,
            }),
            SyntaxKind::FUNCTION_CALL => Some(Classification {
                token_type: TT_FUNCTION,
                modifiers: 0,
            }),
            // PATH or other contexts — try resolution index
            _ => Some(classify_ident_by_resolution(token, resolution_index)),
        };
    }

    // Fallback: try resolution index
    Some(classify_ident_by_resolution(token, resolution_index))
}

fn classify_ident_by_resolution(
    token: &brink_syntax::SyntaxToken,
    resolution_index: &BTreeMap<(u32, u32), SymbolKind>,
) -> Classification {
    // Try the token's own range first
    if let Some(&sym_kind) = resolution_index.get(&range_key(token.text_range())) {
        return symbol_kind_to_classification(sym_kind);
    }

    // Try the parent IDENTIFIER node range (resolutions may use the node range)
    let Some(parent) = token.parent() else {
        return GENERIC_VARIABLE;
    };
    if parent.kind() == SyntaxKind::IDENTIFIER
        && let Some(&sym_kind) = resolution_index.get(&range_key(parent.text_range()))
    {
        return symbol_kind_to_classification(sym_kind);
    }

    // Try the enclosing PATH node's range. A dotted reference's resolution is
    // recorded against the *whole* path (`brink_ir::ResolvedRef::range`'s
    // load-bearing whole-path contract, pinned by #1561), so this is the only
    // range a segment of `p.x.y` / `Colors.Red` can be looked up by. The
    // segment tokens sit directly under `PATH` in some shapes and inside an
    // `IDENTIFIER` wrapper in others, so both are accepted.
    let path = if parent.kind() == SyntaxKind::PATH {
        Some(parent.clone())
    } else if parent.kind() == SyntaxKind::IDENTIFIER {
        parent.parent().filter(|gp| gp.kind() == SyntaxKind::PATH)
    } else {
        None
    };
    if let Some(path) = path
        && let Some(&sym_kind) = resolution_index.get(&range_key(path.text_range()))
    {
        return classify_path_segment(&path, token, sym_kind);
    }

    GENERIC_VARIABLE
}

/// The classification for an identifier no resolution accounts for.
const GENERIC_VARIABLE: Classification = Classification {
    token_type: TT_VARIABLE,
    modifiers: 0,
};

/// Classify one segment of a dotted `PATH` whose *whole* range carries the
/// resolution `sym_kind` (issue #1571).
///
/// Only ONE segment of such a path actually names the resolved symbol —
/// handing `sym_kind`'s colour to all of them rendered `p.x.y` as a single
/// flat `variable` run. Which segment it is depends on the kind, exactly as
/// it does for `rename`'s range narrowing (`ufcs_hover`'s
/// `field_access_head_range_at_path` vs `qualified_tail_range_at_path`):
///
/// - a **value** (`Variable`/`Constant`/`Param`/`Temp`) is named by the
///   *head* — `p` in `p.x.y` (`resolve::lookup_variable` step 11) — and the
///   trailing segments are struct field names, so they get `property`;
/// - a **stitch**, **list item** or **label** is named by the *tail* —
///   `market` in `hub.market`, `Red` in `Colors.Red` — and the leading
///   segments are qualifiers this pass cannot resolve on their own, so they
///   keep the unresolved-identifier fallback;
/// - every other kind resolves from a single-segment path, where head and
///   tail are the same token.
fn classify_path_segment(
    path: &SyntaxNode,
    token: &brink_syntax::SyntaxToken,
    sym_kind: SymbolKind,
) -> Classification {
    let named_by_tail = matches!(
        sym_kind,
        SymbolKind::Stitch | SymbolKind::ListItem | SymbolKind::Label
    );
    let segments: Vec<TextRange> = path
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|t| t.kind() == SyntaxKind::IDENT)
        .map(|t| t.text_range())
        .collect();
    let naming = if named_by_tail {
        segments.last()
    } else {
        segments.first()
    };
    if naming == Some(&token.text_range()) {
        return symbol_kind_to_classification(sym_kind);
    }
    if named_by_tail {
        return GENERIC_VARIABLE;
    }
    Classification {
        token_type: TT_PROPERTY,
        modifiers: 0,
    }
}

fn symbol_kind_to_classification(kind: SymbolKind) -> Classification {
    match kind {
        SymbolKind::Knot => Classification {
            token_type: TT_NAMESPACE,
            modifiers: 0,
        },
        SymbolKind::Stitch | SymbolKind::External => Classification {
            token_type: TT_FUNCTION,
            modifiers: 0,
        },
        SymbolKind::Variable | SymbolKind::Temp => Classification {
            token_type: TT_VARIABLE,
            modifiers: 0,
        },
        SymbolKind::Constant => Classification {
            token_type: TT_VARIABLE,
            modifiers: MOD_READONLY,
        },
        SymbolKind::List => Classification {
            token_type: TT_ENUM,
            modifiers: 0,
        },
        SymbolKind::ListItem => Classification {
            token_type: TT_ENUM_MEMBER,
            modifiers: 0,
        },
        SymbolKind::Label => Classification {
            token_type: TT_LABEL,
            modifiers: 0,
        },
        SymbolKind::Param => Classification {
            token_type: TT_PARAMETER,
            modifiers: 0,
        },
        SymbolKind::Struct => Classification {
            token_type: TT_STRUCT,
            modifiers: 0,
        },
    }
}

// ── Native (`.brink`) token classification (issue #2280) ───────────
//
// `brink_syntax_native::SyntaxKind` is a peer enum with its own discriminant
// space (NF-1 ruling) — a native file's tokens are never `brink_syntax`
// values, so they need their own classifier rather than an extra arm on
// [`classify_token`]. `symbol_kind_to_classification`/`GENERIC_VARIABLE`
// above are frontend-agnostic (they dispatch on `brink_ir::SymbolKind`, not
// on either `SyntaxKind`), so both classifiers share them.

/// Node kinds whose parser raw-bumps arbitrary source text into one CST
/// node, regardless of what the shared lexer tagged each byte as: `TEXT`
/// (`content::text_run_until`), `CUE_NAME` (`element::cue_name`), `TAG`
/// (`content::tag`), and `SCENE_TITLE` (`element::scene_title`). A leaf
/// whose direct parent is one of these is literal prose — a keyword
/// lexeme, a `-`, or a digit run inside one of them is text, never a
/// keyword/operator/number (review finding on #2280/#2286: the original
/// "absorbed into prose" carve-out only checked `TEXT`, missing the three
/// other raw-bump runs the native grammar introduced).
fn is_prose_run_container(kind: brink_syntax_native::SyntaxKind) -> bool {
    use brink_syntax_native::SyntaxKind as NK;
    // `ESCAPE` is here for the same reason and was missing for the same
    // reason the other three were (#3142): an escaped character's direct
    // parent is the `ESCAPE` node, so `\{` in a prose line fell straight
    // through to the operator arm and painted a literal brace as
    // interpolation syntax. An escape exists precisely to say "this
    // character is text", which is what this predicate means.
    matches!(
        kind,
        NK::TEXT | NK::CUE_NAME | NK::TAG | NK::SCENE_TITLE | NK::ESCAPE
    )
}

#[expect(
    clippy::too_many_lines,
    reason = "flat classifier dispatch — one arm per token family"
)]
pub fn classify_native_token(
    token: &brink_syntax_native::SyntaxToken,
    resolution_index: &BTreeMap<(u32, u32), SymbolKind>,
) -> Option<Classification> {
    use brink_syntax_native::SyntaxKind as NK;
    let kind = token.kind();

    // A leaf directly under a string-shaped literal — colour the whole
    // literal uniformly, however the shared lexer tagged this particular
    // byte. `lexer::lex_string_token` always emits `[`/`]`/`<>` as their own
    // punctuation kind even in string mode ("so the parser can find
    // choice-bracket boundaries... regardless of context, mirroring ink"),
    // so a plain string/attribute value with no such structural meaning
    // still gets these as literal children — e.g. a regex character class
    // `[A-Z]` inside a quoted string. Checked before anything else so it
    // wins over the "tokens we never highlight" skip list below (`[`/`]`
    // would otherwise vanish instead of reading as string content).
    // `SPAN_ATTR_VALUE` shares the same STRING_TEXT/STRING_ESCAPE/QUOTE
    // token shape (deliberately with no INTERPOLATION arm, so this is safe
    // unconditionally). `STRING_LIT`'s own INTERPOLATION child (`{expr}`)
    // is a distinct node one level down — its own descendants have a
    // different immediate parent, so they fall through and classify
    // normally as an expression, not as string text.
    if token
        .parent()
        .is_some_and(|p| matches!(p.kind(), NK::STRING_LIT | NK::SPAN_ATTR_VALUE))
    {
        return Some(Classification {
            token_type: TT_STRING,
            modifiers: 0,
        });
    }

    // The escape's backslash — see `classify_token`'s arm of the same
    // shape (#3142). Above the prose carve-out for the same reason.
    if kind == NK::BACKSLASH {
        return token
            .parent()
            .is_some_and(|p| p.kind() == NK::ESCAPE)
            .then_some(Classification {
                token_type: TT_ESCAPE,
                modifiers: 0,
            });
    }

    // Skip tokens we never highlight (mirrors `classify_token`'s ink list).
    if matches!(
        kind,
        NK::WHITESPACE
            | NK::NEWLINE
            | NK::EOF
            | NK::ERROR_TOKEN
            | NK::L_PAREN
            | NK::R_PAREN
            | NK::L_BRACKET
            | NK::R_BRACKET
            | NK::COMMA
            | NK::DOT
            | NK::COLON
            | NK::COLON_COLON
            | NK::SEMICOLON
    ) {
        return None;
    }

    // Content-logic delimiters read as CODE, not prose (author feedback,
    // 2026-08-25) — the native mirror of `classify_token`'s brace/pipe
    // operator arm. The prose-run guard mirrors the keyword carve-out
    // below: a brace/pipe the parser absorbed into a prose run (TEXT,
    // cue names, tag text) is literal text, not a delimiter.
    if matches!(kind, NK::L_BRACE | NK::R_BRACE | NK::PIPE) {
        if token
            .parent()
            .is_some_and(|p| is_prose_run_container(p.kind()))
        {
            return None;
        }
        return Some(Classification {
            token_type: TT_OPERATOR,
            modifiers: 0,
        });
    }

    // Narrative structure markers / flow movement / halt words — the
    // native mirrors of `classify_token`'s arms (theme ruling 2026-08-25).
    let parent_kind = token.parent().map(|p| p.kind());
    if matches!(kind, NK::STAR | NK::PLUS) && parent_kind == Some(NK::CHOICE_BULLET) {
        return Some(Classification {
            token_type: TT_MARKER,
            modifiers: 0,
        });
    }
    if matches!(kind, NK::L_BRACKET | NK::R_BRACKET)
        && matches!(
            parent_kind,
            Some(
                NK::CHOICE
                    | NK::CHOICE_START_CONTENT
                    | NK::CHOICE_BRACKET_CONTENT
                    | NK::CHOICE_INNER_CONTENT
            )
        )
    {
        return Some(Classification {
            token_type: TT_MARKER,
            modifiers: 0,
        });
    }
    if matches!(kind, NK::DIVERT | NK::THREAD | NK::GLUE) {
        return Some(Classification {
            token_type: TT_DIVERT,
            modifiers: 0,
        });
    }
    if matches!(kind, NK::KW_END | NK::KW_DONE) {
        // Same prose-run guard the keyword arm uses: `@THE END:` lexes END
        // as a keyword inside a cue name — that is prose, not a halt.
        if token
            .parent()
            .is_some_and(|p| is_prose_run_container(p.kind()))
        {
            return None;
        }
        return Some(Classification {
            token_type: TT_HALT,
            modifiers: 0,
        });
    }

    if matches!(
        kind,
        NK::LINE_COMMENT | NK::BLOCK_COMMENT | NK::DOC_COMMENT_OUTER | NK::DOC_COMMENT_INNER
    ) {
        return Some(Classification {
            token_type: TT_COMMENT,
            modifiers: 0,
        });
    }

    if kind.is_keyword() {
        // Native hard-reserves keywords everywhere at the lexer (Finding #1,
        // `syntax_kind.rs`'s keyword-section doc) — unlike ink, a prose word
        // that happens to spell a keyword (`or`, `if`, ...) still lexes as
        // that keyword token. The *parser* is what decides a run is prose,
        // folding such tokens into a `TEXT` node exactly as it does the
        // `@`/`AT` sigil (`SyntaxKind::AT`'s doc) — so the same "absorbed
        // into TEXT" guard ink's `classify_token` uses for #275 applies here
        // too, just for a different reason. Extended to every raw-bump run,
        // not only `TEXT` — `@THE END:`'s `CUE_NAME` lexes `END` as
        // `KW_END`, and a `#if only` tag lexes `if` as `KW_IF`; both are a
        // character name / tag text, not a keyword (review finding on
        // #2280/#2286).
        if token
            .parent()
            .is_some_and(|p| is_prose_run_container(p.kind()))
        {
            return None;
        }
        return Some(Classification {
            token_type: TT_KEYWORD,
            modifiers: 0,
        });
    }

    if kind == NK::AT {
        // The bare `@` sigil only means something when it opens a cue
        // (`SyntaxKind::AT`'s doc: "anywhere else... it folds into plain
        // TEXT"). A detached `@ 5pm` in prose sits directly under `TEXT`,
        // same shape as the keyword carve-out above — check the same way.
        return token
            .parent()
            .filter(|p| matches!(p.kind(), NK::CUE | NK::COMPACT_CUE))
            .map(|_| Classification {
                token_type: TT_DECORATOR,
                modifiers: 0,
            });
    }

    if kind == NK::IDENT {
        return classify_native_ident(token, resolution_index);
    }

    // Any other punctuation inside a raw-bump prose container is literal
    // text — never an operator, a divert, or a number.
    // `classify_native_fixed_kind` below has no parent access at all, so it
    // cannot tell these apart from a real token of the same kind on its
    // own; check parent-awareness here first. Originally scoped to just
    // `MINUS | INTEGER | FLOAT` (`INT. HALL - DAY`'s `-`, `@JEAN-LUC`'s `-`;
    // review finding on #2280/#2286, the same hole as the keyword
    // carve-out above). #2293 widens it to every kind reaching this point:
    // `text_run_until`'s stop set doesn't exclude `!`/`?`/`=`/etc, so
    // "Wait! Really?" inside `TEXT` still hit the operator arm below, and
    // `tag()`'s raw-bump loop is even less restrictive — it doesn't stop on
    // `DIVERT`/`GLUE` either, so a `#tag -> text` tag body could still
    // paint `->` as an operator.
    //
    // `HASH`/`AT_L_BRACKET` are exempted: `tag()` (`content.rs`) makes
    // `HASH` a *direct child of `TAG`* — the same node this guard treats as
    // a prose container — so the sigil itself would otherwise be swallowed
    // as prose and never reach `classify_native_fixed_kind`'s decorator arm
    // below (review finding on #2293: the widened guard regressed every
    // tag's own `#`/`@[` sigil colour, which #2280/#2286 had established).
    // The sigil is structure, not prose — only the *text after* it is.
    if !matches!(kind, NK::HASH | NK::AT_L_BRACKET)
        && token
            .parent()
            .is_some_and(|p| is_prose_run_container(p.kind()))
    {
        return None;
    }

    classify_native_fixed_kind(kind)
}

/// The context-free half of [`classify_native_token`] — every `SyntaxKind`
/// whose colour never depends on where the token sits in the tree. Split out
/// only to keep each function under the pedantic line-count lint; there is
/// no semantic reason these couldn't be one big `match`.
fn classify_native_fixed_kind(kind: brink_syntax_native::SyntaxKind) -> Option<Classification> {
    use brink_syntax_native::SyntaxKind as NK;

    if matches!(kind, NK::INTEGER | NK::FLOAT) {
        return Some(Classification {
            token_type: TT_NUMBER,
            modifiers: 0,
        });
    }

    if matches!(kind, NK::STRING_TEXT | NK::STRING_ESCAPE | NK::QUOTE) {
        return Some(Classification {
            token_type: TT_STRING,
            modifiers: 0,
        });
    }

    if matches!(
        kind,
        NK::TILDE
            | NK::EQ
            | NK::EQ_EQ
            | NK::BANG_EQ
            | NK::LT
            | NK::GT
            | NK::LT_EQ
            | NK::GT_EQ
            | NK::PLUS
            | NK::MINUS
            | NK::STAR
            | NK::SLASH
            | NK::PERCENT
            | NK::CARET
            | NK::BANG
            | NK::QUESTION
            | NK::AMP
            | NK::AMP_AMP
            | NK::PLUS_EQ
            | NK::MINUS_EQ
            | NK::STAR_EQ
            | NK::SLASH_EQ
            | NK::FAT_ARROW
    ) {
        return Some(Classification {
            token_type: TT_OPERATOR,
            modifiers: 0,
        });
    }

    if kind == NK::HASH || kind == NK::AT_L_BRACKET {
        return Some(Classification {
            token_type: TT_DECORATOR,
            modifiers: 0,
        });
    }

    None
}

fn classify_native_ident(
    token: &brink_syntax_native::SyntaxToken,
    resolution_index: &BTreeMap<(u32, u32), SymbolKind>,
) -> Option<Classification> {
    use brink_syntax_native::SyntaxKind as NK;
    let parent = token.parent()?;

    match parent.kind() {
        NK::STRUCT_DECL => Some(Classification {
            token_type: TT_STRUCT,
            modifiers: MOD_DECLARATION,
        }),
        NK::FLOW_DECL | NK::MODULE_DECL => Some(Classification {
            token_type: TT_NAMESPACE,
            modifiers: MOD_DECLARATION,
        }),
        // `SCENE_SLUG` shares this arm deliberately: `[slug]` on a scene
        // heading names the stitch the heading declares (`element::
        // scene_stitch`'s doc: "a heading declares a stitch"), so it is the
        // same classification as any other stitch header (review finding on
        // #2280/#2286). Kept folded in rather than as its own identical arm —
        // clippy's `match_same_arms` denies the duplicate.
        NK::FN_DECL | NK::EXTERN_DECL | NK::SCENE_SLUG => Some(Classification {
            token_type: TT_FUNCTION,
            modifiers: MOD_DECLARATION,
        }),
        NK::PARAM => Some(Classification {
            token_type: TT_PARAMETER,
            modifiers: MOD_DECLARATION,
        }),
        NK::VAR_DECL | NK::LET_STMT => Some(Classification {
            token_type: TT_VARIABLE,
            modifiers: MOD_DECLARATION,
        }),
        NK::CONST_DECL => Some(Classification {
            token_type: TT_VARIABLE,
            modifiers: MOD_DECLARATION | MOD_READONLY,
        }),
        NK::FLAGS_DECL => Some(Classification {
            token_type: TT_ENUM,
            modifiers: MOD_DECLARATION,
        }),
        NK::FLAGS_MEMBER => Some(Classification {
            token_type: TT_ENUM_MEMBER,
            modifiers: MOD_DECLARATION,
        }),
        NK::STRUCT_FIELD => Some(Classification {
            token_type: TT_PROPERTY,
            modifiers: MOD_DECLARATION,
        }),
        // `LABEL`: `(name)` — a choice/gather label. `CUE_NAME`: the name
        // inside an `@NAME`/`@NAME:` block cue — both are named markers, so
        // they share `label`.
        NK::LABEL | NK::CUE_NAME => Some(Classification {
            token_type: TT_LABEL,
            modifiers: MOD_DECLARATION,
        }),
        // A type reference — the fixed-set built-ins (`int`, `string`, ...)
        // have no resolvable symbol at all, and a user `struct` name here is
        // a *use*, not the declaration `STRUCT_DECL` matches above — reuse
        // `struct`, the legend's closest entry to "type" (issue #2280's
        // table: "string -> type", and no dedicated `type` token exists).
        NK::TYPE_NAME | NK::TYPE_GENERIC => Some(Classification {
            token_type: TT_STRUCT,
            modifiers: 0,
        }),
        // `!name rest…` self-announcing dispatch sigil.
        NK::DISPATCH_NAME => Some(Classification {
            token_type: TT_FUNCTION,
            modifiers: 0,
        }),
        // `@[name(...)]` — the annotation's own name.
        NK::ANNOTATION_LINE => Some(Classification {
            token_type: TT_DECORATOR,
            modifiers: 0,
        }),
        // `@[...(arg, name = value, ...)]` — one argument's name.
        NK::ANNOTATION_ARG => Some(Classification {
            token_type: TT_PARAMETER,
            modifiers: 0,
        }),
        // `INT. TITLE - DAY` — the heading's display name. Narrative text,
        // not a symbol reference; without this arm every word fell through
        // to the resolution fallback below and rendered as a plain
        // `variable` — precisely #2280's own worked example (review finding
        // on #2280/#2286). `TEXT`/`TAG` are the same story for plain
        // dialogue/narration and `#tag` text: #2286 only closed this gap for
        // `SCENE_TITLE`, so a run-of-the-mill prose word (or a tag's own
        // text) still fell through to `variable` — the exact remainder
        // issue #2293 flagged, now folded into the same arm.
        NK::TEXT | NK::TAG | NK::SCENE_TITLE => None,
        // `PATH`/other contexts — try the resolution index.
        _ => Some(classify_native_ident_by_resolution(token, resolution_index)),
    }
}

fn classify_native_ident_by_resolution(
    token: &brink_syntax_native::SyntaxToken,
    resolution_index: &BTreeMap<(u32, u32), SymbolKind>,
) -> Classification {
    use brink_syntax_native::SyntaxKind as NK;

    // Try the token's own range first.
    if let Some(&sym_kind) = resolution_index.get(&range_key(token.text_range())) {
        return symbol_kind_to_classification(sym_kind);
    }

    // Try the immediate `PATH_SEGMENT` wrapper's range — native wraps every
    // path segment (even a single-segment path) in its own `PATH_SEGMENT`
    // node directly around the `IDENT` (no ink-style extra `IDENTIFIER`
    // indirection).
    let Some(parent) = token.parent() else {
        return GENERIC_VARIABLE;
    };
    if parent.kind() == NK::PATH_SEGMENT
        && let Some(&sym_kind) = resolution_index.get(&range_key(parent.text_range()))
    {
        return symbol_kind_to_classification(sym_kind);
    }

    // Try the enclosing `PATH` node's range — a dotted reference's
    // resolution is recorded against the *whole* path
    // (`brink_ir::ResolvedRef::range`'s frontend-agnostic, load-bearing
    // whole-path contract, pinned by #1561 for ink and confirmed to hold
    // identically for native by this issue's own decode probe).
    let path = if parent.kind() == NK::PATH {
        Some(parent.clone())
    } else if parent.kind() == NK::PATH_SEGMENT {
        parent.parent().filter(|gp| gp.kind() == NK::PATH)
    } else {
        None
    };
    if let Some(path) = path
        && let Some(&sym_kind) = resolution_index.get(&range_key(path.text_range()))
    {
        return classify_native_path_segment(&path, token, sym_kind);
    }

    GENERIC_VARIABLE
}

/// The native sibling of [`classify_path_segment`] — same "only one segment
/// actually names the resolved symbol" logic, over native's `PATH`/
/// `PATH_SEGMENT`/`IDENT` shape instead of ink's `PATH`/`IDENTIFIER`/`IDENT`.
fn classify_native_path_segment(
    path: &brink_syntax_native::SyntaxNode,
    token: &brink_syntax_native::SyntaxToken,
    sym_kind: SymbolKind,
) -> Classification {
    let named_by_tail = matches!(
        sym_kind,
        SymbolKind::Stitch | SymbolKind::ListItem | SymbolKind::Label
    );
    let segments: Vec<TextRange> = path
        .descendants_with_tokens()
        .filter_map(rowan::NodeOrToken::into_token)
        .filter(|t| t.kind() == brink_syntax_native::SyntaxKind::IDENT)
        .map(|t| t.text_range())
        .collect();
    let naming = if named_by_tail {
        segments.last()
    } else {
        segments.first()
    };
    if naming == Some(&token.text_range()) {
        return symbol_kind_to_classification(sym_kind);
    }
    if named_by_tail {
        return GENERIC_VARIABLE;
    }
    Classification {
        token_type: TT_PROPERTY,
        modifiers: 0,
    }
}

// ── Multi-line splitting ───────────────────────────────────────────

/// Generic over the rowan `Language` so both the ink (`brink_syntax`) and
/// native (`brink_syntax_native`) frontends share one line-splitting
/// implementation — the two `SyntaxKind` enums differ, but `text()`/
/// `text_range()` are plain `rowan::SyntaxToken<L>` methods.
pub fn emit_token<L: rowan::Language>(
    token: &rowan::SyntaxToken<L>,
    classification: &Classification,
    idx: &LineIndex,
    out: &mut Vec<RawToken>,
) {
    let text = token.text();
    let start_offset = token.text_range().start();

    // Fast path: single-line token
    if !text.contains('\n') {
        let (line, start_char) = idx.line_col(start_offset);
        let length = utf16_len(text);
        out.push(RawToken {
            line,
            start_char,
            length,
            token_type: classification.token_type,
            modifiers: classification.modifiers,
        });
        return;
    }

    // Multi-line: split by newlines
    let segments: Vec<&str> = text.split('\n').collect();
    let num_segments = segments.len();
    let mut byte_offset = u32::from(start_offset);
    for (i, segment) in segments.iter().enumerate() {
        if segment.is_empty() && i > 0 {
            // Empty line segment after split — skip but advance offset
            byte_offset += 1; // for the \n
            continue;
        }
        let (line, start_char) = idx.line_col(rowan::TextSize::from(byte_offset));
        let length = utf16_len(segment);
        if length > 0 {
            out.push(RawToken {
                line,
                start_char,
                length,
                token_type: classification.token_type,
                modifiers: classification.modifiers,
            });
        }
        // Advance past this segment + the \n separator
        byte_offset += u32::try_from(segment.len()).unwrap_or(u32::MAX);
        if i < num_segments - 1 {
            byte_offset += 1; // \n
        }
    }
}

fn utf16_len(s: &str) -> u32 {
    s.chars()
        .map(|c| u32::try_from(c.len_utf16()).unwrap_or(1))
        .sum()
}

// ── Delta encoding ─────────────────────────────────────────────────

/// Delta-encode raw tokens into relative positions.
pub fn delta_encode(raw_tokens: &[RawToken]) -> Vec<DeltaToken> {
    let mut result = Vec::with_capacity(raw_tokens.len());
    let mut prev_line = 0u32;
    let mut prev_start = 0u32;

    for tok in raw_tokens {
        let delta_line = tok.line - prev_line;
        let delta_start = if delta_line == 0 {
            tok.start_char - prev_start
        } else {
            tok.start_char
        };

        result.push(DeltaToken {
            delta_line,
            delta_start,
            length: tok.length,
            token_type: tok.token_type,
            token_modifiers: tok.modifiers,
        });

        prev_line = tok.line;
        prev_start = tok.start_char;
    }

    result
}

// ── Public API ─────────────────────────────────────────────────────

/// Walk one ink CST emitting classified tokens against a range-keyed
/// resolution-kind map (#3064 B4). The map's keys are `(start, end)`
/// tuples in the SAME coordinate space as `root`/`source` — whole-file
/// callers pass absolute keys, per-segment callers fragment-relative
/// ones. Positions in the returned tokens are line/column within
/// `source`.
#[must_use]
pub fn tokens_with_kinds(
    source: &str,
    root: &brink_syntax::SyntaxNode,
    kinds: &BTreeMap<(u32, u32), SymbolKind>,
) -> Vec<RawToken> {
    let idx = LineIndex::new(source);
    let mut raw_tokens = Vec::new();
    for element in root.descendants_with_tokens() {
        let token = match element {
            rowan::NodeOrToken::Token(t) => t,
            rowan::NodeOrToken::Node(_) => continue,
        };
        if let Some(classification) = classify_token(&token, kinds) {
            emit_token(&token, &classification, &idx, &mut raw_tokens);
        }
    }
    raw_tokens
}

/// The native sibling of [`tokens_with_kinds`].
#[must_use]
pub fn tokens_with_kinds_native(
    source: &str,
    root: &brink_syntax_native::SyntaxNode,
    kinds: &BTreeMap<(u32, u32), SymbolKind>,
) -> Vec<RawToken> {
    let idx = LineIndex::new(source);
    let mut raw_tokens = Vec::new();
    for element in root.descendants_with_tokens() {
        let token = match element {
            rowan::NodeOrToken::Token(t) => t,
            rowan::NodeOrToken::Node(_) => continue,
        };
        if let Some(classification) = classify_native_token(&token, kinds) {
            emit_token(&token, &classification, &idx, &mut raw_tokens);
        }
    }
    raw_tokens
}
