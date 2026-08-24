# Binder v2 — design source

Artboard sources for the binder redesign (ruled 2026-08-23 in-session;
maintainer-annotated on the design canvas, all notes incorporated). The
model borrows deliberately from the maintainer's celeris binder
(`@s92/spine-space`, `docs/design/spine-space.md` in that repo) —
re-implemented here, not ported, per the ruling.

| File | Screen |
|------|--------|
| `Main.dc.html` | Files mode — the calm default: files/folders only, SVG icons, sidecar order, diagnostics marks, pinned brink.toml, 50/50 icon create buttons |
| `Structure.dc.html` | Structure mode — the toggle flipped: knot/stitch/function rows with grab handles; all structural ops intact |
| `CreateRow.dc.html` | Inline creation — 50/50 icon buttons → inline name input → inline validation |
| `Order.dc.html` | The `.binder.json` sidecar — per-container order + empty-folder registry, six rules |
| `Search.dc.html` | Search & filter — one query over file names, structural names, and #tags |
| `canvas.json` | Canvas layout (the maintainer's annotation notes are preserved verbatim) |

## Implementing? Compare against

| Issue | File | Region |
|-------|------|--------|
| toggle + noise | `Main` vs `Structure` | header segmented icon toggle; symbol rows only in Structure |
| icons | `Main`, `Structure` | brink droplet for `.ink`, folder/knot(diamond)/stitch(branch)/function(parens) — currentColor SVG, no glyph characters |
| creation | `CreateRow` + both mode boards | 50/50 dashed icon buttons per container tail + root foot; tooltip carries the words |
| sidecar order | `Order` | the six rules — listed-then-fallback, rekey, `folders` registry for empty dirs, compiler-invisible, self-healing |
| search | `Search` | one query, three namespaces; tag chip shows why a row matched; works in both modes |
| diagnostics | `Main`, `Structure` | error/warning count marks; symbol shows own count, file the sum |
| pinned config | `Main` | brink.toml row above the foot, opens the #3015 form view |

Values match the shipped app: Catppuccin Mocha under `--bs-*`, 26px rows,
`system-ui`, accent `#89b4fa`. The rendered canvas link lives on the epic
issue (artifact URLs are per-account).
