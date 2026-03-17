use brink_ir::SymbolKind;

/// What kind of completion context the cursor is in.
pub enum CompletionContext {
    /// After `->` — show divert targets.
    Divert,
    /// After `knot_name.` — show children of that knot.
    DottedPath { knot: String },
    /// Inside `{ }` — inline expression.
    InlineExpr,
    /// On a `~` logic line.
    Logic,
    /// Inside `( )` — function arguments.
    FunctionArgs,
    /// Default — show everything.
    General,
}

/// Determine the completion context by scanning backwards from the cursor.
pub fn detect_completion_context(source: &str, byte_offset: usize) -> CompletionContext {
    // Find line start.
    let line_start = source[..byte_offset].rfind('\n').map_or(0, |pos| pos + 1);
    let line_prefix = &source[line_start..byte_offset];
    let trimmed = line_prefix.trim_start();

    let is_logic_line = trimmed.starts_with('~');

    // Scan backwards through the line prefix for context clues.
    // More specific contexts (parens, braces, divert) take priority over the
    // logic-line fallback.
    let bytes = line_prefix.as_bytes();
    let mut brace_depth: i32 = 0;
    let mut paren_depth: i32 = 0;
    let mut i = bytes.len();

    while i > 0 {
        i -= 1;
        match bytes[i] {
            b'}' => brace_depth += 1,
            b'{' => {
                if brace_depth > 0 {
                    brace_depth -= 1;
                } else {
                    return CompletionContext::InlineExpr;
                }
            }
            b')' => paren_depth += 1,
            b'(' => {
                if paren_depth > 0 {
                    paren_depth -= 1;
                } else {
                    return CompletionContext::FunctionArgs;
                }
            }
            b'>' if i > 0 && bytes[i - 1] == b'-' && brace_depth == 0 && paren_depth == 0 => {
                return CompletionContext::Divert;
            }
            b'.' if brace_depth == 0 && paren_depth == 0 => {
                // Check for identifier before the dot.
                let before_dot = &line_prefix[..i];
                let ident_start = before_dot
                    .rfind(|c: char| !c.is_alphanumeric() && c != '_')
                    .map_or(0, |p| p + 1);
                let knot = &before_dot[ident_start..];
                if !knot.is_empty() {
                    return CompletionContext::DottedPath {
                        knot: knot.to_owned(),
                    };
                }
            }
            _ => {}
        }
    }

    if is_logic_line {
        return CompletionContext::Logic;
    }

    CompletionContext::General
}

/// The scope (knot/stitch) containing the cursor.
pub struct CursorScope {
    pub knot: Option<String>,
    pub stitch: Option<String>,
}

/// Determine which knot/stitch the cursor is inside.
pub fn cursor_scope(source: &str, byte_offset: usize) -> CursorScope {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();
    let cursor = rowan::TextSize::from(u32::try_from(byte_offset).unwrap_or(u32::MAX));

    let mut result = CursorScope {
        knot: None,
        stitch: None,
    };

    for knot in tree.knots() {
        let range = knot.syntax().text_range();
        if cursor < range.start() || cursor > range.end() {
            continue;
        }
        result.knot = knot.header().and_then(|h| h.name());

        if let Some(body) = knot.body() {
            for stitch in body.stitches() {
                let sr = stitch.syntax().text_range();
                if cursor >= sr.start() && cursor <= sr.end() {
                    result.stitch = stitch.header().and_then(|h| h.name());
                    break;
                }
            }
        }
        break;
    }

    result
}

