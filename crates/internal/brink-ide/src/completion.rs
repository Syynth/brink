use brink_ir::SymbolKind;

/// What kind of completion context the cursor is in.
#[derive(Debug)]
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
    /// Inside the target position of `#fn(` — the T1c creation site's first
    /// argument, which is always a statically-named function (spec §2, "the
    /// static target path — a name, never an expression"). Distinct from
    /// [`CompletionContext::FunctionArgs`] so completion offers only
    /// function-definition names here, not every value symbol in scope.
    FnTarget,
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
    // Whether a `,` was seen at the current (unmatched) paren's top level —
    // i.e. the cursor sits *after* the first argument, not in it. Only
    // matters for distinguishing `#fn(` (first-arg target position) from
    // `#fn(name, ` (an ordinary bound-argument position, ` `).
    let mut saw_comma_at_top = false;
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
                    let before = &line_prefix[..i];
                    if !saw_comma_at_top && before.trim_end().ends_with("#fn") {
                        return CompletionContext::FnTarget;
                    }
                    return CompletionContext::FunctionArgs;
                }
            }
            b',' if brace_depth == 0 && paren_depth == 0 => {
                saw_comma_at_top = true;
            }
            b'>' if i > 0 && bytes[i - 1] == b'-' && brace_depth == 0 && paren_depth == 0 => {
                return CompletionContext::Divert;
            }
            b'.' if brace_depth == 0 && paren_depth == 0 => {
                // Check for identifier before the dot.
                let before_dot = &line_prefix[..i];
                let ident_start = before_dot
                    .char_indices()
                    .rev()
                    .find(|(_, c)| !c.is_alphanumeric() && *c != '_')
                    .map_or(0, |(p, c)| p + c.len_utf8());
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

/// The T1e `ref lvalue-path` **root** completion position
/// (`docs/t1e-spec.md` §2, issue #850) — the cursor sits right after typing
/// `ref` and at least one space, with at most a partial bare identifier
/// typed since (`heal(ref `, `heal(ref np`). Returns the partial identifier
/// typed so far (possibly empty).
///
/// `None` once a `.` or `[` has been typed — that's a path *continuation*
/// (`ref npc.`, `ref inventory[`), which needs the root's resolved
/// struct/collection shape to complete usefully. This helper only detects
/// the root position, deliberately not the continuation: "completion after
/// `ref ` where cheap" (issue #850) scopes the offer to what a pure lexical
/// scan can answer — same "cheap" contract [`detect_completion_context`]
/// itself follows — not a shape/type resolution pass.
#[must_use]
pub fn ref_arg_root_prefix(source: &str, byte_offset: usize) -> Option<String> {
    let line_start = source[..byte_offset].rfind('\n').map_or(0, |pos| pos + 1);
    let line_prefix = &source[line_start..byte_offset];

    // Walk back over the in-progress bare identifier (if any).
    let ident_start = line_prefix
        .char_indices()
        .rev()
        .find(|(_, c)| !(c.is_alphanumeric() || *c == '_'))
        .map_or(0, |(p, c)| p + c.len_utf8());
    let prefix = &line_prefix[ident_start..];
    let before = line_prefix[..ident_start].trim_end();

    if !before.ends_with("ref") {
        return None;
    }
    // Word-boundary check on "ref" itself — `prefer np` must not match.
    let ref_start = before.len() - 3;
    if ref_start > 0 {
        let prev = before.as_bytes()[ref_start - 1];
        if prev.is_ascii_alphanumeric() || prev == b'_' {
            return None;
        }
    }
    Some(prefix.to_owned())
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
        // T1c (docs/t1c-spec.md §2): `#fn`'s target must be a statically-named
        // `=== function ... ===` — the same shape `brink_analyzer::fn_values`
        // enforces as E079. Only function-tagged knots qualify; a plain knot,
        // stitch, or any value symbol is never a legal `#fn` target.
        CompletionContext::FnTarget => {
            info.kind == SymbolKind::Knot && info.detail.as_deref() == Some("function")
        }
        CompletionContext::General => true,
    }
}

