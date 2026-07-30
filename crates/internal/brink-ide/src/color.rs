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
    let Some(ids) = analysis.index.by_name.get(callee_name) else {
        return;
    };
    let Some(info) = ids
        .iter()
        .filter_map(|id| analysis.index.symbols.get(id))
        .find(|info| {
            matches!(
                info.kind,
                brink_ir::SymbolKind::Knot
                    | brink_ir::SymbolKind::Stitch
                    | brink_ir::SymbolKind::External
            ) && info.params.len() == args.len()
        })
    else {
        return;
    };
    let Some(meta) = analysis.symbol_meta.get(&info.id) else {
        return;
    };

    for (i, arg) in args.iter().enumerate() {
        let is_color = meta
            .params
            .get(i)
            .and_then(|rp| rp.ty.as_ref())
            .and_then(|rt| rt.widget.as_ref())
            .is_some_and(|w| w.kind == COLOR_WIDGET_KIND);
        if !is_color {
            continue;
        }
        // Only a quoted string literal carries a color; skip a variable/expr.
        let text = arg.syntax().text().to_string();
        let trimmed = text.trim();
        if trimmed.len() < 2 || !trimmed.starts_with('"') || !trimmed.ends_with('"') {
            continue;
        }
        hints.push(ColorHint {
            start: arg.syntax().text_range().start(),
            end: arg.syntax().text_range().end(),
            value: trimmed.trim_matches('"').to_string(),
        });
    }
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
}
