# brink brand assets

The brink mark: an ink drop with the divert arrow (`->`) carved out of the bowl —
ink, on the brink, pointed at what happens next.

| File | What it is |
|------|------------|
| `brink-icon-night.svg` | The app icon: night squircle tile (`#101420`), wet-ink drop (`#7E96FF`), divert carve painted in the tile color. Source for generated icon sets (e.g. `tauri icon`). |
| `brink-glyph.svg` | The drop alone with the carve as true negative space (SVG mask), for use on any background. Flatten to a single outline path if a consumer can't handle masks. |

Construction: the bowl is a perfect circle (r 30 in glyph units) with shoulder curves
held tangent at the join, meeting in a ~67° tip; the arrow is one stroke weight (7.5)
and mass-centered on the bowl. In the icon tile the glyph is 53% of tile height,
bounding-box centered. The full arrow is used at every size — there is no simplified
small-size variant.

Palette: night `#101420` · wet ink `#7E96FF` · ink blue `#2C46C8` · iron-gall `#1B2333` · paper `#F4F4F0`.
