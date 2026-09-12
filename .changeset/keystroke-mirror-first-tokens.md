---
"@brink-lang/editor": patch
---

Keystroke path: serve fast semantic tokens wholly from the classifier
plane when no refined slices are cached, instead of also asking the
project session for a segment manifest. The session's manifest re-lexes
the whole file (0.6 ms on a 100 KB story) and the classifier had already
paid that lex, so a keystroke on a large document lexed it twice. Output
is unchanged — with nothing refined to preserve, every segment already
came from the classifier — and a desynced mirror or an unservable key
still falls back to the session road.
