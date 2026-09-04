#!/usr/bin/env python3
"""Build `vendor/` — our fork of the GPUI Kit crates, on Zed's own `gpui`.

WHY A FORK EXISTS

`gpui-component`/`gpui-base` hard-wire `gpui = { version = "0.3.1", package =
"gpui-pre" }`. A `[patch.crates-io]` cannot redirect that: the patch resolves,
and Cargo then discards it because Zed's `gpui` is version **0.2.2**, which
does not satisfy `^0.3.1`. `gpui-pre` renumbered to 0.3.x. So substitution
requires vendoring and editing manifests.

It is a small fork. `gpui-pre` 0.3.3 is a snapshot of `zed@5b055fa`
(2026-09-03) and 89 of its 90 source files are byte-identical to Zed's; the
lone difference is `action.rs`, where the `actions!` macro uses
`$crate::Action` instead of `gpui::Action` so the crate works under a new
name. Every edit below is about NAMES — except the last two, which add the
editor capabilities the studio needs (`Editor: Sizable`, and the `IntoPlot`
lookup that would otherwise refuse to see Zed's own crate).

WHY PYTHON RATHER THAN SHELL

`scripts/check-scripts.mjs` scans every shell script in the repo for
unbounded network calls, and its tokenizer reads heredoc bodies as shell — a
blind spot its own header names. An embedded Python heredoc trips it on every
line. Being a Python script sidesteps that honestly, rather than waiving it.

`vendor/` is gitignored; run this once after checkout, then `cargo build`.
The committed `Cargo.toml` already points at `vendor/`.
"""

from __future__ import annotations

import os
import pathlib
import re
import shutil
import sys

REV = "5b055fa789a8b8d38ac951a6e0cde272f66b4495"  # the commit gpui-pre 0.3.3 snapshots
GIT = "https://github.com/zed-industries/zed"

HERE = pathlib.Path(__file__).resolve().parent
VENDOR = HERE / "vendor"
CRATES = ["gpui-base", "gpui-component", "gpui-kit-assets", "gpui-component-macros"]

# Every `gpui-pre*` package is a rename of a crate in Zed's repo.
MAP = {
    "gpui-pre": "gpui",
    "gpui-pre-macros": "gpui_macros",
    "gpui-pre-sum-tree": "sum_tree",
    "gpui-pre-platform": "gpui_platform",
    "gpui-pre-reqwest-client": "reqwest_client",
}


_SWATCH_LAYOUT = """    /// VENDOR EDIT — geometry for the swatch drawn inside each inlay chip.
    ///
    /// `LineLayout::inlay_x_bounds` gives the chip's own x span, so the square
    /// is placed relative to the CHIP rather than to any buffer text — which
    /// is what makes it drawing-in-a-widget rather than decoration-on-text.
    fn layout_inlay_swatches(
        &self,
        last_layout: &LastLayout,
        bounds: &Bounds<Pixels>,
        cx: &mut App,
    ) -> Vec<(Bounds<Pixels>, Hsla)> {
        let state = self.state.read(cx);
        let inlays = state.extras.inlays();
        if inlays.is_empty() {
            return Vec::new();
        }
        let line_height = last_layout.line_height;
        let mut out = Vec::new();
        let mut y = bounds.origin.y + last_layout.visible_top;
        for (vi, line_layout) in last_layout.lines.iter().enumerate() {
            let Some(&line_start) = last_layout.visible_line_byte_offsets.get(vi) else {
                break;
            };
            for inlay in inlays {
                let Some(swatch) = inlay.swatch else { continue };
                if inlay.offset < line_start {
                    continue;
                }
                let Some((x0, _x1)) = line_layout.inlay_x_bounds(inlay.offset - line_start) else {
                    continue;
                };
                let size = line_height * 0.5;
                let origin = point(
                    bounds.origin.x + last_layout.line_number_width + x0 + px(2.),
                    y + (line_height - size) / 2.,
                );
                out.push((Bounds::new(origin, gpui::size(size, size)), swatch));
            }
            y += line_layout.size(line_height).height;
        }
        out
    }

"""

