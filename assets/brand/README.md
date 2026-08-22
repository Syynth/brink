# brink brand assets

The brink mark: an ink drop with the divert arrow (`->`) carved out of the bowl —
ink, on the brink, pointed at what happens next.

| File | What it is |
|------|------------|
| `brink-icon-night.svg` | The mark on its night squircle tile — for anywhere the tile is drawn by us (docs, web, README headers). **Not** the source for OS icon sets; see the note below. |
| `brink-icon-fullbleed.svg` | **The source for generated OS icon sets** (`tauri icon`, and any `.icns`/`.ico` pipeline). Identical glyph and geometry, but the ground is a full square instead of a squircle. |
| `brink-glyph.svg` | The drop alone with the carve as true negative space (SVG mask), for use on any background. Flatten to a single outline path if a consumer can't handle masks. |

Construction: the bowl is a perfect circle (r 30 in glyph units) with shoulder curves
held tangent at the join, meeting in a ~67° tip; the arrow is one stroke weight (7.5)
and mass-centered on the bowl. In the icon tile the glyph is 53% of tile height,
bounding-box centered. The full arrow is used at every size — there is no simplified
small-size variant.

⚠ **Do not feed `brink-icon-night.svg` to an OS icon generator.** It draws its own
squircle, so the corners of the canvas are transparent — and modern macOS treats a
non-conforming icon by compositing it onto its *own* rounded plate, giving a visibly
nested squircle-in-a-squircle in the Dock (observed 2026-08-21). OS icon pipelines
want **full-bleed** art and apply the platform's mask themselves, which is what
`brink-icon-fullbleed.svg` is for. The two files share glyph geometry exactly; only
the ground differs.

Palette: night `#101420` · wet ink `#7E96FF` · ink blue `#2C46C8` · iron-gall `#1B2333` · paper `#F4F4F0`.
