---
"@brink-lang/web": patch
---

A choice's text keeps the whitespace on both sides of an elided mid-line comment, as ink does: `* Hello /* c */ world` is offered as `Hello  world` (two spaces), not `Hello world` (#2975). Content lines, including a chosen choice's echoed text, still read as one space, matching ink. A line split only by a comment is now one line-table entry, not two.