_SPLICE = """/// VENDOR EDIT — splice inlays anchored inside this sub-line into its text
/// and runs.
///
/// Returns the display text, the runs covering it, and the inlays as
/// `(buffer offset within the sub-line, inserted byte length)` for
/// `LineLayout`'s coordinate translation.
fn splice_inlays(
    inlays: &[crate::input::Inlay],
    line_start: usize,
    text: SharedString,
    runs: Vec<TextRun>,
    default_run: &TextRun,
) -> (SharedString, Vec<TextRun>, Vec<(usize, usize)>) {
    let line_len = text.len();
    let hits: Vec<&crate::input::Inlay> = inlays
        .iter()
        .filter(|i| i.offset >= line_start && i.offset <= line_start + line_len)
        .collect();
    if hits.is_empty() {
        return (text, runs, Vec::new());
    }

    let mut out = String::with_capacity(text.len() + 16);
    let mut out_runs: Vec<TextRun> = Vec::with_capacity(runs.len() + hits.len());
    let mut table = Vec::with_capacity(hits.len());
    let mut cursor = 0usize;
    // Runs are consumed in step with the buffer text they cover.
    let mut pending = runs.into_iter().collect::<std::collections::VecDeque<_>>();
    let mut take_runs = |upto: usize, cursor: usize, out_runs: &mut Vec<TextRun>| {
        let mut want = upto - cursor;
        while want > 0 {
            let Some(mut run) = pending.pop_front() else {
                break;
            };
            if run.len <= want {
                want -= run.len;
                out_runs.push(run);
            } else {
                let mut head = run.clone();
                head.len = want;
                run.len -= want;
                pending.push_front(run);
                out_runs.push(head);
                want = 0;
            }
        }
    };

    for inlay in hits {
        let at = inlay.offset - line_start;
        if at > cursor {
            out.push_str(&text[cursor..at]);
            take_runs(at, cursor, &mut out_runs);
            cursor = at;
        }
        table.push((at, inlay.text.len()));
        out.push_str(&inlay.text);
        let mut run = default_run.clone();
        run.len = inlay.text.len();
        if let Some(color) = inlay.style.color {
            run.color = color;
        }
        run.background_color = inlay.style.background_color;
        out_runs.push(run);
    }
    if cursor < line_len {
        out.push_str(&text[cursor..]);
        take_runs(line_len, cursor, &mut out_runs);
    }
    out_runs.extend(pending);

    (out.into(), out_runs, table)
}

"""


def registry_source() -> pathlib.Path:
    """The registry cache the crates were unpacked into."""
    home = pathlib.Path(os.environ.get("CARGO_HOME", pathlib.Path.home() / ".cargo"))
    for base in (home / "registry" / "src").glob("*"):
        if (base / "gpui-base-0.6.0").is_dir():
            return base
    sys.exit("gpui-base-0.6.0 is not in the registry cache; run 'cargo fetch' first.")


def rewrite_manifest(path: pathlib.Path, local_paths: dict[str, str]) -> int:
    """Point `gpui-pre*` dependency tables at Zed's git, and sibling vendored
    crates at their local paths."""
    blocks: list[list[str]] = []
    current: list[str] = []
    for line in path.read_text().splitlines():
        if line.startswith("["):
            if current:
                blocks.append(current)
            current = [line]
        else:
            current.append(line)
    if current:
        blocks.append(current)

    out: list[str] = []
    changed = 0
    for block in blocks:
        header = block[0] if block and block[0].startswith("[") else ""
        is_dep = ".dependencies." in header or header.startswith(
            ("[dependencies.", "[dev-dependencies.", "[build-dependencies.")
        )
        pkg = next(
            (
                m.group(1)
                for m in (re.match(r'\s*package = "([^"]+)"', l) for l in block)
                if m
            ),
            None,
        )
        if is_dep and pkg in MAP:
            body = [l for l in block[1:] if not re.match(r"\s*(version|package) = ", l)]
            block = [
                header,
                f'git = "{GIT}"',
                f'rev = "{REV}"',
                f'package = "{MAP[pkg]}"',
            ] + body
            changed += 1
        elif is_dep:
            name = header.split(".")[-1].rstrip("]")
            if name in local_paths:
                body = [l for l in block[1:] if not re.match(r"\s*version = ", l)]
                block = [header, f'path = "{local_paths[name]}"'] + body
                changed += 1
        out.extend(block)
    path.write_text("\n".join(out) + "\n")
    return changed


