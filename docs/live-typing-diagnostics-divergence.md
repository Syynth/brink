# Live-typing vs. db diagnostics divergence (issue #1347)

**Status: resolved by #1358.** #1358 ("thread native-awareness through the
pure `analyze_with_modules` path") implemented §4's **option B** below:
`IdeSnapshot` now carries the db's real `is_native` and threads it through
`analyze_with_modules` → `finish_analysis` → `per_file_diagnostics` (and adds
the missing `E137` call site). That closes every divergence measured in §3 —
`crates/internal/brink-ide/tests/live_typing_db_divergence.rs` now pins
*agreement* between the two surfaces on the same fixtures that used to pin
the mismatch.

The rest of this document is kept as the original measurement that grounded
the ruling — it is what #1358 fixed, not a live description of current
behavior. Every code coordinate and diagnostic code below was read or
reproduced at `auto/issue-1347` (branched from `origin/main` at `999581354`).

## 1. What #1347 asked

> should live-typing diagnostics route through the same salsa seam, or is
> compile-time-only correct for these?

The issue framed the gap as one-directional and one-code-wide: `E137` (and
"structurally any check wired at `per_file_diagnostics_query`") is reachable
only via an explicit `compile()`, so an author sees a clean buffer for a real
error until they compile.

**The measurement says the gap is wider than that, and it runs in both
directions.** The live-typing path does not merely *miss* diagnostics — on a
native `.brink` file under the default dialect it *invents* one the compiler
does not agree with.

## 2. The three analysis paths

| Path | Producer | `is_native` supplied to `per_file_diagnostics` |
|---|---|---|
| `IdeSession` live typing (`@brink-lang/web` editor squiggles) | `IdeSnapshot::analyze` → `brink_analyzer::analyze_with_modules(…, false)` (`crates/internal/brink-ide/src/session.rs:73`) | always `false` |
| `brink-lsp` background analysis | `analyze_with_modules(…, *is_native)` (`crates/brink-lsp/src/backend.rs:2694`) | always `false` |
| Compile / db queries | `per_file_diagnostics_query` (`crates/internal/brink-db/src/queries/analysis.rs:127`) | real per-file value |

The two `analyze_with_modules` rows land on `false` for different reasons.
`IdeSession` passes a literal `false`. `brink-lsp` passes the project's real
`is_native` — but `analyze_with_modules` forwards that argument only to
`symbol_index_with_modules` (the M-2d duplicate gate); its `finish_analysis`
call then hardcodes `false` for every per-file contributor
(`crates/internal/brink-analyzer/src/lib.rs:1285-1296`). So **no pure-path
caller can currently reach `per_file_diagnostics`'s `is_native = true`,
whatever it passes.**

The db path is the only one that can, because `is_native` is derived from a
file *path* (`file_language`, `crates/internal/brink-db/src/queries/mod.rs:407`)
and the pure path's inputs are `(FileId, HirFile, SymbolManifest)` triples that
carry no path. This is the same root cause as #1526 (module identity is
path-derived) and it was solved there by *copying the db-derived fact into the
snapshot*, not by removing the second path.

`native_strict_only_error` (`E137`) is a further step removed: it has no
pure-path call site at all. `finish_analysis` never calls it; only
`per_file_diagnostics_query` (`crates/internal/brink-db/src/queries/analysis.rs:148`)
and two test/corpus harnesses do.

## 3. Measured divergence

Reproduced through `IdeSession` — `session.analysis()` is the live-typing
result the editor renders, `session.db().analysis()` is what a compile sees.
Both are read from the same session, same files, same options.

### Native `.brink` file

| Session dialect | Live typing (squiggles) | Compile / db | Verdict |
|---|---|---|---|
| `strict-ink` (the `EditorSession` default) | `E051` | `E084`, `E137` | one **false positive**, two **false negatives** |
| `brink` | `E084` | `E084`, `E137` | one **false negative** |

Also confirmed missing from live typing on native files under the default
dialect: `E106` (map-literal key domain) and `E138` (map-literal duplicate
key) — both gated on `dialect == Brink || is_native` inside
`brink_analyzer::per_file_diagnostics`.

### Ink `.ink` file

Identical on both surfaces under both dialects. **The divergence is
native-only.** No ink-corpus behavior is implicated, which is why the oracle
ratchet is not a constraint on any of the options below.

