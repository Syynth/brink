# @brink-lang/dialect

The dialogue-dialect artifact, pure TypeScript with no runtime dependencies:
the `DialogueDialect` types, `validateDialect`, `ResolvedDialect`,
`DialectParser` (source-side classification and emitted-side parsing),
`extendDialect`, `detectCast`, and `runsOf` — the run rule that decides who
is speaking in runtime-emitted text.

`brink compile` writes a project's **resolved** dialect (`brink.toml
[dialogue]` with the preset merged and affix sugar expanded) as
`dialect.json` next to the compiled story. A game engine imports that file
and this package to read its own conventions the way the studio does:

```ts
import { DialectParser, runsOf, type DialogueDialect } from "@brink-lang/dialect";
import dialect from "./story.dialect.json";

const parser = new DialectParser(dialect as DialogueDialect);
const lines = delivered.map((text) => ({ segments: parser.parseEmitted(text) }));
for (const run of runsOf(lines, dialect as DialogueDialect)) {
  // run.kind, run.attrs.speaker, run.lines (indices into `delivered`)
}
```

See the book: *Conventions for your engine*.
