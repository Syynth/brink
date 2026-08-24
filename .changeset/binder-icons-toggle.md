---
"@brink-lang/studio": minor
---

Binder v2, part 1 (#3036, #3037): Files/Structure mode toggle — symbol
rows (knots/stitches/functions) now render only in Structure mode;
files-only is the default, cutting the always-on tree-of-trees noise. A
header toolbar carries the segmented icon toggle plus expand-all /
collapse-all. Every glyph-character icon (📄 📁 ◆ ◇ ƒ 📚 ▶) is replaced
by a monochrome currentColor SVG set (the brink droplet for .ink files),
and draggable rows reveal a grab handle on hover. All structural ops are
unchanged in Structure mode.
