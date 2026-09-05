//! Interactive queries — hover, completions, document symbols.
//!
//! These are request/response, which is what makes moving the session to a
//! worker cheap rather than invasive: `gpui-base`'s provider traits already
//! return a `Task`, so nothing on the UI side has to change shape. This is
//! how LSP works, and how Zed reaches its own analysis.
//!
//! A query is answered **after** the edits queued ahead of it, in the same
//! drain. The channel is FIFO and the editor sends its `Edit` before asking
//! (`document.rs`, `seed_edit`), so a query never sees text older than the
//! keystroke that prompted it. [`clamp_offset`] is the guard behind that
//! promise: an offset past the text is pulled back to its end rather than
//! allowed to panic the analysis thread.
//!
//! Results are plain data in **byte offsets**, like everything else crossing
//! the boundary. The mapping onto `lsp_types` lives with the editor that
//! consumes it, so it exists once.

use brink_ir::SymbolKind;

/// What the UI wants to know.
#[derive(Debug, Clone)]
pub enum QueryKind {
    Hover { path: String, offset: u32 },
    Completions { path: String, offset: u32 },
    DocumentSymbols { path: String },
    InlayHints { path: String },
}

/// The answer. `Unavailable` is the honest result for a path the session
/// does not hold or a project that has not analyzed yet — distinct from an
/// empty answer, which means "asked, and there is nothing here".
#[derive(Debug, Clone)]
pub enum QueryResult {
    Hover(Option<HoverInfo>),
    Completions(Vec<Completion>),
    DocumentSymbols(Vec<Symbol>),
    InlayHints(Vec<InlayHint>),
    Unavailable,
}

/// A parameter-name hint, drawn inside the line although the buffer does
/// not contain it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InlayHint {
    pub offset: u32,
    /// Already carries its own `:`; `padding_right` is folded in here so the
    /// editor does not have to know the convention.
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HoverInfo {
    /// Markdown, with link refs already stripped.
    pub markdown: String,
    pub range: Option<(u32, u32)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Completion {
    pub label: String,
    pub kind: CompletionKind,
}

/// Kept as brink's own kind rather than an LSP one so the LSP mapping is
/// written once, next to the editor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompletionKind {
    Symbol(SymbolKind),
    StdlibFunction,
    /// `DONE` / `END`.
    Builtin,
}

/// One knot or stitch, for the Binder's structure view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: String,
    /// The name's own range — where "play from here" jumps to.
    pub start: u32,
    /// The whole declaration, header through body.
    pub full_start: u32,
    pub full_end: u32,
    pub is_function: bool,
    pub children: Vec<Symbol>,
}

pub(crate) fn answer(session: &brink_ide::session::IdeSession, kind: &QueryKind) -> QueryResult {
    match kind {
        QueryKind::Hover { path, offset } => QueryResult::Hover(hover(session, path, *offset)),
        QueryKind::Completions { path, offset } => match completions(session, path, *offset) {
            Some(items) => QueryResult::Completions(items),
            None => QueryResult::Unavailable,
        },
        QueryKind::DocumentSymbols { path } => match symbols(session, path) {
            Some(found) => QueryResult::DocumentSymbols(found),
            None => QueryResult::Unavailable,
        },
        QueryKind::InlayHints { path } => match inlay_hints(session, path) {
            Some(found) => QueryResult::InlayHints(found),
            None => QueryResult::Unavailable,
        },
    }
}

fn inlay_hints(session: &brink_ide::session::IdeSession, path: &str) -> Option<Vec<InlayHint>> {
    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let whole = rowan::TextRange::new(
        rowan::TextSize::from(0),
        rowan::TextSize::from(u32::try_from(source.len()).unwrap_or(u32::MAX)),
    );
    // The native and ink frontends are distinct nominal trees, so the
    // dispatch is on the file's own language — feeding an ink-parsed root to
    // the native query would silently reproduce #2280.
    let hints = if session.is_native(id) {
        let root = session.syntax_root_native(id)?;
        brink_ide::inlay_hints::inlay_hints_native(&root, analysis, session.db(), id, whole, None)
    } else {
        let root = session.syntax_root(id)?;
        brink_ide::inlay_hints::inlay_hints(&root, analysis, session.db(), id, whole, None)
    };
    Some(
        hints
            .into_iter()
            .map(|h| InlayHint {
                offset: u32::from(h.offset),
                label: if h.padding_right {
                    format!("{} ", h.label)
                } else {
                    h.label
                },
            })
            .collect(),
    )
}