/// Check whether a symbol should be shown in the given completion context.
pub fn is_visible_in_context(
    ctx: &CompletionContext,
    info: &brink_ir::SymbolInfo,
    scope: &CursorScope,
) -> bool {
    // Scope filter: locals are only visible if we're in their scope.
    if matches!(info.kind, SymbolKind::Param | SymbolKind::Temp)
        && let Some(ref sym_scope) = info.scope
    {
        let knot_matches = scope.knot.as_deref() == sym_scope.knot.as_deref();
        let stitch_visible =
            sym_scope.stitch.is_none() || scope.stitch.as_deref() == sym_scope.stitch.as_deref();
        if !knot_matches || !stitch_visible {
            return false;
        }
    }

    match ctx {
        CompletionContext::Divert => {
            matches!(
                info.kind,
                SymbolKind::Knot | SymbolKind::Stitch | SymbolKind::Label
            ) || (info.kind == SymbolKind::Param
                && info.param_detail.as_ref().is_some_and(|p| p.is_divert))
        }
        CompletionContext::DottedPath { .. } => {
            // Handled separately in the caller.
            false
        }
        CompletionContext::InlineExpr => matches!(
            info.kind,
            SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Param
                | SymbolKind::Temp
                | SymbolKind::List
                | SymbolKind::ListItem
                | SymbolKind::Knot
                | SymbolKind::External
        ),
        CompletionContext::Logic => matches!(
            info.kind,
            SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Param
                | SymbolKind::Temp
                | SymbolKind::External
        ),
        CompletionContext::FunctionArgs => matches!(
            info.kind,
            SymbolKind::Variable
                | SymbolKind::Constant
                | SymbolKind::Param
                | SymbolKind::Temp
                | SymbolKind::ListItem
        ),
        CompletionContext::General => true,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn context_divert() {
        let src = "-> ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Divert
        ));
    }

    #[test]
    fn context_divert_no_space() {
        let src = "->";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Divert
        ));
    }

    #[test]
    fn context_divert_partial() {
        let src = "-> kno";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Divert
        ));
    }

    #[test]
    fn context_dotted_path() {
        let src = "-> my_knot.";
        let ctx = detect_completion_context(src, src.len());
        assert!(matches!(ctx, CompletionContext::DottedPath { ref knot } if knot == "my_knot"));
    }

    #[test]
    fn context_inline_expr() {
        let src = "Hello {";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::InlineExpr
        ));
    }

    #[test]
    fn context_inline_expr_nested() {
        let src = "Hello {x + ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::InlineExpr
        ));
    }

    #[test]
    fn context_logic_line() {
        let src = "~ x = ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Logic
        ));
    }

    #[test]
    fn context_logic_line_indented() {
        let src = "    ~ temp x = ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::Logic
        ));
    }

    #[test]
    fn context_function_args() {
        let src = "~ foo(";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::FunctionArgs
        ));
    }

    #[test]
    fn context_function_args_partial() {
        let src = "~ foo(x, ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::FunctionArgs
        ));
    }

    #[test]
    fn context_general() {
        let src = "Hello world ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::General
        ));
    }

    #[test]
    fn context_closed_braces_is_general() {
        // Braces are balanced — not inside an expression.
        let src = "{x} and then ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::General
        ));
    }

    #[test]
    fn cursor_scope_in_knot() {
        let src = "=== my_knot ===\nSome text\n";
        let offset = src.find("Some").unwrap_or(src.len());
        let scope = cursor_scope(src, offset);
        assert_eq!(scope.knot.as_deref(), Some("my_knot"));
        assert!(scope.stitch.is_none());
    }

    #[test]
    fn cursor_scope_in_stitch() {
        let src = "=== my_knot ===\n= my_stitch\nSome text\n";
        let offset = src.find("Some").unwrap_or(src.len());
        let scope = cursor_scope(src, offset);
        assert_eq!(scope.knot.as_deref(), Some("my_knot"));
        assert_eq!(scope.stitch.as_deref(), Some("my_stitch"));
    }

    #[test]
    fn cursor_scope_top_level() {
        let src = "Some text before any knot\n";
        let scope = cursor_scope(src, 5);
        assert!(scope.knot.is_none());
        assert!(scope.stitch.is_none());
    }
}
