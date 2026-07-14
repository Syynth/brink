---
"@brink-lang/web": patch
---

Fixed formatter line classification for constructs containing inline
`/* … */` block comments (observable via `format_document`). The line
classifier used to mark any physical line containing a block-comment
token anywhere in its subtree as a pure comment line, which skipped the
line's real construct entirely:

- a single-line `STRUCT Point = #{x: float, /* mid */ y: float}` was
  passed through verbatim instead of being normalized by the struct
  renderer;
- a block comment on a multiline struct's `#{ /* c */` opening line (or
  a `~ { /* c */` logic-block opening line) caused the entire body to
  lose its indentation;
- a one-liner `~ x = 5 /* foo */` logic line skipped `~`-spacing
  normalization.

A single-line block comment nested inside a construct whose renderer
handles comments itself (struct bodies, `~ { … }` block bodies, plain
`~` logic lines) is now left to that construct's formatting.
Free-floating comments — banners, multi-line comments, and comments
outside those regions (e.g. `STRUCT Point /* c */ = #{…}` or trailing
after a block's closing `}`) — keep the verbatim treatment.