def patch_intoplot() -> None:
    """`IntoPlot` resolves the GPUI path by looking up a dependency whose
    PACKAGE is literally `gpui-kit` or `gpui-pre`, which makes the macro
    unusable against the crate those two are snapshots of. Renaming the
    dependency does not help: `proc-macro-crate` matches the real package
    name, not the alias."""
    path = VENDOR / "gpui-component-macros/src/crate_path.rs"
    s = path.read_text()
    s = s.replace(
        '        Err(kit_error) => crate_name("gpui-pre")',
        "        // VENDOR EDIT: accept Zed's own `gpui` package too.\n"
        '        Err(kit_error) => crate_name("gpui-pre")\n'
        '            .or_else(|_| crate_name("gpui"))',
    )
    path.write_text(s)


def patch_editor_sizable() -> None:
    """`Editor` has no size of its own, so `Input` renders it at
    `Size::default()` (Medium) and pads it 8px top and bottom. In the stacked
    Continuous view that reads as half a line of dead space above every file,
    and it makes a section impossible to size exactly — which matters,
    because a section that can scroll at all swallows the wheel instead of
    scrolling the manuscript."""
    path = VENDOR / "gpui-component/src/input/editor.rs"
    s = path.read_text()
    s = s.replace(
        "use crate::{ActiveTheme as _, RoleOverride, StyledExt as _};",
        "use crate::{ActiveTheme as _, RoleOverride, Sizable as _, StyledExt as _};",
    )
    s = s.replace(
        "pub struct Editor {\n    state: Entity<EditorState>,\n    style: StyleRefinement,",
        "pub struct Editor {\n    state: Entity<EditorState>,\n    style: StyleRefinement,\n"
        "    /// VENDOR EDIT: drives `Input`'s vertical padding (`Size::input_py()`).\n"
        "    size: crate::Size,",
    )
    s = s.replace(
        "            state: state.clone(),\n            style: StyleRefinement::default(),",
        "            state: state.clone(),\n            style: StyleRefinement::default(),\n"
        "            size: crate::Size::default(),",
    )
    s = s.replace(
        "        Input::from_state(self.state.clone())",
        "        Input::from_state(self.state.clone())\n            .with_size(self.size)",
    )
    s = s.replace(
        "impl Styled for Editor {",
        "impl crate::Sizable for Editor {\n"
        "    fn with_size(mut self, size: impl Into<crate::Size>) -> Self {\n"
        "        self.size = size.into();\n"
        "        self\n"
        "    }\n"
        "}\n\n"
        "impl Styled for Editor {",
    )
    path.write_text(s)