/// `offset` pulled inside `source`: at most its length, and never inside a
/// multi-byte character.
pub(crate) fn clamp_offset(source: &str, offset: u32) -> u32 {
    let mut at = usize::try_from(offset)
        .unwrap_or(usize::MAX)
        .min(source.len());
    while at > 0 && !source.is_char_boundary(at) {
        at -= 1;
    }
    u32::try_from(at).unwrap_or(u32::MAX)
}

fn hover(session: &brink_ide::session::IdeSession, path: &str, offset: u32) -> Option<HoverInfo> {
    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let offset = clamp_offset(source, offset);
    let info = brink_ide::hover::hover(
        analysis,
        session.db(),
        id,
        source,
        offset.into(),
        &session.db().file_metadata(),
    )?;
    Some(HoverInfo {
        markdown: brink_ide::hover::strip_link_refs(&info.content),
        range: info.range.map(|r| (r.start().into(), r.end().into())),
    })
}

fn completions(
    session: &brink_ide::session::IdeSession,
    path: &str,
    offset: u32,
) -> Option<Vec<Completion>> {
    use brink_ide::{
        CompletionContext, cursor_scope, detect_completion_context, is_visible_in_context,
        ref_arg_root_prefix, stdlib_completions,
    };

    let id = session.file_id(path)?;
    let analysis = session.analysis()?;
    let source = session.source(id)?;
    let offset = clamp_offset(source, offset) as usize;

    let ctx = detect_completion_context(source, offset);
    let scope = cursor_scope(source, offset);
    let ref_root = ref_arg_root_prefix(source, offset);
    let mut items = Vec::new();

    // A dotted path is exhaustive: only that knot's members can complete,
    // so this returns rather than falling through to the general sweep.
    if let CompletionContext::DottedPath { ref knot } = ctx {
        let prefix = format!("{knot}.");
        for (name, ids) in &analysis.index.by_name {
            let Some(suffix) = name.strip_prefix(&*prefix) else {
                continue;
            };
            for def_id in ids {
                if let Some(info) = analysis.index.symbols.get(def_id) {
                    items.push(Completion {
                        label: suffix.to_owned(),
                        kind: CompletionKind::Symbol(info.kind),
                    });
                }
            }
        }
        return Some(items);
    }

    for info in analysis.index.symbols.values() {
        if !is_visible_in_context(&ctx, info, &scope) {
            continue;
        }
        // A `ref` argument can only take a variable, so nothing else is a
        // legal completion there however visible it is.
        if ref_root.is_some() && info.kind != SymbolKind::Variable {
            continue;
        }
        items.push(Completion {
            label: info.name.clone(),
            kind: CompletionKind::Symbol(info.kind),
        });
    }
    for f in stdlib_completions(&ctx, session.language_dialect()) {
        items.push(Completion {
            label: f.name.to_owned(),
            kind: CompletionKind::StdlibFunction,
        });
    }
    if matches!(
        ctx,
        CompletionContext::Divert | CompletionContext::InlineExpr
    ) {
        for label in ["DONE", "END"] {
            items.push(Completion {
                label: label.to_owned(),
                kind: CompletionKind::Builtin,
            });
        }
    }
    Some(items)
}

fn symbols(session: &brink_ide::session::IdeSession, path: &str) -> Option<Vec<Symbol>> {
    let id = session.file_id(path)?;
    let hir = session.hir(id)?;
    let manifest = session.manifest(id)?;
    let source = session.source(id)?;
    Some(
        brink_ide::document::document_symbols(hir, manifest, source)
            .iter()
            .map(convert)
            .collect(),
    )
}

fn convert(symbol: &brink_ide::document::DocumentSymbol) -> Symbol {
    Symbol {
        name: symbol.name.clone(),
        start: symbol.range.start().into(),
        full_start: symbol.full_range.start().into(),
        full_end: symbol.full_range.end().into(),
        is_function: symbol
            .detail
            .as_deref()
            .is_some_and(|d| d.contains("function")),
        children: symbol.children.iter().map(convert).collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::clamp_offset;

    #[test]
    fn an_offset_past_the_text_lands_on_its_end_at_a_char_boundary() {
        assert_eq!(clamp_offset("hello", 3), 3);
        assert_eq!(clamp_offset("hello", 5), 5);
        assert_eq!(clamp_offset("hello", 199), 5);
        // "é" is two bytes; an offset inside it steps back to its start.
        assert_eq!(clamp_offset("caf\u{e9}", 4), 3);
        assert_eq!(clamp_offset("", 7), 0);
    }
}
