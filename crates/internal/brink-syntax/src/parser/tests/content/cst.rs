use crate::parser::tests::cst::{ExpectedNode, assert_equivalent};
use crate::{SyntaxKind, parse};

// ── Section A: Plain text ──────────────────────────────────────────

/// `Hello\n` — single word aggregates into one TEXT.
#[test]
fn text_single_word() {
    assert_equivalent(
        parse("Hello\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// `Hello world\n` — multiple words aggregate into one TEXT.
#[test]
fn text_multiple_words() {
    assert_equivalent(
        parse("Hello world\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Period is text.
#[test]
fn text_with_period() {
    assert_equivalent(
        parse("Hello.\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Comma is text.
#[test]
fn text_with_comma() {
    assert_equivalent(
        parse("Hello, world\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Exclamation is text.
#[test]
fn text_with_exclamation() {
    assert_equivalent(
        parse("It works!\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Question mark is text.
#[test]
fn text_with_question() {
    assert_equivalent(
        parse("How are you?\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Colon is text.
#[test]
fn text_with_colon() {
    assert_equivalent(
        parse("Name: Bob\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Semicolon is text.
#[test]
fn text_with_semicolon() {
    assert_equivalent(
        parse("A; B\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Parentheses are text.
#[test]
fn text_with_parentheses() {
    assert_equivalent(
        parse("Hello (world)\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Quotes are text.
#[test]
fn text_with_quotes() {
    assert_equivalent(
        parse("She said \"hello\"\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Numbers are text.
#[test]
fn text_with_numbers() {
    assert_equivalent(
        parse("Player 1\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// EOF terminates content line without error.
#[test]
fn text_at_eof() {
    assert_equivalent(
        parse("Hello"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Equals sign is text.
#[test]
fn text_with_equals() {
    assert_equivalent(
        parse("A = B\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

// ── Section B: Glue ────────────────────────────────────────────────

/// `a<>b\n` — glue between two text runs.
#[test]
fn glue_between_text() {
    assert_equivalent(
        parse("a<>b\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// `hello <>world\n` — glue with surrounding whitespace.
#[test]
fn glue_with_spaces() {
    assert_equivalent(
        parse("hello <>world\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// `<>continued\n` — glue at start of line.
#[test]
fn glue_at_start() {
    assert_equivalent(
        parse("<>continued\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// `text<>\n` — glue at end of content.
#[test]
fn glue_at_end() {
    assert_equivalent(
        parse("text<>\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    GLUE_NODE
                }
            }
        }),
    );
}

/// `a<>b<>c\n` — multiple glues.
#[test]
fn multiple_glues() {
    assert_equivalent(
        parse("a<>b<>c\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// `<><>\n` — consecutive glues with no text between.
#[test]
fn consecutive_glues() {
    assert_equivalent(
        parse("<><>\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    GLUE_NODE
                    GLUE_NODE
                }
            }
        }),
    );
}

/// `<>\n` — single glue only.
#[test]
fn glue_only() {
    assert_equivalent(
        parse("<>\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    GLUE_NODE
                }
            }
        }),
    );
}

/// `text<> -> knot\n` — glue before divert.
#[test]
fn glue_before_divert() {
    assert_equivalent(
        parse("text<> -> knot\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    GLUE_NODE
                }
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

// ── Section C: Escapes ─────────────────────────────────────────────

/// `\# not a tag\n` — escaped hash.
#[test]
fn escape_hash() {
    assert_equivalent(
        parse("\\# not a tag\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `\{ not logic\n` — escaped open brace.
#[test]
fn escape_open_brace() {
    assert_equivalent(
        parse("\\{ not logic\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `\\ text\n` — escaped backslash.
#[test]
fn escape_backslash() {
    assert_equivalent(
        parse("\\\\ text\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `\| not branch\n` — escaped pipe.
#[test]
fn escape_pipe() {
    assert_equivalent(
        parse("\\| not branch\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `Hello \# world\n` — escape in the middle of text.
#[test]
fn escape_mid_text() {
    assert_equivalent(
        parse("Hello \\# world\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `Hello \#\n` — escape at end of content.
#[test]
fn escape_at_end() {
    assert_equivalent(
        parse("Hello \\#\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    ESCAPE
                }
            }
        }),
    );
}

/// `\# tag\n` — escape at start of content.
#[test]
fn escape_at_start() {
    assert_equivalent(
        parse("\\# tag\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

/// `\# and \{\n` — multiple escapes with text between.
#[test]
fn multiple_escapes() {
    assert_equivalent(
        parse("\\# and \\{\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    TEXT
                    ESCAPE
                }
            }
        }),
    );
}

/// `\#\{\n` — consecutive escapes with no text between.
#[test]
fn consecutive_escapes() {
    assert_equivalent(
        parse("\\#\\{\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    ESCAPE
                }
            }
        }),
    );
}

/// `\} text\n` — escaped close brace.
#[test]
fn escape_close_brace() {
    assert_equivalent(
        parse("\\} text\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

// ── Section D: Mixed element combinations ──────────────────────────

/// Text + escape.
#[test]
fn text_then_escape() {
    assert_equivalent(
        parse("Hello \\#\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    ESCAPE
                }
            }
        }),
    );
}

/// Glue + escape.
#[test]
fn glue_then_escape() {
    assert_equivalent(
        parse("<>\\#\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    GLUE_NODE
                    ESCAPE
                }
            }
        }),
    );
}

/// Escape + glue + text.
#[test]
fn escape_then_glue() {
    assert_equivalent(
        parse("\\#<>text\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    ESCAPE
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// Text + escape + glue + text.
#[test]
fn text_escape_glue_text() {
    assert_equivalent(
        parse("a\\#<>b\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    ESCAPE
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// Text + inline logic + text.
#[test]
fn text_inline_text() {
    assert_equivalent(
        parse("Hello {x} world\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                }
            }
        }),
    );
}

/// Glue + inline logic + glue.
#[test]
fn glue_inline_glue() {
    assert_equivalent(
        parse("<>{x}<>\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    GLUE_NODE
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    GLUE_NODE
                }
            }
        }),
    );
}

/// Inline logic at start of content.
#[test]
fn inline_at_start() {
    assert_equivalent(
        parse("{x} world\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                }
            }
        }),
    );
}

/// Inline logic at end of content.
#[test]
fn inline_at_end() {
    assert_equivalent(
        parse("Hello {x}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

// ── Section E: Content line composition ────────────────────────────

/// Content only — no divert, no tags.
#[test]
fn content_only() {
    assert_equivalent(
        parse("Hello\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Content + divert.
#[test]
fn content_with_divert() {
    assert_equivalent(
        parse("Hello -> knot\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// Content + single tag.
#[test]
fn content_with_tag() {
    assert_equivalent(
        parse("Hello #tag\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// Content + two tags.
#[test]
fn content_with_two_tags() {
    assert_equivalent(
        parse("Hello #tag1 #tag2\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
                TAGS {
                    TAG
                    TAG
                }
            }
        }),
    );
}

/// Content + divert + tags.
#[test]
fn content_divert_tags() {
    assert_equivalent(
        parse("Hello -> knot #tag\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// Bare divert — no `MIXED_CONTENT` child.
#[test]
fn divert_only() {
    assert_equivalent(
        parse("-> knot\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// Divert + tags (no content text).
#[test]
fn divert_with_tags() {
    assert_equivalent(
        parse("-> knot #tag\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
                TAGS {
                    TAG
                }
            }
        }),
    );
}

/// Content + tunnel call.
#[test]
fn content_then_tunnel_call() {
    assert_equivalent(
        parse("Hello -> target ->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// Content + tunnel call + tunnel onwards.
#[test]
fn content_then_tunnel_call_onwards() {
    assert_equivalent(
        parse("Hello -> target ->->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE
                }
            }
        }),
    );
}

/// Divert tunnel call + tunnel onwards (no content text).
#[test]
fn divert_tunnel_call_onwards() {
    assert_equivalent(
        parse("-> target ->->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_CALL_NODE {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE
                }
            }
        }),
    );
}

/// Bare tunnel onwards.
#[test]
fn tunnel_onwards_only() {
    assert_equivalent(
        parse("->->\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                DIVERT_NODE {
                    TUNNEL_ONWARDS_NODE
                }
            }
        }),
    );
}

/// Content at EOF (no trailing newline).
#[test]
fn content_at_eof() {
    assert_equivalent(
        parse("Hello"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Glue + text + divert + tags — full composition.
#[test]
fn glue_content_divert_tags() {
    assert_equivalent(
        parse("Hi<>there -> knot #tag\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                }
                DIVERT_NODE {
                    SIMPLE_DIVERT {
                        DIVERT_TARGET_WITH_ARGS {
                            PATH
                        }
                    }
                }
                TAGS {
                    TAG
                }
            }
        }),
    );
}

// ── Section F: Inline logic in content (integration) ───────────────

/// Inline bare expression.
#[test]
fn inline_bare_expr() {
    assert_equivalent(
        parse("Hello {x}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// Inline conditional.
#[test]
fn inline_conditional() {
    assert_equivalent(
        parse("Hello {x: yes}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        CONDITIONAL_WITH_EXPR {
                            PATH
                            INLINE_BRANCHES_COND {
                                BRANCH_CONTENT {
                                    TEXT
                                }
                            }
                        }
                    }
                }
            }
        }),
    );
}

/// Inline between text runs.
#[test]
fn inline_between_text() {
    assert_equivalent(
        parse("before {x} after\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                }
            }
        }),
    );
}

/// Multiple inline logic blocks.
#[test]
fn multiple_inlines() {
    assert_equivalent(
        parse("{a} and {b}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                    TEXT
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

/// Inline with glue.
#[test]
fn inline_with_glue() {
    assert_equivalent(
        parse("<>{x}\n"),
        cst!(SOURCE_FILE {
            CONTENT_LINE {
                MIXED_CONTENT {
                    GLUE_NODE
                    INLINE_LOGIC {
                        INNER_EXPRESSION {
                            PATH
                        }
                    }
                }
            }
        }),
    );
}

// ── Section G: Content in context ──────────────────────────────────

/// Content inside a knot body.
#[test]
fn content_in_knot_body() {
    assert_equivalent(
        parse("=== myKnot ===\nHello.\n"),
        cst!(SOURCE_FILE {
            KNOT_DEF {
                KNOT_HEADER {
                    IDENTIFIER
                }
                KNOT_BODY {
                    CONTENT_LINE {
                        MIXED_CONTENT {
                            TEXT
                        }
                    }
                }
            }
        }),
    );
}

/// Choice with content text.
#[test]
fn choice_with_content() {
    assert_equivalent(
        parse("* Hello\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Gather with content text.
#[test]
fn gather_with_content() {
    assert_equivalent(
        parse("- Hello\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                MIXED_CONTENT {
                    TEXT
                }
            }
        }),
    );
}

/// Choice content with glue.
#[test]
fn choice_content_with_glue() {
    assert_equivalent(
        parse("* Hello<>world\n"),
        cst!(SOURCE_FILE {
            CHOICE {
                CHOICE_BULLETS
                CHOICE_START_CONTENT {
                    TEXT
                    GLUE_NODE
                    TEXT
                }
            }
        }),
    );
}

/// Gather content with escape.
#[test]
fn gather_content_with_escape() {
    assert_equivalent(
        parse("- Hello \\# tag\n"),
        cst!(SOURCE_FILE {
            GATHER {
                GATHER_DASHES
                MIXED_CONTENT {
                    TEXT
                    ESCAPE
                    TEXT
                }
            }
        }),
    );
}

// ── Section H: Structural invariants ───────────────────────────────

const MIXED_CONTENT_CHILDREN: [SyntaxKind; 4] = [
    SyntaxKind::TEXT,
    SyntaxKind::GLUE_NODE,
    SyntaxKind::ESCAPE,
    SyntaxKind::INLINE_LOGIC,
];

/// Assert structural invariants for content parsing:
/// - Every `CONTENT_LINE` has at most one `MIXED_CONTENT` child.
/// - Every `MIXED_CONTENT` has at least one composite child.
/// - `TEXT`, `ESCAPE`, and `GLUE_NODE` are leaf containers (no composite children).
fn assert_content_invariants(src: &str) {
    let p = parse(src);
    assert!(p.errors().is_empty(), "unexpected errors: {:?}", p.errors());

    for node in p.syntax().descendants() {
        match node.kind() {
            SyntaxKind::CONTENT_LINE => {
                let mixed_count = node
                    .children()
                    .filter(|c| c.kind() == SyntaxKind::MIXED_CONTENT)
                    .count();
                assert!(
                    mixed_count <= 1,
                    "CONTENT_LINE should have at most 1 MIXED_CONTENT, found {mixed_count} in `{src}`"
                );
            }
            SyntaxKind::MIXED_CONTENT => {
                let composite_count = node
                    .children()
                    .filter(|c| MIXED_CONTENT_CHILDREN.contains(&c.kind()))
                    .count();
                assert!(
                    composite_count >= 1,
                    "MIXED_CONTENT should have at least one composite child, found 0 in `{src}`"
                );
            }
            SyntaxKind::TEXT | SyntaxKind::ESCAPE | SyntaxKind::GLUE_NODE => {
                let composite_children: Vec<_> = node.children().map(|c| c.kind()).collect();
                assert!(
                    composite_children.is_empty(),
                    "{:?} should have no composite children, found {:?} in `{src}`",
                    node.kind(),
                    composite_children,
                );
            }
            _ => {}
        }
    }
}

#[test]
fn invariants_plain_text() {
    assert_content_invariants("Hello\n");
}

#[test]
fn invariants_glue() {
    assert_content_invariants("a<>b\n");
}

#[test]
fn invariants_escape() {
    assert_content_invariants("\\# tag\n");
}

#[test]
fn invariants_inline() {
    assert_content_invariants("Hello {x}\n");
}

#[test]
fn invariants_mixed() {
    assert_content_invariants("Hello \\# <>world {x}\n");
}

#[test]
fn invariants_divert() {
    assert_content_invariants("Hello -> knot\n");
}

#[test]
fn invariants_tags() {
    assert_content_invariants("Hello #tag\n");
}

#[test]
fn invariants_all() {
    assert_content_invariants("Hello \\# <>world {x} -> knot #tag\n");
}

#[test]
fn invariants_multiple_lines() {
    assert_content_invariants("Line 1.\nLine 2.\n");
}

#[test]
fn invariants_eof_no_newline() {
    assert_content_invariants("Hello");
}

// ── Section I: Error recovery ──────────────────────────────────────

/// Backslash at EOF — cannot form escape.
#[test]
fn backslash_at_eof() {
    let src = "\\";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// Backslash before newline — cannot form escape.
#[test]
fn backslash_before_newline() {
    let src = "\\\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}

/// Unclosed inline logic — missing `}`.
#[test]
fn unclosed_inline_logic() {
    let src = "Hello {name\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
    assert!(
        !p.errors().is_empty(),
        "expected parse error for unclosed inline logic"
    );
}

/// Stray `}` in content — `}` is a text stop char.
#[test]
fn stray_rbrace_in_content() {
    let src = "Hello } world\n";
    let p = parse(src);
    assert_eq!(src, p.syntax().text().to_string(), "lossless round-trip");
}
