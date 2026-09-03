# brink fix

`brink fix` is `cargo fix`/`eslint --fix` for brink diagnostics: it applies
every fixer the project's policy admits, re-analyzes, and repeats until the
project reaches a fixpoint (or a round cap). It shares its batching engine
(`brink_ide::fix`) with the Studio's "Fix all safe" and the LSP's
`source.fixAll.brink`, so the three surfaces never disagree about what a
batch does — see [`docs/autofix-spec.md`](https://github.com/Syynth/brink/blob/main/docs/autofix-spec.md)
for the full model (tiers, batching algorithm, policy).

```sh
brink fix <PATH> [--dry-run] [--diff [FILE]] [--suggested [CODES]] \
                  [--placeholder] [--code CODES] [--max-rounds N]
```

`PATH` is an entry file — an `.ink` or `.brink` source — addressed exactly
like [`brink compile`](./compile.md): a [`brink.toml`](../project-config.md)
is discovered from its directory and `INCLUDE`s (or the native module graph)
are followed to build the whole project. A bare file with no discovered
`brink.toml` is the same code path, not a separate mode — it just resolves
every fixer's policy to its tier default (see [Tiers](#tiers) below).

## Tiers

Every fixer declares a tier, and the tier decides whether `brink fix` may
apply it without being told to:

| Tier | Meaning | Default policy |
|------|---------|-----------------|
| **Safe** | Observably equivalent to the original — batched unconditionally | Always applied |
| **Suggested** | Probably what the author meant, but changes meaning or loses text | Applied only when the project (or `--suggested`) promotes the code |
| **Placeholder** | Leaves a hole the author must fill by hand | Never applied — `--placeholder` only *lists* these |

A project's [`brink.toml`](../project-config.md) `[fix]` table promotes or
withdraws individual codes:

```toml
[fix]
E033 = "auto"   # promote a Suggested fixer to batch in this project
E014 = "off"    # never offer this fixer here
```

## Options

| Flag | Default | Description |
|------|---------|-------------|
| `--dry-run` | off | Print the report; write nothing to disk. |
| `--diff [FILE]` | off | Emit a `git apply`-able unified diff instead of writing — to stdout, or to `FILE` if given. Implies no disk write, like `--dry-run`. |
| `--suggested [CODES]` | off | Promote the Suggested tier to batchable **for this run only**. Bare, it promotes every Suggested-tier fixer *except one the project's `[fix]` table set to `"off"`* (`off` still means off — a codeless flag isn't the explicit action that widens it); `--suggested E025,E080` names codes explicitly and so wins over `[fix]` for those, even over an `"off"` entry (CLI beats file, like `-D`/`--warn`/`--allow` beat `[lints]`). |
| `--code CODES` | every code | Restrict the run to these diagnostic codes (comma-separated, e.g. `E025,E080`). An unrecognized code is a hard error, not a silent no-op. |
| `--placeholder` | off | Also report every Placeholder-tier fix available, on **stderr** — never applied, since a Placeholder fix always leaves a hole. Written to stderr (not stdout) so it never lands inside a `--diff` patch piped to `git apply`. Useful with `--dry-run` to see where an author still needs to fill something in. |
| `--max-rounds N` | 5 | Round cap for the fixpoint loop. A fixer that never discharges its own diagnostic surfaces as a cap breach naming it, rather than looping forever. |

With none of `--dry-run`/`--diff` given, `brink fix` writes every file the
batch actually changed and prints a short report.

## Exit codes

| Code | Meaning |
|------|---------|
| `0` | The fixpoint was reached — every admitted fix converged within the round cap. |
| `1` | The round cap was hit, or a fixer failed to discharge its own diagnostic (the report names it either way). |
| `2` | Usage error (a bad path, an unrecognized `--code`/`--suggested` code, an I/O failure). |

## Examples

```sh
# Apply every Safe fix (and anything the project's [fix] table promotes),
# write the files.
brink fix story.ink

# See what would change, without writing anything.
brink fix story.ink --dry-run

# Get a patch instead of a write — pipe straight into `git apply`.
brink fix story.ink --diff | git apply

# Promote every Suggested fixer for one run, without editing brink.toml.
brink fix story.ink --suggested

# Promote just E025 (missing-import) for one run.
brink fix story.ink --suggested E025

# Restrict the batch to one code, e.g. while triaging a specific diagnostic.
brink fix story.ink --code E025

# See where an author still has to fill in a required attribute by hand.
brink fix story.ink --dry-run --placeholder
```
