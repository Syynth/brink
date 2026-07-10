# The Two Pipelines

brink can produce a runnable story two ways. You will almost always want the
first; the second exists mainly as a correctness reference. Both emit the same
`StoryData`, so everything downstream — linking, the VM, localization — is
identical regardless of which produced it.

## Native compiler — what you use

```text
.ink source → parse → HIR → analyze → LIR → bytecode codegen → StoryData
```

This is the real compiler: it reads `.ink` source directly. The CLI's
`brink compile` and the library's `brink_compiler::compile_path` both drive it.
If you are writing ink and shipping a game, this is your path — you never touch
the converter.

## Converter — the reference pipeline

```text
.ink.json (inklecate output) → parse → convert → StoryData
```

inklecate is inkle's reference C# compiler. The converter takes *its* JSON
output and turns it into brink `StoryData`. It exists for one reason:
**it is known-good.** Because inklecate is the canonical ink implementation, a
story routed through the converter behaves exactly as inkle's tools intend.

That makes the converter the yardstick the native compiler is measured against.
The test corpus compiles each story *both* ways and compares the runtime
behavior episode-by-episode (see [Test Corpus](../../contributing/test-corpus.md)).
The native compiler is correct to the extent it matches the converter.

## Which should I use?

| You have… | Use |
|-----------|-----|
| `.ink` source | **`brink compile`** (native) — the normal case |
| only inklecate `.ink.json` output | `brink convert` (converter) |
| a need to diff brink against inklecate | both, and compare the `.inkt` dumps |

The native compiler is under active development and not every ink feature is
fully supported yet. Until it reaches parity, the converter is available as a
production-ready path for stories you already have as `.ink.json`. For new work,
write `.ink` and use `brink compile`.

> The translation pipeline (`export-xliff`, `compile-locale`) **only** accepts
> compiled `.inkb` — never feed inklecate `.ink.json` into the intl tooling.
> See [Localization](../localization/overview.md).
