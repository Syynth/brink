---
"@brink-lang/web": patch
---

Issue #1993: `RuntimeError::RanOutOfContent` is now a tuple variant
carrying a `RanOutOfContentCause` (`Tunnel` / `Function` / `Plain` /
`Unknown`) instead of a bare unit variant — a breaking change for any
consumer matching the old shape. The four messages mirror C#'s
`Story.cs` call-stack selection (`CanPop(Tunnel)` / `CanPop(Function)` /
`!canPop` / backstop) word-for-word.

`RanOutOfContentCause` is exported alongside `RuntimeError`.

In practice only `Plain` is reachable through any story today — its
message text is byte-identical to the old unit variant's, so this ships
with no behavioral change for `@brink-lang/web` consumers. The other
three causes classify correctly at the instant a frame's content is
discovered exhausted, but the classification is only ever persisted for
the exhaustion that actually produces the terminal `Done`: this runtime's
own frame-popping (unlike C#'s) always unwinds an exhausted Tunnel frame
even with nothing pending, so a Tunnel or Function frame's exhaustion
cascades down to the root frame's own `Plain` exhaustion before the
deferred fault ever reads it (tracked in #2005; see
`tunnel_fall_off_classifies_as_plain_not_tunnel_today` /
`function_fall_off_classifies_as_plain_not_function_today` in
`crates/brink-runtime/tests/terminal_classification.rs`).