def patch_inlays() -> None:
    """In-text inlays: text drawn inside a line that the buffer does not
    contain — CodeMirror's `WidgetType`, which `gpui-base` has no equivalent
    for (`EDITOR-SWEEP.md` round 5).

    Two halves. RENDERING splices the inlay's text and its own `TextRun` into
    the shaped line at layout time; a `TextRun` already carries colour,
    background, font and underline, so a chip needs no element. COORDINATE
    TRANSLATION then reconciles the two offset spaces the splice creates:
    everything outside `LineLayout` keeps working in buffer offsets, and the
    three x-mapping call sites translate. Missing one is exactly as visible
    as it sounds — a click after an inlay lands the caret N characters late,
    N being the inlay's length.
    """
    # ── the type, the state, and the trait hook ──────────────────────
    kind = VENDOR / "gpui-base/src/input/base/kind.rs"
    s = kind.read_text()
    s = s.replace(
        "pub struct EditorExtras {\n    pub(crate) lsp: Lsp,",
        """/// VENDOR EDIT — an in-text inlay: text the editor draws inside a line
/// that the buffer does not contain. The unit CodeMirror calls a widget.
#[derive(Debug, Clone)]
pub struct Inlay {
    /// Where in the buffer the inlay is anchored.
    pub offset: usize,
    pub text: gpui::SharedString,
    pub style: gpui::HighlightStyle,
    /// A colour swatch drawn INSIDE the chip, left of its text — a filled
    /// quad painted at the inlay's own pixel bounds, not a glyph.
    pub swatch: Option<gpui::Hsla>,
}

/// Called when a click lands inside an inlay, with the buffer offset the
/// inlay is anchored at.
pub type InlayClickHandler = std::rc::Rc<dyn Fn(usize, &mut gpui::Window, &mut gpui::App)>;

pub struct EditorExtras {
    /// VENDOR EDIT — see [`Inlay`]. Sorted by offset.
    pub(crate) inlays: Vec<Inlay>,
    /// VENDOR EDIT — see [`InlayClickHandler`].
    pub(crate) on_inlay_click: Option<InlayClickHandler>,
    pub(crate) lsp: Lsp,""",
    )
    s = s.replace(
        "        Self {\n            lsp: Lsp::default(),",
        "        Self {\n            inlays: Vec::new(),\n            on_inlay_click: None,\n            lsp: Lsp::default(),",
    )
    s = s.replace(
        "pub trait InputExtras: Default + 'static {\n    /// Decoration ranges to paint, innermost collection first.",
        """pub trait InputExtras: Default + 'static {
    /// VENDOR EDIT — in-text inlays. Only the code-editor kind has any.
    fn inlays(&self) -> &[Inlay] {
        &[]
    }

    /// Decoration ranges to paint, innermost collection first.""",
    )
    kind.write_text(s)

    # ── export it ────────────────────────────────────────────────────
    imod = VENDOR / "gpui-base/src/input/mod.rs"
    s = imod.read_text().replace("EditorExtras", "EditorExtras, Inlay", 1)
    imod.write_text(s)

    # gpui-component republishes gpui-base's input types; the new one has to
    # travel with them or a consumer cannot name it.
    cmod = VENDOR / "gpui-component/src/input/mod.rs"
    s = cmod.read_text().replace("HoverProvider, Indent,", "HoverProvider, Indent, Inlay,", 1)
    cmod.write_text(s)

    # ── setters, the click hook, and the extras impl ─────────────────
    emod = VENDOR / "gpui-base/src/input/editor/mod.rs"
    s = emod.read_text()
    s = s.replace(
        "use gpui::{App, Div, Entity, InteractiveElement as _, IntoElement, RenderOnce, Stateful, Window};",
        "use gpui::{\n    App, Context, Div, Entity, InteractiveElement as _, IntoElement, RenderOnce, Stateful,\n    Window,\n};",
    )
    s = s.replace(
        "impl EditorState {",
        """impl EditorState {
    /// VENDOR EDIT — set the in-text inlays this editor draws.
    ///
    /// Sorted here so the layout and the offset mapping can both assume
    /// order. Text inside an inlay is not addressable: a click that lands in
    /// one resolves to the buffer offset it is anchored at.
    pub fn set_inlays(&mut self, mut inlays: Vec<crate::input::Inlay>, cx: &mut Context<Self>) {
        inlays.sort_by_key(|i| i.offset);
        self.extras.inlays = inlays;
        cx.notify();
    }

    /// VENDOR EDIT — called when a click lands inside an inlay.
    pub fn on_inlay_click(
        &mut self,
        handler: impl Fn(usize, &mut Window, &mut gpui::App) + 'static,
    ) {
        self.extras.on_inlay_click = Some(std::rc::Rc::new(handler));
    }
""",
        1,
    )
    s = s.replace(
        "    ) -> bool {\n        state.handle_click_hover_definition(event, offset, window, cx)\n    }",
        """    ) -> bool {
        // VENDOR EDIT — a click inside an inlay belongs to the inlay, not to
        // the text under it. Consumes the click when a handler takes it.
        if let Some(anchor) = state.inlay_at_mouse_position(event.position) {
            if let Some(handler) = state.extras.on_inlay_click.clone() {
                handler(anchor, window, cx);
                return true;
            }
        }
        state.handle_click_hover_definition(event, offset, window, cx)
    }""",
    )
    s = s.replace(
        "impl crate::input::InputExtras for super::EditorExtras {",
        "impl crate::input::InputExtras for super::EditorExtras {\n    fn inlays(&self) -> &[crate::input::Inlay] {\n        &self.inlays\n    }\n",
    )
    emod.write_text(s)