### Why the `E051` false positive is the sharper problem

`E051` is the T1b dialect gate's "brink extension syntax under `strict-ink`"
error. A native `.brink` file's own grammar *is* the superset grammar the gate
exists to police (#1348's reasoning), so the gate must not run on it at all —
which is exactly what `per_file_diagnostics_query`'s `is_native` arranges for
the db path. On the live-typing path the gate does run, so `Point { x: 3, x: 4 }`,
`Map { … }`, and UFCS calls in an ordinary `.brink` file get a red squiggle
that disappears the instant the author compiles.

`EditorSession` defaults `dialect` to `StrictInk`
(`crates/brink-web/src/editor/mod.rs:68-78`), so this is the *default* experience
for a native file opened in the web editor with no `brink.toml` dialect set —
not an edge case behind a non-default flag.

## 4. Options

### A — route `IdeSession`'s live analysis through its own `ProjectDb`

Replace `snapshot().analyze()` with `db.analysis()`
(`crates/internal/brink-db/src/db.rs:531`) in `IdeSession::reanalyze` /
`update_and_analyze`.

Arguments for, given what has landed since #1347 was filed:

- **The db is already in lockstep with live typing.** `update_source` writes
  the file input, #1553 (`sync_db_options`) writes the options input, and
  #1526 already treats the db as the authority for module identity. There is
  no longer a fact the pure path holds that the db lacks; the reverse is not
  true.
- **It closes the class, not the instance.** #1526, #1553, #1562 and this
  issue are four separate repairs of the same seam. Every future check wired
  at a db query inherits the fix.

Performance is **not** a settled argument for A. `db.analysis()`
(`analysis_query`, `crates/internal/brink-db/src/queries/analysis.rs:760`)
composes `resolutions_index_query` + `analysis_diagnostics_query` +
`whole_project_diagnostics_query` — all three keyed on the whole
`ProjectInput`, not per file. A keystroke invalidates the edited file's
`SourceFile` input, so all three re-run, the same as `analyze_with_modules`
does today. What salsa's `lru = 4096` per-file memo on
`per_file_diagnostics_query` preserves inside that recomputation is the
*other* files' per-file contributors — not the whole-project half
(`module_diagnostics`, `strict_diagnostics`, `ufcs_resolution_query`, …)
either path also runs on every keystroke. Whether that partial memoization
nets out cheaper than `analyze_with_modules`'s whole-project walk is not
measured here.

Costs: `IdeSnapshot` becomes vestigial (it is `pub` in `brink-ide` but has no
consumer outside `IdeSession` — `brink-web`'s `session.snapshot()` is an
unrelated runtime-state snapshot); the session's analysis stops being a value
that can be cloned out and run off-thread; `update_and_analyze` would have to
start calling `sync_db_options`, which it does not today.

### B — carry per-file `Language` into `IdeSnapshot`

The #1526 shape, applied again: add the db-derived classification to the
snapshot, thread it through `analyze_with_modules` → `finish_analysis` →
`per_file_diagnostics`, and add an `E137` call site to `finish_analysis`.

Smaller and lower-risk, and it fixes `brink-lsp` in the same stroke (which
option A does not — the LSP has its own pure-path call site). But it
institutionalizes a second check path running parallel to the db's, and it
leaves the next db-side check to be discovered the same way this one was.

### C — rule that compile-time-only is correct for these

Defensible for `E137` alone (a strict-only *project configuration* error is
arguably a compile concern). It is not defensible for the `E051` false
positive, which no ruling makes correct.

## 5. Recommendation, not a ruling

The `E051` false positive is a bug on any reading, so #1347 cannot be closed
as "compile-time-only is correct" without separately fixing the gate. Between
A and B, the evidence in §4 favours **A** on correctness grounds — the db
already holds every fact the pure path does, plus one the pure path
structurally cannot reach (a file's `Language`) — not on performance: whether
A is cheaper or more expensive than `analyze_with_modules` per keystroke is
not measured here (see §4A).

But A removes a public type's reason to exist and changes the session's
threading posture, and `IdeSession::compile`'s own doc
(`crates/internal/brink-ide/src/session.rs`, the #1385 ruling) explicitly
records that #1347 is open and that adjacent PRs must not prejudge it. That is
a maintainer call. This document and its test exist so the call can be made
against measurements instead of assumptions.
