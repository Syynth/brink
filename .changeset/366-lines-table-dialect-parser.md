---
"@brink-lang/editor": minor
"@brink-lang/web": minor
---

Compiler lines table + public `DialectParser` (#366): a host can now work out the cast (and similar per-line analyses) from the compiler's own line table instead of duplicating the `@Name:<>` convention.

- **`@brink-lang/web`**: `StoryRunnerHandle.linesTable()` returns the compiled program's line table — one entry per scope (root/knot/stitch), project-wide (`INCLUDE`s already resolved by the compile), each line carrying its text (plain or a slot/select template) and, when known, its source span (`file` + byte range). Reuses the exact shape the `export-xliff` CLI path already produces (`brink_intl::export_lines`) rather than inventing a second representation. Static for the loaded program — no running `Story` required.
- **`@brink-lang/editor`**: `DialectParser` (pure TS, no CM6/wasm dependency) — `parseSource(text)` classifies plain `.ink`-style source line-by-line against a `DialogueDialect` (mirrors `element-type.ts`'s classify + chain passes); `parseEmitted(text)` walks *runtime-emitted* text (the post-glue output of `continue_line()`) into composite segments per the pinned iteration protocol: a cue + parenthetical + trailing text emitting as ONE line is the normal case, and a non-reserved-prefix shape (e.g. a parenthetical) never opens a composite line — it only peels as a continuation after a reserved-prefix (cue) segment.
- **`detectCast(lines, dialect)`** ships as the #366 answer to cast detection: it walks `parseSource` output and collects the distinct values of whichever attr a dialect's chain rules `carry` forward (dialect-agnostic — not hardcoded to `speaker`). `characterName()` is NOT exported publicly (stays `screenplay.ts`-internal, per the dialect-spec ruling).

First consumer: celeris cast detection feeding its speaker-color settings surface. The same lines-table exposure serves future analyses (per-speaker word counts, the #362 line-fit metrics epic).
