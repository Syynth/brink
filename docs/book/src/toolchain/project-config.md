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
unprune-dirs = ["node_modules"]  # directory names a native (.brink) compile
                                 # must not prune from discovery, on top of
                                 # the default target/.git/node_modules list
                                 # — see "Directory discovery pruning" below
                                 # (issue #1407)
conventions = "conventions.brink"  # a project-relative path, or a bare
                                   # built-in preset name (e.g.
                                   # "screenplay"), pointing at the
                                   # project's conventions module (issue
                                   # #1844; see the dialect spec's
                                   # "Where conventions live" section)

[lints]
deny-warnings = true   # promote every Warning-severity diagnostic to
                       # Error (the `-D warnings` equivalent; issue #1160)
E014 = "deny"          # per-code severity override:
                       # "allow" | "warn" | "deny" | "info" | "hint"
                       # ("info"/"hint" down-level to an advisory tier below
                       # Warning — issue #1162)
```

`[project] elements` is a deprecated alias for `conventions` (issue #2180):
it still sets the same value, but emits a `ConfigWarning` naming the
rename — migrate to `conventions` at your own pace.

All keys are optional. An empty or absent `[project]`/`[lints]` table — or
no `brink.toml` at all — changes nothing on a *first* apply: **a missing
file is exactly today's behavior**, no regression. For a long-lived caller
that re-applies `brink.toml` on every change (the wasm editor session, see
below), `[lints]` is the one exception: each apply **replaces** the
resolved lint policy wholesale from whatever the file currently says
(issue #1397), so an empty or absent `[lints]` table on a *later* apply
reverts any codes a previous, non-empty `[lints]` table had set.

Unknown keys — a stray top-level table, a key inside `[project]`, or a
`[lints]` entry naming a code this version of `brink` doesn't recognize (or
one whose default severity is `Error`, so it isn't overridable at all —
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
  specific site, use a `//brink-disable` comment, or — in a `.brink` file —
  an `@[allow(…)]` annotation on the declaration. Both are per-site
  mechanisms, not project-wide policy knobs.
- `info` / `hint` (issue #1162) — down-level the diagnostic to the `Info` or
  `Hint` severity tier respectively, below `Warning`. Like `allow`, both are
  immune to `deny-warnings` (escalating a deliberate downgrade back up would
  defeat the point of it). These map to the LSP client's `Information`/`Hint`
  `DiagnosticSeverity` — the tier IDE conventions use for advisory findings
  that would be too loud as a `Warning` squiggle (e.g. unused-symbol
  dimming). Only `E157` defaults to either tier (`Info`); a project opts any
  other non-`Error`-default code into one explicitly, per code.

### A source-level `@[allow]` wins

In a `.brink` file, `@[allow(E151)]` written above a declaration removes
that diagnostic for the declaration's whole span, and it **beats this
table** — including `E151 = "deny"` and `deny-warnings = true`. The
annotation names one declaration and was written deliberately; `brink.toml`
cannot be that specific. What the annotation cannot do is widen the
suppressible set: it accepts only codes whose *default* severity is not
`Error`, so no `[lints]` entry can make an error-tier code suppressible,
and none can make a non-error-tier code unsuppressible. Naming an unknown
code (`E153`) or an error-tier one (`E154`) is itself a compile error — a
suppression that silently does nothing is never allowed.

Only codes whose *default* severity is not `Error` are overridable at all —
a diagnostic that is a hard error by default (e.g. a parse error) can never
be downgraded through `[lints]`; the table is never even consulted for it.
`E063` (annotation-vs-inference mismatch) is a special case worth knowing:
its own *base* severity is `types`-policy-dependent (`Error` under `types =
strict`), so a `[lints]` entry for it is only ever consulted under `types =
gradual`.

A key that isn't a real diagnostic code, or names a non-overridable one, is
never merged into the resolved policy — it's reported as a warning (the same
channel unknown top-level/`[project]` keys use), never silently dropped.

