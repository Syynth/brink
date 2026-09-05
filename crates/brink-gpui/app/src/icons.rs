//! The binder's icon language, ported verbatim from the studio.
//!
//! `packages/studio-ui/src/icons.tsx` (#3037) draws these as `currentColor`
//! SVGs. GPUI paints an SVG as a monochrome mask tinted by the element's
//! text color (`Window::paint_svg` takes one `Hsla`), so the source colours
//! here are placeholders — only the ALPHA the shape covers matters, which
//! is why `fill="none" stroke=…` still reads as an outline and
//! `fill=…` as a solid. `opacity` and `stroke-dasharray` survive, so the
//! draft drop stays dashed and the collapse/expand chevrons keep their
//! second, fainter stroke.
//!
//! The `d` attributes are copied from the studio unchanged: the geometry is
//! the thing being evaluated, so re-drawing it by eye would make the
//! comparison meaningless.

use gpui::{Hsla, Pixels, Styled as _, Svg, svg};

/// One icon, sized and tinted. `src` is a complete SVG document.
pub fn icon(src: &'static str, size: Pixels, color: Hsla) -> Svg {
    svg().size(size).text_color(color).data(src.as_bytes())
}

macro_rules! stroke_icon {
    ($body:expr) => {
        concat!(
            r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#000" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">"##,
            $body,
            "</svg>"
        )
    };
}

// ── The brink droplet (files) — viewBox 0 0 100 100 ──────────────────

/// The drop's outline: an expanded file, or one with no knots in it.
pub const FILE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none" stroke="#000" stroke-width="8" stroke-linecap="round" stroke-linejoin="round"><path d="M50 6 C54 16 64 28 73 41 A28 28 0 1 1 27 41 C36 28 46 16 50 6 Z"/></svg>"##;

/// The filled drop: collapsed WITH knots inside (the fill rule, ruled
/// 2026-08-23 — filled = collapsed over content, outline = expanded or leaf).
pub const FILE_FILLED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="#000" stroke="#000" stroke-width="8" stroke-linecap="round" stroke-linejoin="round"><path d="M50 6 C54 16 64 28 73 41 A28 28 0 1 1 27 41 C36 28 46 16 50 6 Z"/></svg>"##;

/// A draft — the same drop drawn provisionally (#3145): a file matching a
/// `[project] drafts` glob that the entry does not reach ("reachability
/// wins", 2026-08-27). Live now that `brink.toml` goes through the
/// session-level `apply_project_config` and the compile closure is
/// established at open; the spike could draw neither half.
pub const FILE_DRAFT: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none" stroke="#000" stroke-width="8" stroke-linecap="round" stroke-linejoin="round" stroke-dasharray="14 11"><path d="M50 6 C54 16 64 28 73 41 A28 28 0 1 1 27 41 C36 28 46 16 50 6 Z"/></svg>"##;

/// The entry file, collapsed: the brand mark — the drop with the divert
/// carved out of the bowl as true negative space (a stroke through a mask).
pub const FILE_ENTRY: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="#000"><mask id="m"><rect x="0" y="0" width="100" height="100" fill="white"/><g transform="translate(3.333 6.569) scale(0.93333)"><path d="M36 54 L56 54 M50 43 L62 54 L50 65" fill="none" stroke="black" stroke-width="7.5" stroke-linecap="round" stroke-linejoin="round"/></g></mask><g transform="translate(3.333 6.569) scale(0.93333)"><path d="M50 0 C54 10 65.6 23.4 74.94 37.34 A30 30 0 1 1 25.06 37.34 C34.4 23.4 46 10 50 0 Z" mask="url(#m)"/></g></svg>"##;

/// The entry file, expanded: the siblings' outline drop with the divert
/// INLAID, so the arrow does not move when the row expands.
pub const FILE_ENTRY_OUTLINE: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 100 100" fill="none" stroke="#000" stroke-width="8" stroke-linecap="round" stroke-linejoin="round"><path d="M50 6 C54 16 64 28 73 41 A28 28 0 1 1 27 41 C36 28 46 16 50 6 Z"/><g transform="translate(3.333 6.569) scale(0.93333)"><path d="M36 54 L56 54 M50 43 L62 54 L50 65" stroke-width="7.5"/></g></svg>"##;

// ── Folders ──────────────────────────────────────────────────────────

pub const FOLDER: &str = stroke_icon!(
    r##"<path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/>"##
);

pub const FOLDER_OPEN: &str = stroke_icon!(
    r##"<path d="M5 19l2.7-6.3A2 2 0 0 1 9.5 11H21l-2.8 6.8a2 2 0 0 1-1.8 1.2z"/><path d="M5 19V7a2 2 0 0 1 2-2h3l2 2h7a2 2 0 0 1 2 2v2"/>"##
);

pub const FOLDER_FILLED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000" stroke="#000" stroke-width="2" stroke-linejoin="round"><path d="M3 7a2 2 0 0 1 2-2h4l2 2h8a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H5a2 2 0 0 1-2-2z"/></svg>"##;

// ── Symbols ──────────────────────────────────────────────────────────

/// A knot — the diamond.
pub const KNOT: &str = stroke_icon!(r##"<path d="M12 3l7 9-7 9-7-9z"/>"##);

/// A knot collapsed over its stitches.
pub const KNOT_FILLED: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><path d="M12 3l7 9-7 9-7-9z"/></svg>"##;

/// A stitch — the turned arrow.
pub const STITCH: &str =
    stroke_icon!(r##"<path d="M6 4v8a4 4 0 0 0 4 4h8"/><path d="M14 12l4 4-4 4"/>"##);

/// A function knot — parentheses.
pub const FUNCTION: &str = stroke_icon!(
    r##"<path d="M8 4c-2 0-3 1-3 4v3c0 2-1 3-2 3 1 0 2 1 2 3v3c0 2 1 4 3 4"/><path d="M16 4c2 0 3 1 3 4v3c0 2 1 3 2 3-1 0-2 1-2 3v3c0 2-1 4-3 4"/>"##
);

/// A non-story file (`brink.toml`, …).
pub const DOC: &str = stroke_icon!(
    r##"<path d="M14 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V8z"/><path d="M14 2v6h6"/>"##
);

// ── Marks and chrome ─────────────────────────────────────────────────

pub const ERROR_MARK: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><circle cx="12" cy="12" r="9"/></svg>"##;

pub const WARNING_MARK: &str = stroke_icon!(
    r##"<path d="M10.29 3.86 1.82 18a2 2 0 0 0 1.71 3h16.94a2 2 0 0 0 1.71-3L13.71 3.86a2 2 0 0 0-3.42 0z"/>"##
);

pub const SEARCH: &str =
    stroke_icon!(r##"<circle cx="11" cy="11" r="7"/><path d="M21 21l-4.3-4.3"/>"##);

/// Group-by-file: three stacked rows under a folded corner.
pub const GROUP_BY_FILE: &str =
    stroke_icon!(r##"<path d="M4 6h16M4 12h10M4 18h13"/><path d="M17 10l3 2-3 2"/>"##);

pub const DOTS: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="#000"><circle cx="5" cy="12" r="1.7"/><circle cx="12" cy="12" r="1.7"/><circle cx="19" cy="12" r="1.7"/></svg>"##;

pub const EXPAND_ALL: &str =
    stroke_icon!(r##"<path d="M7 8l5 5 5-5"/><path d="M7 14l5 5 5-5" opacity=".45"/>"##);

pub const COLLAPSE_ALL: &str =
    stroke_icon!(r##"<path d="M7 16l5-5 5 5"/><path d="M7 10l5-5 5 5" opacity=".45"/>"##);
