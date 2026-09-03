---
"@brink-lang/web": patch
---

New `Safe` auto-fixer for `E014` ("logic line has no effect"): deletes a
bare `~` line — one with no statement and no expression at all — along
with its line break. The fixer re-derives effect-freedom from the source
itself rather than trusting the diagnostic, so it refuses the handful of
unrelated malformed-partial parses that share the `E014` code (a `~ temp`
or assignment missing a name, place, or value) and never fires on a
native file (native's own "nothing after `~`" shape raises `E015`
instead). Offered fixes and `fix_all` results reaching `EditorSession`
now include this code.
