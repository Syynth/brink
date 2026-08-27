---
"@brink-lang/studio": minor
---

Right-click a row in the Problems panel to silence that diagnostic, at
three scopes — this line, this file, or this project. Each writes a
directive the compiler already understands: `// brink-disable Exxx` above
the line, `// brink-disable-file` at the top, or `[lints] Exxx = "allow"`
in `brink.toml`.

A code the compiler will not let you suppress — anything error-tier — gets
no suppression items, since every channel would refuse it.