def patch_inlay_layout() -> None:
    """The layout half of `patch_inlays`: splice at shaping time, and
    translate between display and buffer offsets everywhere x is mapped."""
    # ── LineLayout: the inlay table and the two conversions ──────────
    tw = VENDOR / "gpui-base/src/input/editor/display_map/text_wrapper.rs"
    s = tw.read_text()
    s = s.replace(
        "    has_background: bool,\n}",
        """    has_background: bool,
    /// VENDOR EDIT — in-text inlays: `(buffer offset within this line, byte
    /// length of the inserted text)`, sorted and non-overlapping.
    ///
    /// The shaped line contains text the BUFFER does not, so display and
    /// buffer byte offsets diverge. Everything outside this struct keeps
    /// working in buffer coordinates — `len` stays the buffer length — and
    /// only the x-mapping functions translate.
    inlays: Vec<(usize, usize)>,
}""",
        1,
    )
    s = s.replace(
        "            whitespace_indicators: None,\n            has_background: false,\n        }\n    }",
        """            whitespace_indicators: None,
            has_background: false,
            inlays: Vec::new(),
        }
    }

    /// Record the inlays spliced into this line's shaped text.
    pub(crate) fn with_inlays(mut self, inlays: Vec<(usize, usize)>) -> Self {
        self.inlays = inlays;
        self
    }

    /// Buffer offset -> display offset: every inlay at or before `offset`
    /// pushes it right by its own length.
    fn to_display(&self, offset: usize) -> usize {
        self.inlays
            .iter()
            .filter(|(at, _)| *at <= offset)
            .map(|(_, len)| *len)
            .sum::<usize>()
            + offset
    }

    /// The inlay a DISPLAY offset falls inside, as its buffer anchor.
    fn inlay_hit(&self, display: usize) -> Option<usize> {
        let mut shift = 0;
        for (at, len) in &self.inlays {
            let start = at + shift;
            if display <= start {
                return None;
            }
            if display < start + len {
                return Some(*at);
            }
            shift += len;
        }
        None
    }

    /// The x span an inlay occupies on screen, for painting inside it.
    pub(crate) fn inlay_x_bounds(&self, anchor: usize) -> Option<(Pixels, Pixels)> {
        let line = self.wrapped_lines.first()?;
        let mut shift = 0;
        for (at, len) in &self.inlays {
            if *at == anchor {
                let start = at + shift;
                return Some((line.x_for_index(start), line.x_for_index(start + len)));
            }
            shift += len;
        }
        None
    }

    /// Which inlay, if any, the position lands in.
    pub(crate) fn inlay_at_position(
        &self,
        pos: Point<Pixels>,
        last_layout: &LastLayout,
    ) -> Option<usize> {
        let (i, offset, x) = self.wrapped_line_at(pos, last_layout)?;
        let display = self.wrapped_lines[i].index_for_x(x)?;
        self.inlay_hit(display).map(|at| offset + at)
    }

    /// Display offset -> buffer offset. An x that lands INSIDE an inlay
    /// resolves to the buffer position it is anchored at — an inlay is not
    /// text you can put a cursor in.
    fn to_buffer(&self, display: usize) -> usize {
        let mut shift = 0;
        for (at, len) in &self.inlays {
            let inlay_display_start = at + shift;
            if display <= inlay_display_start {
                break;
            }
            if display < inlay_display_start + len {
                return *at;
            }
            shift += len;
        }
        display.saturating_sub(shift)
    }""",
        1,
    )
    # the three x-mapping call sites
    s = s.replace(
        "                let x = line.x_for_index(offset.saturating_sub(acc_len))\n                    + x_offset\n                    + self.line_indent(i);",
        "                let local = self.to_display(offset.saturating_sub(acc_len));\n                let x = line.x_for_index(local) + x_offset + self.line_indent(i);",
    )
    s = s.replace(
        "            if x <= line_indent + line.width {\n                return acc_len + line.closest_index_for_x(x - line_indent);\n            }",
        "            if x <= line_indent + line.width {\n                return acc_len + self.to_buffer(line.closest_index_for_x(x - line_indent));\n            }",
    )
    s = s.replace(
        "        Some((offset + ix, line_end_affinity))",
        "        // VENDOR EDIT: `ix` is a DISPLAY offset.\n        Some((offset + self.to_buffer(ix), line_end_affinity))",
    )
    s = s.replace(
        "        Some(offset + self.wrapped_lines[i].index_for_x(x)?)",
        "        // VENDOR EDIT: display offset -> buffer offset.\n        Some(offset + self.to_buffer(self.wrapped_lines[i].index_for_x(x)?))",
    )
    tw.write_text(s)

    # ── the hit-test on state ────────────────────────────────────────
    st = VENDOR / "gpui-base/src/input/base/state.rs"
    s = st.read_text()
    s = s.replace(
        "    pub(crate) fn index_for_mouse_position(&self, position: Point<Pixels>) -> (usize, bool) {",
        """    /// VENDOR EDIT — the inlay under `position`, as its buffer anchor.
    ///
    /// Mirrors [`Self::index_for_mouse_position`]'s traversal; an inlay is
    /// only hit-testable through the same line-layout walk.
    pub(crate) fn inlay_at_mouse_position(&self, position: Point<Pixels>) -> Option<usize> {
        let (bounds, last_layout) = (self.last_bounds.as_ref()?, self.last_layout.as_ref()?);
        let inner_position =
            position - bounds.origin - point(last_layout.line_number_width, px(0.));
        let mut y_offset = last_layout.visible_top;
        for (vi, line_layout) in last_layout.lines.iter().enumerate() {
            let line_start_offset = *last_layout.visible_line_byte_offsets.get(vi)?;
            let pos = inner_position - point(px(0.), y_offset);
            if let Some(local) = line_layout.inlay_at_position(pos, last_layout) {
                return Some(line_start_offset + local);
            }
            y_offset += line_layout.size(last_layout.line_height).height;
        }
        None
    }

    pub(crate) fn index_for_mouse_position(&self, position: Point<Pixels>) -> (usize, bool) {""",
        1,
    )
    st.write_text(s)

    _patch_element()


