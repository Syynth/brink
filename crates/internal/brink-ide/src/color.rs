//! Color hints — a studio-builtin picker for `color`-widget arguments.
//!
//! When an `EXTERNAL` call argument's semantic type declares the built-in
//! `color` widget (manifest `widget: { kind: "color" }`) and the literal is a
//! quoted hex string, we surface its span + value so the editor can render a
//! color swatch/picker over it. Reuses the same call-site → semantic-type join
//! point as inlay hints and the argument picker; tooling-only, never touches the
//! compiled program.

use brink_analyzer::AnalysisResult;
use brink_syntax::SyntaxNode;
use brink_syntax::ast::AstNode;
use rowan::{TextRange, TextSize};

/// The built-in widget kind that triggers the color picker.
pub const COLOR_WIDGET_KIND: &str = "color";

/// A `hex_color` argument literal: its full span (including quotes) and the
/// bare hex value (quotes stripped, e.g. `#FF0000`).
pub struct ColorHint {
    pub start: TextSize,
    pub end: TextSize,
    pub value: String,
}

/// Color hints for every `hex_color` argument literal within `range`.
#[must_use]
pub fn color_hints(
    root: &SyntaxNode,
    analysis: &AnalysisResult,
    range: TextRange,
) -> Vec<ColorHint> {
    let mut hints = Vec::new();
    for node in root.descendants() {
        let node_range = node.text_range();
        if node_range.end() < range.start() || node_range.start() > range.end() {
            continue;
        }
        if let Some(call) = brink_syntax::ast::FunctionCall::cast(node.clone()) {
            if let Some(name) = call.name() {
                collect(&name, call.arg_list(), analysis, &mut hints);
            }
        } else if let Some(target) = brink_syntax::ast::DivertTargetWithArgs::cast(node.clone())
            && let Some(path_node) = target.path()
        {
            collect(
                &path_node.full_name(),
                target.arg_list(),
                analysis,
                &mut hints,
            );
        }
    }
    hints
}

fn collect(
    callee_name: &str,
    arg_list: Option<brink_syntax::ast::ArgList>,
    analysis: &AnalysisResult,
    hints: &mut Vec<ColorHint>,
) {
    let Some(arg_list) = arg_list else { return };
    let args: Vec<_> = arg_list.args().collect();
    if args.is_empty() {
        return;
    }
    let Some(meta) = resolve_callee(callee_name, args.len(), analysis) else {
        return;
    };

    for (i, arg) in args.iter().enumerate() {
        if !slot_is_color(meta, i) {
            continue;
        }
        // Only a quoted string literal carries a color; skip a variable/expr.
        let text = arg.syntax().text().to_string();
        let Some(value) = quoted_hex_value(&text) else {
            continue;
        };
        hints.push(ColorHint {
            start: arg.syntax().text_range().start(),
            end: arg.syntax().text_range().end(),
            value,
        });
    }
}

/// The native (`.brink`) sibling of [`color_hints`] (issue #2359) — same
/// call-site -> semantic-type join, over `brink_syntax_native::SyntaxKind`
/// call/divert-target nodes (`CallExpr`/`DivertTarget`) instead of ink's
/// typed AST, which has no native equivalent to cast into (#2291's
/// `syntax_root` failure mode: ink-parsing native source text produces a
/// garbled tree whose `FunctionCall`/`DivertTargetWithArgs` casts don't mean
/// what this pass assumes). Never pass a `brink_syntax::SyntaxNode` root
/// parsed from native source text here — use
/// [`crate::session::IdeSession::syntax_root_native`].
#[must_use]
pub fn color_hints_native(
    root: &brink_syntax_native::SyntaxNode,
    analysis: &AnalysisResult,
    range: TextRange,
) -> Vec<ColorHint> {
    use brink_syntax_native::ast::AstNode as _;
    let mut hints = Vec::new();
    for node in root.descendants() {
        let node_range = node.text_range();
        if node_range.end() < range.start() || node_range.start() > range.end() {
            continue;
        }
        if let Some(call) = brink_syntax_native::ast::CallExpr::cast(node.clone()) {
            if let Some(callee) = call.callee() {
                collect_native(
                    &native_path_name(&callee),
                    call.arg_list(),
                    analysis,
                    &mut hints,
                );
            }
        } else if let Some(target) = brink_syntax_native::ast::DivertTarget::cast(node.clone())
            && let Some(path_node) = target.path()
        {
            collect_native(
                &native_path_name(&path_node),
                target.call_args(),
                analysis,
                &mut hints,
            );
        }
    }
    hints
}

