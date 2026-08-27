---
"@brink-lang/studio": minor
---

Documents now carry a file icon before their name in every view that names
one — the Code view's tab, the Single File header, the Continuous section
heading, the takeover header.

Draft status (#3145) moves into that icon: a draft is the same ink-file
drop drawn provisionally, dashed and orange, replacing the "DRAFT" text
badge. A badge was a second element competing with the filename for the
same row; the icon is already beside the name, so it carries the status
for free and cannot drift away from what it describes.

The shell prop `documentMark` is now `documentIcon`, and renders before the
name rather than after.
