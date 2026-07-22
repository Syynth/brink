# Project Settings (`brink.toml`)

Every surface that compiles the same project — `brink compile`, `brink ide`,
an embedded editor session, the Studio player — chooses a
[dialect](./dialect/index.md) and a [type policy](./dialect/types.md)
(`gradual` or `strict`). Before `brink.toml`, each mount picked its own
default independently: `brink compile` had `--dialect`/`--types` flags,
`brink ide` had none at all, and an embedder set the wasm editor session's
dialect with `setLanguageDialect`/`setTypePolicy` calls hardcoded in its own
lens code. Two mounts compiling the same project could silently disagree
about which syntax/typing surface it's written in — an author writes syntax
one surface accepts and another rejects.

`brink.toml`, at the project root beside the root `.ink` file, is the one
config every mount reads.

## Schema

```toml
[project]
dialect = "brink"      # "brink" | "strict-ink" (default: "strict-ink")
types   = "gradual"    # "gradual" | "strict"   (default: dialect-keyed —
                       # strict for "brink", gradual for "strict-ink")
```

Both keys are optional. An empty or absent `[project]` table — or no
`brink.toml` at all — changes nothing: **a missing file is exactly today's
behavior**, no regression.

Unknown keys — a stray top-level table, or a key inside `[project]` this
version of `brink` doesn't recognize — are reported as **warnings**, never
compile failures. This is a forward-compatibility guarantee: a `brink.toml`
written against a newer schema still compiles with an older `brink` binary,
just with a warning about the keys it didn't understand.

## Discovery

A mount discovers `brink.toml` by walking **up** from the entry `.ink`
file's directory through each ancestor, stopping at the first `brink.toml`
it finds. The file doesn't have to sit directly beside the entry point — a
multi-file project with `story.ink` in `src/chapters/` and `brink.toml` at
the repo root still finds it.

```text
my-project/
├── brink.toml          ← found even though the entry is nested
└── src/
    └── chapters/
        └── story.ink    ← brink compile src/chapters/story.ink
```

## Precedence: the file is the default, code wins

**An explicit API call or CLI flag always overrides `brink.toml`.** The file
supplies the *default* for a project; an author who reaches for
`--dialect`/`--types` on a single invocation, or an embedder that calls
`setLanguageDialect`/`setTypePolicy` explicitly, is making a deliberate
one-off choice that the file must not silently overrule.

| Source | Wins over |
|--------|-----------|
| `--dialect brink` / `--types strict` (CLI flag actually passed) | `brink.toml`, defaults |
| `setLanguageDialect(...)` / `setTypePolicy(...)` (explicit call) | `brink.toml`, defaults |
| `brink.toml`'s `[project] dialect`/`types` | defaults only |
| Dialect-keyed default (`brink` → `strict`, `strict-ink` → `gradual`) | — |

## Per mount

- **`brink compile`** discovers `brink.toml` from the entry file you pass it.
  `--dialect`/`--types`, when actually given, override the file field-by-field
  (setting only `--dialect` leaves the file's `types`, if any, in effect).
  See [`brink compile`](./cli/compile.md).
- **`brink ide`** has no `--dialect`/`--types` flags of its own — the file
  (or the plain defaults, absent one) is the only source. See
  [`brink ide`](./cli/ide.md).
- **The wasm editor session** (`@brink-lang/web`'s `EditorSessionHandle`) has
  no filesystem of its own. Read `brink.toml`'s text with your host's own
  file APIs (Node `fs`, the browser File System Access API, a bundler
  import, …) and hand it to `applyProjectConfig`:

  ```ts
  import { EditorSessionHandle } from "@brink-lang/web";

  const handle = new EditorSessionHandle();
  const toml = await readProjectFile("brink.toml"); // your own host API
  if (toml !== null) {
    const warnings = handle.applyProjectConfig(toml);
    for (const w of warnings) console.warn(w);
  }
  ```

  Call `applyProjectConfig` once, right after construction, before any
  explicit `setLanguageDialect`/`setTypePolicy` call — a field the session
  already has an explicit value for is left untouched, so a later explicit
  call always wins over an earlier `applyProjectConfig`, matching the CLI's
  flag precedence.

## Driving the compiler as a library

`AnalysisOptions` itself has no notion of a config file — it's the plain
input every mount eventually builds. If you're driving `brink-compiler`
directly (not through the CLI), read and apply `brink.toml` with
`brink-project-config`:

```rust,no_run
# extern crate brink_compiler;
# extern crate brink_project_config;
use std::path::Path;
use brink_compiler::{AnalysisOptions, compile_path_with_options};

let entry = Path::new("story.ink");
let mut options = AnalysisOptions::default();
if let Some(loaded) = brink_project_config::load_from_entry(entry)? {
    for warning in &loaded.warnings {
        eprintln!("{warning}");
    }
    // `false, false`: no explicit override in this example — an embedder
    // with its own flags would pass `true` for any field it's setting itself.
    brink_project_config::apply_to_options(&mut options, &loaded.config, false, false);
}
let output = compile_path_with_options(entry, options)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`dialect`/`types` remain **mount-time-only**: never embedded in `.inkb`,
never delivered to the runtime, exactly as before `brink.toml` existed (see
[Enabling the Dialect](./dialect/enabling.md#what-doesnt-change)).