/// A `Path`'s full dotted name (e.g. `"knot.stitch"`) — native has no
/// `full_name()` accessor on its `ast::Path` (unlike `brink-syntax`'s), so
/// this joins `segments()` the same way ink's own `full_name()` does.
/// `pub(crate)` — shared by [`crate::inlay_hints`]'s and
/// [`crate::argument_widgets`]'s native passes (issue #2359).
pub(crate) fn native_path_name(path: &brink_syntax_native::ast::Path) -> String {
    path.segments()
        .map(|t| t.text().to_string())
        .collect::<Vec<_>>()
        .join(".")
}

fn collect_native(
    callee_name: &str,
    arg_list: Option<brink_syntax_native::ast::ArgList>,
    analysis: &AnalysisResult,
    hints: &mut Vec<ColorHint>,
) {
    use brink_syntax_native::ast::AstNode as _;
    let Some(arg_list) = arg_list else { return };
    let args: Vec<_> = arg_list.syntax().children().collect();
    if args.is_empty() {
        return;
    }
    let Some(meta) = resolve_callee(callee_name, args.len(), analysis) else {
        return;
    };

    for (i, arg) in args.iter().enumerate() {
        if !slot_is_color(meta, i) {
            continue;
        }
        let text = arg.text().to_string();
        let Some(value) = quoted_hex_value(&text) else {
            continue;
        };
        hints.push(ColorHint {
            start: arg.text_range().start(),
            end: arg.text_range().end(),
            value,
        });
    }
}

/// Resolve `callee_name` to a callable symbol with exactly `arity` params,
/// then its `SymbolMeta` (the manifest-derived type info) — the join point
/// [`collect`]/[`collect_native`] share, independent of which CST produced
/// the callee name.
fn resolve_callee<'a>(
    callee_name: &str,
    arity: usize,
    analysis: &'a AnalysisResult,
) -> Option<&'a brink_analyzer::SymbolMeta> {
    let ids = analysis.index.by_name.get(callee_name)?;
    let info = ids
        .iter()
        .filter_map(|id| analysis.index.symbols.get(id))
        .find(|info| {
            matches!(
                info.kind,
                brink_ir::SymbolKind::Knot
                    | brink_ir::SymbolKind::Stitch
                    | brink_ir::SymbolKind::External
            ) && info.params.len() == arity
        })?;
    analysis.symbol_meta.get(&info.id)
}

fn slot_is_color(meta: &brink_analyzer::SymbolMeta, i: usize) -> bool {
    meta.params
        .get(i)
        .and_then(|rp| rp.ty.as_ref())
        .and_then(|rt| rt.widget.as_ref())
        .is_some_and(|w| w.kind == COLOR_WIDGET_KIND)
}

/// A quoted string literal's bare hex value (quotes stripped), or `None` for
/// anything else (a variable/expr, or a non-string literal) — shared by
/// [`collect`]/[`collect_native`], which pass in each arg's own source text
/// regardless of which CST it came from.
fn quoted_hex_value(arg_text: &str) -> Option<String> {
    let trimmed = arg_text.trim();
    if trimmed.len() < 2 || !trimmed.starts_with('"') || !trimmed.ends_with('"') {
        return None;
    }
    Some(trimmed.trim_matches('"').to_string())
}

#[cfg(test)]
mod tests {
    use super::ColorHint;
    use crate::session::IdeSession;