def _patch_element() -> None:
    """Shaping-time splice and the swatch painted inside a chip."""
    el = VENDOR / "gpui-base/src/input/base/element.rs"
    s = el.read_text()
    s = s.replace(
        "        let mut lines = Vec::with_capacity(last_layout.visible_buffer_lines.len());",
        "        // An inlay borrows the document's font and metrics; only colour is\n"
        "        // its own. Without any run there is no text to inlay into.\n"
        "        let inlay_run_template = runs.first().cloned();\n\n"
        "        let mut lines = Vec::with_capacity(last_layout.visible_buffer_lines.len());",
        1,
    )
    s = s.replace(
        "            let mut wrapped_lines: SmallVec<[ShapedLine; 1]> = SmallVec::with_capacity(1);\n            let mut line_has_background = false;",
        "            let mut wrapped_lines: SmallVec<[ShapedLine; 1]> = SmallVec::with_capacity(1);\n            let mut line_has_background = false;\n            let mut inlays_for_line: Vec<(usize, usize)> = Vec::new();",
    )
    s = s.replace(
        """                let shaped_line = window
                    .text_system()
                    .shape_line(sub_line, font_size, &line_runs, None);""",
        """                // VENDOR EDIT — splice in-text inlays into the shaped line.
                // The shaped text is what the reader sees; the buffer is what
                // the cursor addresses, and they diverge exactly here.
                let line_start = last_layout.visible_line_byte_offsets[vi] + range.start;
                let (sub_line, line_runs, line_inlays) = match &inlay_run_template {
                    Some(template) => splice_inlays(
                        state.extras.inlays(),
                        line_start,
                        sub_line,
                        line_runs,
                        template,
                    ),
                    None => (sub_line, line_runs, Vec::new()),
                };
                inlays_for_line.extend(line_inlays);

                let shaped_line = window
                    .text_system()
                    .shape_line(sub_line, font_size, &line_runs, None);""",
    )
    s = s.replace(
        "            let line_layout = LineLayout::new()\n                .lines(wrapped_lines)",
        "            let line_layout = LineLayout::new()\n                .with_inlays(inlays_for_line)\n                .lines(wrapped_lines)",
    )
    s = s.replace("    document_color_paths: Vec<(Path<Pixels>, Hsla)>,",
        "    document_color_paths: Vec<(Path<Pixels>, Hsla)>,\n    /// VENDOR EDIT — quads drawn INSIDE inlay chips (`Inlay::swatch`).\n    inlay_swatches: Vec<(Bounds<Pixels>, Hsla)>,")
    s = s.replace(
        "        let document_color_paths =\n            self.layout_document_colors(&document_colors, &last_layout, &bounds, cx);",
        "        let document_color_paths =\n            self.layout_document_colors(&document_colors, &last_layout, &bounds, cx);\n        let inlay_swatches = self.layout_inlay_swatches(&last_layout, &bounds, cx);")
    s = s.replace("            document_color_paths,", "            document_color_paths,\n            inlay_swatches,")
    s = s.replace(
        """        for (path, color) in prepaint.document_color_paths.iter() {
            let color = if disabled { color.opacity(0.5) } else { *color };
            window.paint_path(path.clone(), color);
        }""",
        """        for (path, color) in prepaint.document_color_paths.iter() {
            let color = if disabled { color.opacity(0.5) } else { *color };
            window.paint_path(path.clone(), color);
        }

        // VENDOR EDIT — a chip's own drawing, inside its bounds.
        for (rect, color) in prepaint.inlay_swatches.iter() {
            let color = if disabled { color.opacity(0.5) } else { *color };
            window.paint_quad(gpui::fill(*rect, color));
        }""",
    )
    s = s.replace("    fn layout_document_colors(", _SWATCH_LAYOUT + "    fn layout_document_colors(", 1)
    s = s.replace("fn empty_bottom_height(", _SPLICE + "fn empty_bottom_height(", 1)
    el.write_text(s)


