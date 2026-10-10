---
"@brink-lang/web": minor
---

`brink.toml` `[cast]`: per-speaker colours for the Player

`[cast.<name>]` tables with a `color` (`#rgb` or `#rrggbb`) are now a known
section, so the editor session no longer warns about an unknown top-level
`cast` key. `EditorSession.configured_cast()` and
`EditorSessionHandle.getConfiguredCast()` report the speakers with a valid
colour as `{ name, color }` (colour normalised to lowercase `#rrggbb`). A
malformed entry or colour is a warning on `brink.toml` and never fails the
rest of the config. Names match a cue's speaker ignoring case.
