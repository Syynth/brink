---
"@brink-lang/studio": patch
"@brink-lang/web": patch
---

"Suppress a code in this file" now suppresses that code, not the whole file.

The Problems panel offered **Suppress E157 in this file** and wrote a bare
`// brink-disable-file`, which silences every diagnostic in the file. The
label and the effect disagreed, and `// brink-disable-file E157` — written
by hand, in the obvious analogy to the line-scoped form — matched no
directive at all and was dropped in silence.

- `// brink-disable-file E027 E035` suppresses those codes for the whole
  file. Whitespace-separated, matching `// brink-disable E027 E035`.
- `// brink-disable-file-all` is the blanket gesture's new spelling.
- The Problems menu offers both as separate items, each labelled for what it
  does.
- A `brink-disable`/`brink-expect` comment the parser cannot read is now
  reported as **E192** instead of vanishing.

`// brink-disable-all` (project-wide) is unchanged.
