---
"@brink-lang/web": patch
---

Four output-order fixes for functions that print, found by the program
generator's functions tier and matched against the ink reference:
a function-end whitespace trim that now skips over glue (`{0} <>` at
the end of a function glues to the next line, #3522); a leading newline
on every multi-line conditional arm, `- else:` included (#3523); a
function's newline is dropped while the function has printed nothing,
so two calls on one line no longer break it (#3519); and a slot
expression that *contains* a call (`{f() == "x"}`) composes the call's
output into the slot the way a bare `{f()}` does (#3525).
