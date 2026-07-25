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

[lints]
deny-warnings = true   # promote every Warning-severity diagnostic to
                       # Error (the `-D warnings` equivalent; issue #1160)
E014 = "deny"          # per-code severity override: "allow" | "warn" | "deny"
```

All keys are optional. An empty or absent `[project]`/`[lints]` table — or
no `brink.toml` at all — changes nothing: **a missing file is exactly
today's behavior**, no regression.

Unknown keys — a stray top-level table, a key inside `[project]`, or a
`[lints]` entry naming a code this version of `brink` doesn't recognize (or
one whose default severity isn't `Warning`, so it isn't overridable at all —
see [Lint severity](#lint-severity) below) — are reported as
**warnings**, never compile failures. This is a forward-compatibility
guarantee: a `brink.toml` written against a newer schema still compiles with
an older `brink` binary, just with a warning about the keys it didn't
understand.

## Lint severity

`[lints]` (issue #1160) is shaped like Rust's own `[lints]` table, but is
**not** a semantic drop-in for it. Each key other than the reserved
`deny-warnings` names a diagnostic code (`"E014"`) mapped to a severity:

- `deny` — always `Error`, regardless of `deny-warnings`.
- `warn` — the code's ordinary behavior: `Warning`, promoted to `Error` by
  `deny-warnings` like any other unconfigured warning.
- `allow` — **unlike Rust's `allow`, this does not remove the diagnostic.**
  It only buys immunity from `deny-warnings`; the diagnostic still resolves
  to `Warning` and is still reported. To actually suppress a diagnostic at a
  specific site, use a `//brink-disable` comment instead — a different,
  per-site mechanism, not a project-wide policy knob.

Only codes whose *default* severity is `Warning` are overridable at all — a
diagnostic that is a hard error by default (e.g. a parse error) can never be
downgraded through `[lints]`; the table is never even consulted for it.
`E063` (annotation-vs-inference mismatch) is a special case worth knowing:
its own *base* severity is `types`-policy-dependent (`Error` under `types =
strict`), so a `[lints]` entry for it is only ever consulted under `types =
gradual`.

A key that isn't a real diagnostic code, or names a non-overridable one, is
never merged into the resolved policy — it's reported as a warning (the same
channel unknown top-level/`[project]` keys use), never silently dropped.

`brink compile` has a CLI override tier for `[lints]`/`deny-warnings`, same
as `dialect`/`types` below: repeatable `--deny`/`--warn`/`--allow <CODE>`
flags, plus `-D warnings` (mirroring `rustc`'s own flag) for `deny-warnings`.
See [`brink compile`](./cli/compile.md#options) and
[Precedence](#precedence-the-file-is-the-default-code-wins) below. No other
mount has an override source for `[lints]`/`deny-warnings` yet — `brink ide`,
`brink-lsp`, and the wasm editor session all still resolve it from
`brink.toml` (or the plain default) alone.

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
| `--deny`/`--warn`/`--allow <CODE>` / `-D warnings` (`brink compile` only, CLI flag actually passed) | `brink.toml`, defaults |
| `setLanguageDialect(...)` / `setTypePolicy(...)` (explicit call) | `brink.toml`, defaults |
| `brink.toml`'s `[project] dialect`/`types` | defaults only |
| `brink.toml`'s `[lints]`/`deny-warnings` (for a code without a `brink compile` CLI override) | defaults only |
| Dialect-keyed default (`brink` → `strict`, `strict-ink` → `gradual`) | — |

## Per mount

- **`brink compile`** discovers `brink.toml` from the entry file you pass it.
  `--dialect`/`--types`, when actually given, override the file field-by-field
  (setting only `--dialect` leaves the file's `types`, if any, in effect).
  `[lints]`/`deny-warnings` apply too — a build that previously succeeded
  with a warning can now fail. `--deny`/`--warn`/`--allow <CODE>` and `-D
  warnings` override the file the same way, per code (issue #1373): passing
  `--allow E014` wins over a `brink.toml` `E014 = "deny"` for that code,
  while any other code in the file's `[lints]` table still applies. See
  [`brink compile`](./cli/compile.md).
- **`brink ide`** has no `--dialect`/`--types` flags of its own — the file
  (or the plain defaults, absent one) is the only source, and this includes
  `[lints]`/`deny-warnings`. See [`brink ide`](./cli/ide.md).
- **`brink-lsp`** discovers `brink.toml` from the workspace roots the client
  declares at `initialize`, resolving `[project] dialect`/`types` *and*
  `[lints]`/`deny-warnings` into its shared `LanguageOptions`. A later
  `workspace/didChangeConfiguration` notification or a watched edit to
  `brink.toml` re-resolves and re-stores the policy (`reload_brink_toml`),
  so published diagnostic severity picks up a `[lints]` change without a
  client restart.
- **The wasm editor session** (`@brink-lang/web`'s `EditorSessionHandle`) has
  no filesystem of its own. Read `brink.toml`'s text with your host's own
  file APIs (Node `fs`, the browser File System Access API, a bundler
  import, …) and hand it to `applyProjectConfig`. **`applyProjectConfig`
  applies only `[project] dialect`/`types`; `[lints]`/`deny-warnings` are not
  wired to the wasm editor session yet.** It is the only surface whose
  diagnostic output does not change severity based on `[lints]` today —
  `brink compile`, `brink ide`, and `brink-lsp` all do:

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
    options.apply_project_config(&loaded.config, false, false);
}
let output = compile_path_with_options(entry, options)?;
# Ok::<(), Box<dyn std::error::Error>>(())
```

`dialect`/`types` remain **mount-time-only**: never embedded in `.inkb`,
never delivered to the runtime, exactly as before `brink.toml` existed (see
[Enabling the Dialect](./dialect/enabling.md#what-doesnt-change)).