Every mount now has a CLI/API override tier for `[lints]`/`deny-warnings`,
same as `dialect`/`types` below — always winning over the same code in a
discovered `brink.toml` (see [Precedence](#precedence-the-file-is-the-default-code-wins)
below):

- **`brink compile`** and **`brink ide`** (issue #1373, extended to
  `brink ide` by #1417): repeatable `--deny`/`--warn`/`--allow <CODE>`
  flags, plus `-D warnings` (mirroring `rustc`'s own flag) for
  `deny-warnings`. See [`brink compile`](./cli/compile.md#options) /
  [`brink ide`](./cli/ide.md#anatomy-of-a-command).
- **`bevy-brink`**'s dev-mode `InkLoader`, via
  `BrinkPlugin::with_config(ProjectConfig { lints, deny_warnings, .. })`
  (issue #1394) — the same override also reaches
  `compile_story_inline` (issue #1380), as long as it's called *after*
  the `BrinkPlugin`/`BrinkAssetsPlugin` that carries the override has
  been added to the app.
- **`brink-lsp`** (issue #1417), via
  `initializationOptions.lints`/`.denyWarnings` — see
  [Per mount](#per-mount) below.
- **The wasm editor session** (issue #1417), via
  `EditorSessionHandle.setLintOverrides(json)`/
  `.setDenyWarningsOverride(bool)`/`.clearDenyWarningsOverride()` — see
  [Per mount](#per-mount) below.

## Discovery

A mount discovers `brink.toml` by walking **up** from the entry `.ink`
file's directory through each ancestor, stopping at the first `brink.toml`
it finds. The file doesn't have to sit directly beside the entry point — a
multi-file project with `story.ink` in `src/chapters/` and `brink.toml` at
the repo root still finds it.

For the real-filesystem mounts — `brink compile`, `brink ide`, and
`brink-lsp` — the walk is bounded two ways, either of which stops it. It
never climbs past a directory containing a `.git` entry (an ordinary
repository's `.git/` directory, or a linked worktree's `.git` pointer file);
and, independent of that, it never climbs more than a fixed number of
ancestor directories, so a non-repository tree (no VCS at all, hence no
`.git` boundary to stop at) doesn't climb all the way to the filesystem root
either. Either way, a `brink.toml` that lives outside the bound is never
picked up by these mounts, even by accident — but it isn't treated as
silently as if it didn't exist: discovery reports it back as a warning
(logged by `brink-lsp`; returned alongside the result by
`brink_project_config::load_from_entry`), naming the skipped file so an
author can tell why it wasn't applied.

The virtual mounts have no filesystem or `.git` to bound against, so each is
bounded at its own tree instead: the wasm editor session's
`discoverProjectConfig` never looks past the document tree's own root (see
below), and `bevy-brink`'s dev-mode `InkLoader` never climbs past the asset
source root it was loaded from.

```text
my-project/
├── brink.toml          ← found even though the entry is nested
└── src/
    └── chapters/
        └── story.ink    ← brink compile src/chapters/story.ink
```

## Directory discovery pruning

A native (`.brink`) compile's discovery walk enumerates every `.brink` file
under the project root — but never descends into a directory named `target`,
`.git`, or `node_modules`. These are build output and VCS/dependency
metadata, never a valid source location, and can be enormous; pruning them
is the default with no opt-in required.

Before issue #1407, that pruning was absolute: a project that legitimately
kept `.brink` sources under one of those names got no file and no error —
the source was silently invisible to every compile. Three things changed:

- **An escape hatch.** `[project] unprune-dirs` (the Schema block above)
  names directories that should **not** be pruned, on top of the default
  list. Only entries that are actually one of `target`/`.git`/`node_modules`
  have any effect — a value outside that set is a no-op (nothing was ever
  pruned there) and is reported as a warning, the same "unknown key" channel
  described above, on the theory it's more likely a typo than a deliberate
  no-op.
- **A diagnostic.** When discovery prunes a directory that, within a bounded
  scan of itself, contains a `.brink` file — the shape of "an author
  probably meant for this to be found" — it's reported as a warning naming
  the directory and the `unprune-dirs` fix, rather than saying nothing. The
  scan is bounded by depth and by a total-entry budget (not a full recursive
  descent), deep enough to catch the `node_modules/<package>/lib.brink`
  shape an npm-style dependency tree actually uses, but never turning a
  cheap prune into an expensive walk of the very tree being skipped. A
  directory named by `unprune-dirs` is, naturally, never reported this way —
  it wasn't pruned in the first place.
- **`.gitignore` is deliberately not consulted**, and that's a decision, not
  a gap. Discovery is a deterministic-compilation input: the same tree,
  compiled by anyone, must discover the same files. `.gitignore` resolution
  depends on more than a repository's tracked content — a local uncommitted
  edit, a per-clone `.git/info/exclude`, a user's global `core.excludesFile`
  — any of which could make two checkouts of byte-identical tracked source
  compile differently. `unprune-dirs` avoids exactly that: it lives in
  `brink.toml`, itself tracked, versioned source, so it resolves the same
  way on every clone.

Both the escape hatch and the diagnostic live once, as opt-in builders
(`Walk::allow`, `Walk::warn_on_pruned_sources`) on the shared recursive walk
every native discovery traversal goes through — so a *new* traversal never
has to reimplement the pruning policy itself. But each builder is still
opt-in per traversal: today only the `brink compile` / `brink ide` path
(`brink-driver`'s `RealFs::list`) wires them up. `brink-lsp`'s own
workspace-scan walk calls the shared `Walk` unadorned, so an LSP-open
project honors neither `unprune-dirs` nor the silent-skip diagnostic yet —
tracked as a follow-up to wire both into `brink-lsp`.

## Precedence: the file is the default, code wins

**An explicit API call or CLI flag always overrides `brink.toml`.** The file
supplies the *default* for a project; an author who reaches for
`--dialect`/`--types` on a single invocation, or an embedder that calls
`setLanguageDialect`/`setTypePolicy` explicitly, is making a deliberate
one-off choice that the file must not silently overrule.

| Source | Wins over |
|--------|-----------|
| `--dialect brink` / `--types strict` (CLI flag actually passed) | `brink.toml`, defaults |
| `--deny`/`--warn`/`--allow <CODE>` / `-D warnings` (`brink compile`/`brink ide`, CLI flag actually passed) | `brink.toml`, defaults |
| `initializationOptions.lints`/`.denyWarnings` (`brink-lsp`, key actually set at `initialize`) | `brink.toml`, defaults |
| `setLanguageDialect(...)` / `setTypePolicy(...)` (explicit call) | `brink.toml`, defaults |
| `setLintOverrides(...)` / `setDenyWarningsOverride(...)` (wasm editor session, explicit call) | `brink.toml`, defaults |
| `BrinkPlugin::with_config(...)` / `BrinkAssetsPlugin::with_config(...)` (`bevy-brink`, field actually set — reaches `InkLoader` and `compile_story_inline`) | `brink.toml`, defaults |
| `brink.toml`'s `[project] dialect`/`types` | defaults only |
| `brink.toml`'s `[lints]`/`deny-warnings` (for a code without a CLI/API override above) | defaults only |
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
  (or the plain defaults, absent one) is the only source for those two. It
  does have a `--deny`/`--warn`/`--allow <CODE>` / `-D warnings` tier for
  `[lints]`/`deny-warnings` (issue #1417), identical to `brink compile`'s and
  applied the same way — an explicit flag wins over the file for that code,
  every other code in the file's `[lints]` table still applies. Every
  subcommand that loads a project honors it. See
  [`brink ide`](./cli/ide.md#anatomy-of-a-command).
- **`brink-lsp`** discovers `brink.toml` from the workspace roots the client
  declares at `initialize`, resolving `[project] dialect`/`types` *and*
  `[lints]`/`deny-warnings` into its shared `LanguageOptions`. A later
  `workspace/didChangeConfiguration` notification or a watched edit to
  `brink.toml` re-resolves and re-stores the policy (`reload_brink_toml`),
  so published diagnostic severity picks up a `[lints]` change without a
  client restart. `initializationOptions.lints` (issue #1417) is an object
  `{ "<CODE>": "deny" | "warn" | "allow" | "info" | "hint" }` (the last two
  added by issue #1162), and
  `initializationOptions.denyWarnings` a boolean — both resolved once at
  `initialize` (mirroring `initializationOptions.dialect`/`.types`) and
  applied last, so they always win over the same code in the discovered
  `brink.toml`. An unrecognized per-code level string, or an unrecognized/
  non-overridable code, is reported through the server's usual
  `tracing::warn!` channel, never silently dropped. A second, independent
  mechanism also dims text in the client (issue #1618): `E033` (unreachable
  code after a divert) and `E095` (`#@was` self-alias) publish with LSP's
  `DiagnosticTag::UNNECESSARY`, which VS Code and similar clients render as
  faded/dimmed rather than underlined. This tag is orthogonal to
  severity — it rides alongside whatever severity the code is published at
  (including the `Warning` default these two carry today), not another tier
  like `Info`/`Hint` above.
- **The wasm editor session** (`@brink-lang/web`'s `EditorSessionHandle`) has
  no filesystem of its own — but it is inherently virtual, so it discovers
  `brink.toml` the same way `brink compile`/`brink ide` do: by walking its
  own document tree, not a real filesystem (issue #1414). Serve `brink.toml`
  as an ordinary document — `updateFile("brink.toml", text)`, at the entry's
  directory or any ancestor of it — and call `discoverProjectConfig(entry)`.
  **It applies `[project] dialect`/`types` *and* `[lints]`/`deny-warnings`**
  (issue #1366) — diagnostic severity rendered through this surface now
  reflects the file the same way `brink compile`, `brink ide`, and
  `brink-lsp` already did. Because this session is long-lived, a repeated
  `applyProjectConfig`/`discoverProjectConfig` call fully **re-resolves**
  `[lints]` from the file each time rather than merging onto the previous
  result (issue #1397) — a code or `deny-warnings` present in an earlier
  `brink.toml` but absent from the current one reverts to its default
  severity:

  ```ts
  import { EditorSessionHandle } from "@brink-lang/web";

  const handle = new EditorSessionHandle();
  const toml = await readProjectFile("brink.toml"); // your own host API
  if (toml !== null) {
    handle.updateFile("brink.toml", toml);
  }
  handle.updateFile("story.ink", await readProjectFile("story.ink"));
  const warnings = handle.discoverProjectConfig("story.ink");
  for (const w of warnings) console.warn(w);
  ```

  Call `discoverProjectConfig` once, after the project's files are loaded
  and before any explicit `setLanguageDialect`/`setTypePolicy` call — a
  field the session already has an explicit value for is left untouched, so
  a later explicit call always wins over an earlier `discoverProjectConfig`,
  matching the CLI's flag precedence. Returns `[]` (never throws) when no
  `brink.toml` is found anywhere from the entry's directory up to the tree
  root.

  `entry` must use the same root-relative spelling (no leading `/`) as every
  document path given to `updateFile`/`updateSource` — the walk-up matches
  keys by exact string equality. Mixing a `/`-prefixed path with unprefixed
  ones is a silent no-op: discovery finds nothing and `discoverProjectConfig`
  returns `[]` exactly as if no `brink.toml` existed, with no warning.

  If your embedder reads `brink.toml`'s text with its own host file API
  (Node `fs`, the browser File System Access API, a bundler import, …) and
  would rather hand that text in directly than load it as a document, use
  `applyProjectConfig(toml)` instead — the same application/precedence
  rules apply, just without the discovery step.

  An embedder that wants to set `[lints]`/`deny-warnings` policy
  programmatically — without shipping a `brink.toml` at all, or to override
  one it doesn't control — calls `setLintOverrides(json)` (issue #1417): a
  JSON object `{ "<CODE>": "deny" | "warn" | "allow" | "info" | "hint" }`
  (the last two added by issue #1162) that **replaces**
  the session's explicit override map (`"{}"` clears it), plus
  `setDenyWarningsOverride(bool)`/`clearDenyWarningsOverride()` for the
  blanket flag. Both always win over the same code in an applied
  `brink.toml`'s `[lints]` table, in either call order — a later
  `applyProjectConfig`/`discoverProjectConfig` re-applies the explicit
  overrides on top of whatever it just resolved from the file, so a
  `brink.toml` reload can never silently drop a previously-set override.
  Returns the unrecognized-level/unrecognized-code warnings as JSON (a
  `string[]`), the same channel `applyProjectConfig` uses.

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
let (loaded, discovery_warnings) = brink_project_config::load_from_entry(entry)?;
// A `brink.toml` the bounded discovery walk stepped over (a workspace/git
// boundary, or the ancestor-depth cap for a VCS-less tree) is reported here
// rather than silently ignored — never applied, but worth telling the
// author about (issue #1435).
for warning in &discovery_warnings {
    eprintln!("{warning}");
}
if let Some(loaded) = loaded {
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