    fn hints(src: &str) -> Vec<ColorHint> {
        use brink_ir::{
            BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef,
            TypeRef, WidgetDecl,
        };
        let mut session = IdeSession::new();
        session.update_and_analyze("test.ink", src.to_string());
        session.set_host_manifest(HostManifest {
            markup: Vec::new(),
            externals: vec![ManifestExternal {
                name: "set_tint".into(),
                params: vec![ManifestParam {
                    name: "color".into(),
                    ty: TypeRef("hex_color".into()),
                }],
                returns: TypeRef::default(),
                kind: ExternalKind::Effect,
                doc: None,

                widgets: vec![],
                path: Vec::new(),
            }],
            types: vec![SemanticTypeDef {
                name: "hex_color".into(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: Some(WidgetDecl {
                    kind: "color".into(),
                }),
            }],
        });
        let analysis = session.analysis().expect("analysis");
        let parsed = brink_syntax::parse(src);
        super::color_hints(
            &parsed.syntax(),
            analysis,
            rowan::TextRange::new(0.into(), rowan::TextSize::of(src)),
        )
    }

    #[test]
    fn hex_color_string_literal_yields_a_hint() {
        let h = hints("EXTERNAL set_tint(color)\n~ set_tint(\"#FF8800\")\n-> END\n");
        assert_eq!(h.len(), 1);
        assert_eq!(h[0].value, "#FF8800");
    }

    #[test]
    fn non_hex_color_param_and_non_literal_are_skipped() {
        // A bare (non-string) arg gets no hint; a different external too.
        let h = hints("EXTERNAL set_tint(color)\n~ temp c = 1\n~ set_tint(c)\n-> END\n");
        assert!(h.is_empty());
    }

    // ── #2359: `color_hints_native` — the native (`.brink`) sibling ────────

    fn native_hints(src: &str) -> Vec<ColorHint> {
        use brink_ir::{
            BaseType, ExternalKind, HostManifest, ManifestExternal, ManifestParam, SemanticTypeDef,
            TypeRef, WidgetDecl,
        };
        let mut session = IdeSession::new();
        session.update_and_analyze("test.brink", src.to_string());
        session.set_host_manifest(HostManifest {
            markup: Vec::new(),
            externals: vec![ManifestExternal {
                name: "set_tint".into(),
                params: vec![ManifestParam {
                    name: "color".into(),
                    ty: TypeRef("hex_color".into()),
                }],
                returns: TypeRef::default(),
                kind: ExternalKind::Effect,
                doc: None,

                widgets: vec![],
                path: Vec::new(),
            }],
            types: vec![SemanticTypeDef {
                name: "hex_color".into(),
                base: BaseType::String,
                constraint: None,
                values: None,
                widget: Some(WidgetDecl {
                    kind: "color".into(),
                }),
            }],
        });
        let analysis = session.analysis().expect("analysis");
        let parsed = brink_syntax_native::parse(src);
        super::color_hints_native(
            &parsed.syntax(),
            analysis,
            rowan::TextRange::new(0.into(), rowan::TextSize::of(src)),
        )
    }

    #[test]
    fn native_hex_color_string_literal_yields_a_hint() {
        // A regression test that fails with `color_hints_native` deleted
        // and `color_hints_impl` in `crates/brink-web` back on the pre-#2359
        // `is_native` early-return: this exact call site would silently get
        // zero hints instead of one.
        let h =
            native_hints("extern set_tint(color)\nflow main() {\n  ~ set_tint(\"#FF8800\")\n}\n");
        assert_eq!(
            h.len(),
            1,
            "{:?}",
            h.iter().map(|h| &h.value).collect::<Vec<_>>()
        );
        assert_eq!(h[0].value, "#FF8800");
    }

    #[test]
    fn native_non_hex_color_param_and_non_literal_are_skipped() {
        let h = native_hints(
            "extern set_tint(color)\nflow main() {\n  ~ let c = 1\n  ~ set_tint(c)\n}\n",
        );
        assert!(
            h.is_empty(),
            "{:?}",
            h.iter().map(|h| &h.value).collect::<Vec<_>>()
        );
    }
}
