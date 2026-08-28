---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Diagnostic tooltips get a fixed anatomy and a width cap.

Both producers — the compiler and the prose checker — now render through one
shape: a severity/kind label, the message, the fix buttons on their own row,
and the diagnostic's code as a source tag.

- **Width is capped**, at the same 460px the hover card has always used, now
  shared through one token so the two floating explainers cannot drift apart.
  The lint tooltip previously had no cap at all, so a long message ran to a
  200-character measure and pushed the fixes out of reach.
- **Fixes sit on their own row** with 26px targets, hover, active and
  focus-visible states. Inline, a long message pushed them toward the far
  edge, so reaching one meant crossing the whole message without leaving the
  tooltip.
- **The label carries severity as a word as well as a colour** — the rail
  alone fails a colourblind reader and fails a screenshot pasted into an
  issue, which is how most of these get reported. Prose lints label with the
  checker's rule name (`spelling`), which says more than `info` would.
- **The diagnostic code is shown.** It was computed and then dropped, so
  there was no way to look a diagnostic up from the tooltip.
- `info` severity was never themed, so every prose lint inherited the error
  rail and announced a spelling suggestion in the colour reserved for "this
  will not compile".
- Hover-card rows wrap rather than widening the card, so an `effects` row
  listing several variables no longer fights the cap.
