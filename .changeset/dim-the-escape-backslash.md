---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

The `\` of an escape now carries its own `escape` semantic token, so the
editor dims it while the character it protects reads as ordinary prose. An
escape exists to say "this character is text"; the mark that says so should
be legible when looked for and invisible when reading.

Also fixes the same mis-highlight #3154 fixed for `.ink` on the NATIVE
surface: an escaped `{` in a `.brink` prose line was painted as
interpolation syntax, because the native prose carve-out
(`is_prose_run_container`) listed `TEXT`/`CUE_NAME`/`TAG`/`SCENE_TITLE` but
not `ESCAPE`.

`escape` is appended to the token legend as index 18; existing indices are
unchanged.