/// Whether `ctx` is a call-expression position where a T1b stdlib slice 1
/// call is legal (docs/t1b-surface-spec.md §5 — expression position only:
/// `~` lines, block statements, call arguments, condition expressions).
/// Mirrors the contexts [`is_visible_in_context`] already offers real
/// function symbols (`SymbolKind::Knot`/`External`) in: [`CompletionContext::Logic`]
/// and [`CompletionContext::InlineExpr`].
#[must_use]
pub fn stdlib_completion_context(ctx: &CompletionContext) -> bool {
    matches!(
        ctx,
        CompletionContext::Logic | CompletionContext::InlineExpr
    )
}

/// Stdlib slice 1 completion candidates for `ctx` under `dialect`
/// (docs/t1b-surface-spec.md §5). Empty outside a call-expression position,
/// and empty under [`brink_analyzer::Dialect::StrictInk`] — "Names live in
/// the brink dialect only; strict-ink projects never see them" (§5). Callers
/// still need their own shadowing filter: an author-defined symbol of the
/// same name is offered too (from the ordinary symbol-index completion
/// list), matching §5's shadow-with-warning rule.
#[must_use]
pub fn stdlib_completions(
    ctx: &CompletionContext,
    dialect: brink_analyzer::Dialect,
) -> &'static [crate::stdlib::StdlibFn] {
    if dialect != brink_analyzer::Dialect::Brink || !stdlib_completion_context(ctx) {
        return &[];
    }
    crate::stdlib::STDLIB_FUNCTIONS
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
    fn context_dotted_path_no_panic_on_multibyte_delimiter() {
        // Same multibyte-delimiter hazard as `ref_arg_root_prefix`, but for
        // the dotted-path scan in `detect_completion_context`: a smart
        // apostrophe or other multibyte char sitting right before the dot's
        // preceding identifier must not panic on a non-char-boundary slice.
        for src in ["-> it’s.knot.", "-> He said—word."] {
            let _ = detect_completion_context(src, src.len());
        }
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

    // ── T1e (docs/t1e-spec.md §2, issue #850): `ref` root completion ──────

    #[test]
    fn ref_root_right_after_keyword_and_space() {
        let src = "~ heal(ref ";
        assert_eq!(ref_arg_root_prefix(src, src.len()), Some(String::new()));
    }

    #[test]
    fn ref_root_with_partial_identifier() {
        let src = "~ heal(ref np";
        assert_eq!(ref_arg_root_prefix(src, src.len()), Some("np".to_owned()));
    }

    #[test]
    fn ref_root_inside_fn_literal_bound_args() {
        // `#fn(heal, ref npc)` — the bound-arg position after the target,
        // same `CompletionContext::FunctionArgs` classification a plain
        // call's ref-argument position gets.
        let src = "~ temp f = #fn(heal, ref np";
        assert_eq!(ref_arg_root_prefix(src, src.len()), Some("np".to_owned()));
    }

    #[test]
    fn ref_root_none_without_ref_keyword() {
        let src = "~ heal(np";
        assert_eq!(ref_arg_root_prefix(src, src.len()), None);
    }

    #[test]
    fn ref_root_none_for_word_ending_in_ref() {
        // `xref` ends in the three letters "ref" but isn't the `ref`
        // keyword — the word-boundary check must reject it.
        let src = "~ heal(xref np";
        assert_eq!(ref_arg_root_prefix(src, src.len()), None);
    }

    #[test]
    fn ref_root_none_once_a_dot_continuation_has_started() {
        // `ref npc.` is a path *continuation*, not the root position — this
        // helper deliberately only answers the root ("where cheap", #850).
        let src = "~ heal(ref npc.h";
        assert_eq!(ref_arg_root_prefix(src, src.len()), None);
    }

    #[test]
    fn ref_root_none_once_a_bracket_continuation_has_started() {
        let src = "~ bump(ref inventory[";
        assert_eq!(ref_arg_root_prefix(src, src.len()), None);
    }

    #[test]
    fn ref_root_no_panic_on_multibyte_delimiter_before_identifier() {
        // A multibyte delimiter (smart apostrophe, em-dash, ellipsis) sitting
        // directly before the in-progress identifier must not panic: the
        // scan has to advance by the delimiter's real UTF-8 width, not
        // assume it's one byte.
        for src in ["~ heal(it’np", "~ heal(He said—np", "~ heal(Wait…np"] {
            let _ = ref_arg_root_prefix(src, src.len());
        }
    }

    // ── T1c-4 (#702): `#fn(` target completion (docs/t1c-spec.md §2) ─────

    #[test]
    fn context_fn_literal_target() {
        let src = "~ temp f = #fn(";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::FnTarget
        ));
    }

    #[test]
    fn context_fn_literal_target_partial_name() {
        let src = "~ temp f = #fn(hea";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::FnTarget
        ));
    }

    #[test]
    fn context_fn_literal_bound_arg_is_ordinary_function_args() {
        // Past the first comma — a bound-argument position, not the target.
        let src = "~ temp f = #fn(heal, ";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::FunctionArgs
        ));
    }

    #[test]
    fn context_plain_call_is_not_fn_target() {
        // An ordinary call — `#fn` specifically gates this, not just any
        // trailing `fn`-like identifier.
        let src = "~ temp f = heal(";
        assert!(matches!(
            detect_completion_context(src, src.len()),
            CompletionContext::FunctionArgs
        ));
    }

    #[test]
    fn fn_target_context_offers_only_function_definitions() {
        let scope = CursorScope {
            knot: None,
            stitch: None,
        };
        let function_knot = brink_ir::SymbolInfo {
            kind: SymbolKind::Knot,
            file: brink_ir::FileId(0),
            range: rowan::TextRange::new(0.into(), 1.into()),
            id: brink_format::DefinitionId::new(brink_format::DefinitionTag::Address, 1),
            name: "heal".to_owned(),
            params: vec![],
            detail: Some("function".to_owned()),
            scope: None,
            param_detail: None,
            module: None,
            visibility: brink_ir::Visibility::Public,
        };
        let plain_knot = brink_ir::SymbolInfo {
            detail: None,
            ..function_knot.clone()
        };
        let variable = brink_ir::SymbolInfo {
            kind: SymbolKind::Variable,
            detail: None,
            ..function_knot.clone()
        };
        assert!(is_visible_in_context(
            &CompletionContext::FnTarget,
            &function_knot,
            &scope
        ));
        assert!(!is_visible_in_context(
            &CompletionContext::FnTarget,
            &plain_knot,
            &scope
        ));
        assert!(!is_visible_in_context(
            &CompletionContext::FnTarget,
            &variable,
            &scope
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

    // ── Stdlib slice 1 completion (#589) ────────────────────────────────

    #[test]
    fn stdlib_completions_offered_in_brink_dialect_logic_context() {
        let items = stdlib_completions(&CompletionContext::Logic, brink_analyzer::Dialect::Brink);
        assert_eq!(items.len(), 8, "all eight stdlib names: {items:?}");
    }

    #[test]
    fn stdlib_completions_offered_in_brink_dialect_inline_expr_context() {
        let items = stdlib_completions(
            &CompletionContext::InlineExpr,
            brink_analyzer::Dialect::Brink,
        );
        assert_eq!(items.len(), 8);
    }

    #[test]
    fn stdlib_completions_never_offered_in_strict_ink() {
        for ctx in [CompletionContext::Logic, CompletionContext::InlineExpr] {
            let items = stdlib_completions(&ctx, brink_analyzer::Dialect::StrictInk);
            assert!(items.is_empty(), "strict-ink must never see stdlib names");
        }
    }

    #[test]
    fn stdlib_completions_empty_outside_call_expression_contexts() {
        for ctx in [
            CompletionContext::Divert,
            CompletionContext::General,
            CompletionContext::FunctionArgs,
        ] {
            let items = stdlib_completions(&ctx, brink_analyzer::Dialect::Brink);
            assert!(
                items.is_empty(),
                "stdlib calls aren't offered outside Logic/InlineExpr: {ctx:?} -> {items:?}",
            );
        }
    }
}