def main() -> None:
    src = registry_source()
    if VENDOR.exists():
        shutil.rmtree(VENDOR)
    VENDOR.mkdir(parents=True)
    for crate in CRATES:
        shutil.copytree(src / f"{crate}-0.6.0", VENDOR / crate)
        for path in (VENDOR / crate).rglob("*"):
            path.chmod(path.stat().st_mode | 0o200)
        (VENDOR / crate / ".cargo-ok").unlink(missing_ok=True)

    print("gpui-base            ", rewrite_manifest(VENDOR / "gpui-base/Cargo.toml", {}))
    print(
        "gpui-kit-assets      ",
        rewrite_manifest(VENDOR / "gpui-kit-assets/Cargo.toml", {}),
    )
    print(
        "gpui-component       ",
        rewrite_manifest(
            VENDOR / "gpui-component/Cargo.toml",
            {
                "gpui-base": "../gpui-base",
                "gpui-kit-assets": "../gpui-kit-assets",
                "gpui-component-macros": "../gpui-component-macros",
            },
        ),
    )
    patch_intoplot()
    print("gpui-component-macros 1 (IntoPlot crate lookup)")
    patch_editor_sizable()
    print("gpui-component        1 (Editor: Sizable)")
    patch_inlays()
    patch_inlay_layout()
    print("gpui-base             6 (in-text inlays)")
    print(f"\nDone. `cargo build` now builds against Zed's own gpui at {REV}.")


if __name__ == "__main__":
    main()
