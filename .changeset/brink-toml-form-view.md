---
"@brink-lang/studio": minor
---

Structured form view for brink.toml (#3015): opening a brink.toml now
renders a form panel above the raw text editor — entry and conventions
offer the project's actual files (a typo'd entry reproduces the
silent-dead-Player failure, which is why free text was the problem),
dialect and types offer the schema's values, and a configured value
naming a missing file is flagged "(missing)" rather than rewritten.
Edits are comment-preserving targeted line operations, never a
parse-and-reserialize; the text editor below remains the escape hatch
for anything the form doesn't model (e.g. [lints]).
