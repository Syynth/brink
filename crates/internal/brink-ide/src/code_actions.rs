use crate::formatting::{format_region, sort_knots_in_source, sort_stitches_in_knot};

/// The kind of code action.
pub enum CodeActionKind {
    QuickFix,
    Refactor,
    Source,
}

/// Data identifying which code action to perform on resolve.
pub enum CodeActionData {
    SortKnots,
    SortStitches { knot: String },
    FormatKnot { knot: String },
    FormatStitch { knot: String, stitch: String },
}

/// A code action offered to the user.
pub struct CodeAction {
    pub title: String,
    pub kind: CodeActionKind,
    pub data: CodeActionData,
}

/// Collect all applicable code actions for the given source and cursor byte offset.
pub fn code_actions(source: &str, cursor_byte_offset: usize) -> Vec<CodeAction> {
    use brink_syntax::ast::AstNode as _;

    let parse = brink_syntax::parse(source);
    let tree = parse.tree();

    let mut actions = Vec::new();

    // ── Sort knots ──────────────────────────────────────────────
    let knot_names: Vec<String> = tree.knots().filter_map(|k| k.header()?.name()).collect();

    if knot_names.len() >= 2 {
        let already_sorted = knot_names
            .windows(2)
            .all(|w| w[0].to_lowercase() <= w[1].to_lowercase());

        if !already_sorted {
            actions.push(CodeAction {
                title: "Sort knots alphabetically".to_owned(),
                kind: CodeActionKind::Source,
                data: CodeActionData::SortKnots,
            });
        }
    }

    // ── Cursor-scoped actions ───────────────────────────────────
    let cursor = rowan::TextSize::from(u32::try_from(cursor_byte_offset).unwrap_or(u32::MAX));

    let config = brink_fmt::FormatConfig::default();
    let formatted = brink_fmt::format(source, &config);

    let knots: Vec<_> = tree.knots().collect();
    for (ki, knot) in knots.iter().enumerate() {
        let knot_range = knot.syntax().text_range();
        if cursor < knot_range.start() || cursor > knot_range.end() {
            continue;
        }

        let knot_name = knot.header().and_then(|h| h.name()).unwrap_or_default();

        let knot_start: usize = knot_range.start().into();
        let knot_end: usize = if ki + 1 < knots.len() {
            knots[ki + 1].syntax().text_range().start().into()
        } else {
            source.len()
        };

        // Format knot
        if source.get(knot_start..knot_end) != formatted.get(knot_start..knot_end) {
            actions.push(CodeAction {
                title: format!("Format knot '{knot_name}'"),
                kind: CodeActionKind::Source,
                data: CodeActionData::FormatKnot {
                    knot: knot_name.clone(),
                },
            });
        }

        // Sort stitches
        let Some(body) = knot.body() else { break };
        let stitches: Vec<_> = body.stitches().collect();

        let stitch_names: Vec<String> =
            stitches.iter().filter_map(|s| s.header()?.name()).collect();

        if stitch_names.len() >= 2 {
            let already_sorted = stitch_names
                .windows(2)
                .all(|w| w[0].to_lowercase() <= w[1].to_lowercase());

            if !already_sorted {
                actions.push(CodeAction {
                    title: format!("Sort stitches in '{knot_name}' alphabetically"),
                    kind: CodeActionKind::Source,
                    data: CodeActionData::SortStitches {
                        knot: knot_name.clone(),
                    },
                });
            }
        }

        // Format stitch
        for (si, stitch) in stitches.iter().enumerate() {
            let stitch_range = stitch.syntax().text_range();
            if cursor < stitch_range.start() || cursor > stitch_range.end() {
                continue;
            }

            let stitch_name = stitch.header().and_then(|h| h.name()).unwrap_or_default();

            let stitch_start: usize = stitch_range.start().into();
            let stitch_end: usize = if si + 1 < stitches.len() {
                stitches[si + 1].syntax().text_range().start().into()
            } else {
                knot_end
            };

            if source.get(stitch_start..stitch_end) != formatted.get(stitch_start..stitch_end) {
                actions.push(CodeAction {
                    title: format!("Format stitch '{stitch_name}'"),
                    kind: CodeActionKind::Source,
                    data: CodeActionData::FormatStitch {
                        knot: knot_name,
                        stitch: stitch_name,
                    },
                });
            }
            break;
        }

        break;
    }

    actions
}

/// Resolve a code action by applying its transformation and returning the new source.
pub fn resolve_code_action(source: &str, data: &CodeActionData) -> Option<String> {
    let new_source = match data {
        CodeActionData::SortKnots => sort_knots_in_source(source),
        CodeActionData::SortStitches { knot } => sort_stitches_in_knot(source, knot),
        CodeActionData::FormatKnot { knot } => format_region(source, knot, None),
        CodeActionData::FormatStitch { knot, stitch } => format_region(source, knot, Some(stitch)),
    };

    if new_source == source {
        None
    } else {
        Some(new_source)
    }
}
