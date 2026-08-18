# CLAUDE.md

## What we're building

A narrative language, compiler, runtime, and studio in Rust — with **two source surfaces sharing one pipeline**:

- **`.brink` — the native surface** (`brink-syntax-native`): the language the project is actually building. Modules, typed mode, conventions/prose-dialect, structs, lambdas.
- **`.ink` — the compatibility surface** (`brink-syntax`): Inkle's ink, maintained so existing stories keep working.

Both parse to their own CST, then converge: → HIR lower (`brink-ir::hir`) → analyze (`brink-analyzer`) → LIR lower (`brink-ir::lir`) → bytecode codegen (`brink-codegen-inkb`) → `StoryData` (`brink-format`) → link + execute (`brink-runtime`). Above that sits the **authoring stack**: `brink-ide`/`brink-db` (queries), `brink-web` (wasm), the TypeScript editor packages, and **brink-desktop** (Tauri).

A parallel **converter** pipeline (`.ink.json` → `brink-json` → `brink-converter` → `StoryData`) served as the known-good reference until it was retired (2026-07-11 ruling, #544). Correctness is anchored by the C# ink oracle; `.ink.json` files stay on disk only as inklecate output for oracle regeneration — nothing in-tree consumes them.

## Current state

**The project's center is the NATIVE surface and the authoring experience** (ruled 2026-08-07, `docs/decision-log.md` "Oracle conformance is no longer the core metric"): the `.brink` language (conventions, prose dialect, modules, typed mode), the editor packages (`@brink-lang/editor` / `@brink-lang/studio` / `@brink-lang/web`), and the **brink-desktop** Tauri studio (`packages/brink-desktop/`, `docs/desktop-shell-spec.md`). Progress is measured there — by what an author can do in the editor and what the native language can express.

**Ink compatibility is a maintained floor, not the goal.** The oracle ratchet (`RATCHET_EPISODE_COUNT` in `crates/internal/brink-test-harness/tests/oracle_snapshots.rs`, currently 5,608 of 6,619 C#-oracle episodes) stays CI-enforced as a **regression floor**: it must never move down, and unexpected movement in either direction is still stop-and-report. But the residual ~1,000 mismatched episodes are not the priority backlog, and "the gap" is not the measure of progress — the C# oracle only sees the ink-compat subset, which the native surface has outgrown.

The **editor acceptance gate** (`crates/brink-web/src/editor/acceptance_gate.rs`) carries the same standing as the ratchet for the editor path: one canonical native project, both analysis roads, zero diagnostics plus positive queries. Its fixture is byte-identical to the studio's `?fixture=native` — keep them in sync.

Corpus/case counts drift as cases are added — run `corpus_report` rather than trusting any snapshot written here. `tests/tier1-native/` goldens are self-referential (no C# counterpart) and separate from the ratchet; never conflate the two.

Runtime restructuring is **complete** (all 9 steps of `docs/runtime-restructuring-spec.md`). Active tracks:

- **Native surface + authoring** — conventions/prose-dialect semantics, editor features, desktop stages (D3: export, `brink-cli` sidecar, file associations).
- **`bevy-brink` integration** — the runtime as a Bevy plugin plus the external-function binding facility (ink↔engine). See `docs/bevy-brink.md`.
- **Ink-compat maintenance** — the ratchet holds; residual oracle mismatches are taken opportunistically, not as a driving backlog.

## Trust hierarchy

1. **Ink language docs** (`~/code/rs/s92-studio/reference/ink/documentation/`) — ground truth. **Maintainer-local, absent in cloud/remote sessions** — `ls` on that path fails there; there is no in-repo `reference/` and no copy elsewhere on the filesystem.
2. **Reference C# implementation** (`~/code/rs/s92-studio/reference/ink`) — for checking behavior. **Same maintainer-local path, same absence in cloud sessions.**
3. **C# ink oracle** (`tools/ink-oracle/`, `oracle/*.oracle.json`) — golden episodes from the C# runtime. In-tree, reachable everywhere.
4. **Brink compiler** — under test, not trusted. In-tree, reachable everywhere.

Ranks 1–2 are the two highest-authority sources, and they are exactly the two a cloud/remote session cannot open. When a question routes there (e.g. a parser-semantics call: does ink actually require some construct?) and you're in a cloud session, do not guess from a lower-ranked source while describing it as settled: check the in-tree corpus first — `tests/tier{1,2,3}/**/story.ink` plus the checked-in `oracle/*.oracle.json` episodes (curated, oracle-backed) **and** `tests/tests_github/` plus `tests/tests_patched/` (real-world `.ink` used for parser smoke tests and lossless roundtrip validation — see `docs/book/src/contributing/test-corpus.md` §"GitHub corpus"). If the corpus settles it, cite the file. If it doesn't, say the question is unsettled here and surface it for a maintainer ruling rather than deciding it. (Same accounting as "Cloud / fresh-environment sessions" already does for `dotnet`/the oracle generator.)

Changes to the runtime mean we will need to re-evaluate the oracle corpus and potentially regenerate. This is a major operation — surface it immediately if necessary.

## Design principles

These are standing values, not situational rules. They apply to all work on this project.

- **Design before implementation.** Discuss the design with the user before writing code for non-trivial changes. Present options and tradeoffs. The user makes architectural decisions — agents propose and implement.
- **Understand the consumer.** APIs are designed from the consumer's perspective, not the implementor's. Internal VM concepts should not leak into public types. Ask "who calls this and what do they need?" before defining an interface.
- **Separate concerns by ownership.** If two pieces of data have different lifetimes, mutability, or swap semantics, they belong in different structs. Don't bundle unrelated things for convenience — it creates borrow conflicts and unclear ownership. (Example: `Program` should be immutable; line tables are swappable — they don't belong in the same struct.)
- **Instrumentation doesn't belong in the production path.** Test harnesses and observers should wrap or compose with production types, not thread optional parameters through them. If an `if observer` branch appears in a hot loop, the abstraction boundary is wrong.
- **Defer resolution to the latest useful point.** Don't eagerly materialize data (strings, resolved content) if it will be consumed later in a context where you have more information (e.g., current locale). Store structural references and resolve at read time.
- **Guard against unbounded growth.** Any loop that accumulates data must have a limit. The VM has a step limit. `continue_maximally` has a line limit. If a new accumulation pattern appears, add a cap.
- **Correctness above all.** The goal is not to make numbers go up. A fix that makes the ratchet go down because it removes a hack that was accidentally passing tests is better than a hack that inflates the count. A correct fix to the wrong layer is worse than no fix.

## Workflows

### Native surface + authoring (the primary track)

Most work lands here. The discipline that matters:

1. **Rulings before implementation.** Language/UX semantics are the maintainer's call — check `docs/decision-log.md` for an existing ruling before designing; if the issue body asks a design question, decline and surface it rather than deciding.
2. **Both roads.** Editor behavior has two analysis paths — the db-direct road (`ProjectDb`, what the studio's Problems panel renders) and the off-db snapshot road (`IdeSnapshot::analyze`). A change can be correct on one and wrong on the other; exercise both, and remember a green `brink compile` is NOT evidence the editor agrees.
3. **The editor acceptance gate is the invariant** (`crates/brink-web/src/editor/acceptance_gate.rs`) — extend it when new behavior is ruled; never weaken it.
4. **Verify through a real consumer.** Rust-level tests over a real `EditorSession`, not a browser screenshot: the playground has silently lied before (it once never applied `brink.toml` at all, #2324).

### Ink-compat conformance (secondary — see "Current state")

When working an oracle mismatch (making failing episodes pass):

1. **Run the corpus report** to identify highest-impact categories.
2. **Sample 3–5 failing cases**, study the diagnostic dumps (`.ink` source, compiler `.inkt`, oracle episodes).
3. **Root-cause the systematic issue** — trace to the exact pipeline layer.
4. **Write failing tests** that prove the root cause before changing production code.
5. **Enter plan mode** and present the RCA + tests + fix approach. Do not implement before plan approval.
6. **Implement, test, commit.** One fix per commit.
7. **Verify with the corpus report.** Present before/after comparison.

### What NOT to do

- Do not patch symptoms. Find the root cause.
- Do not cargo-cult the reference C# implementation. Understand what it does; don't copy its structure.
- Do not assume existing compiler code is correct. Every layer was written by agents and may contain fundamental misunderstandings.
- Do not implement before plan mode for compiler fixes.

Ratchet: `RATCHET_EPISODE_COUNT` in `crates/internal/brink-test-harness/tests/oracle_snapshots.rs`.

Test cases: `tests/tier{1,2,3}/` — each has `story.ink`, `story.ink.json` (inklecate output, kept only for oracle regeneration), and `oracle/*.oracle.json` (golden episodes from the C# ink runtime).

## Runtime public API

The runtime exposes a `Step` enum as the primary output type. Only `Line`
carries a payload — the terminal variants carry none of their own (any
trailing text already arrived as its own preceding `Step::Line`;
`docs/prose-dialect-spec.md` §7, RULED):

```rust
pub enum Step {
    Line(OutputLine),      // OutputLine { text, tags, block_id, element } — more output coming
    Done,                  // turn complete (ink -> DONE); no payload
    Choices(Vec<Choice>),  // pick a choice; no text/tags of its own
    End,                   // story permanently ended (ink -> END); no payload
    Suspended,             // flow parked at an `await` site (FlowFrame model); no payload
}
```

(`Suspended` is the flow-suspension park — `docs/flow-suspension-spec.md`
§10.1. Like `Done` it is a turn boundary. The host wakes the flow via
`Story::wake_check`; a park never auto-continues. See
`brink-runtime/src/story/types.rs` for the real definition — this block is a
summary, not the source of truth.)

⚠ `OutputLine.element` (`Element { kind, data }`, PR #2109) is the carrier for
per-line classification. `kind` still reports `Element::NARRATIVE` on every
line — no claim handler classifies its own `kind` yet (#1683). As of #2108,
`data` is populated for one case: an `@[convention(..., attach = StructName)]`
handler's returned struct fields merge into `data` on every line in the run
that follows it. Do not read `kind`'s presence as "the host can distinguish a
scene heading" — that part of the field still carries no classification.

Primary consumer pattern:

```rust
loop {
    match story.continue_single()? {
        Step::Line(line) => print!("{}", line.text),
        Step::Done => {}
        Step::Choices(choices) => {
            story.choose(pick)?;
        }
        Step::End => break,
        Step::Suspended => break,
    }
}
```

`continue_maximally()` returns `Vec<Step>` — the last element is always a terminal variant (`Done`, `Choices`, or `End`).

`Step::Done` is delivered both for an explicit `-> DONE` and for a flow
that ran out of content with nothing left to run — call
`Story::did_safe_exit()` (or `FlowInstance::did_safe_exit()`) right after
receiving it to tell the two apart; `false` means the *next*
`continue_single`/`advance` call will return `RuntimeError::RanOutOfContent`
instead of more text.

`FlowInstance` adds lower-level entry points for orchestration layers (e.g. `bevy-brink`):

- `advance()` → `StepOutcome::{ Step(Step), AwaitingExternal }` — like `step_single_line` but surfaces a deferred external (`ExternalResult::Pending`) cleanly instead of erroring, so a world-access binding can pause and be resolved out-of-band. `step_single_line` is the thin wrapper that maps `AwaitingExternal` back to an error.
- `begin_function_eval` / `resume_function_eval` → `FunctionEval::{ Returned(Value), AwaitingExternal }` — evaluate an ink function from engine code without advancing the visible story (output isolated, transcript untouched), pausing/resuming on world-access externals. Plus `has_pending_external` / `pending_external_name` / `resolve_external` accessors.

## Cloud / fresh-environment sessions

On a fresh checkout (including cloud sessions with no local toolchain cache), run `scripts/setup-dev.sh` first. It installs/verifies rustup + the pinned toolchain (`rust-toolchain.toml`), `wasm-pack`, and `pnpm` (via corepack), mirroring the versions `.github/workflows/ci.yml` uses. Setting `BRINK_SETUP_FULL=1` additionally installs CI's pinned `cargo-deny` and runs both workspace audits (root and `packages/brink-desktop/src-tauri`) — opt-in because cargo-deny has no prebuilt binary and compiles from source in ~2m, which is real latency at every cloud-session start for gates CI already enforces.

**Every network step in that script is timeout-bounded** (#2531/#2584/#2591/#2638), so a stalled proxy fails fast with a named diagnostic instead of hanging a session — this requires GNU `timeout` (or macOS's `gtimeout`) to be on PATH; without either, a bound silently degrades to no protection at all (a printed warning only). Each bound is overridable by its own `BRINK_SETUP_*_TIMEOUT` env var, and **some of them abort the run rather than warning and continuing** — so if setup dies naming an env var, that is the knob to raise, not a bug. The full knob/default/**fail-vs-warn** table lives in `scripts/setup-dev.sh`'s own header block; read it there rather than guessing, since the fail-vs-warn column differs per step (the pinned-toolchain and rustup-installer fetches are fatal; the prebuilt-tarball fetches warn and fall back to a from-source build). Both halves of that arrangement are **mechanically checked, not hand-audited** (#2648/#2647/#2666/#2667/#2677/#2678) — hand enumeration of "which commands here touch the network" went 0-for-3 on completeness (#2591 missed two, #2638 missed a third, each caught only by the next round's review), a FOURTH round (#2667) showed the enumeration of *which scripts* to scan was itself incomplete (`scripts/refresh-excluded-lockfiles.sh` sat outside the scan entirely with two bare `cargo update` calls), and a FIFTH round (#2677/#2678) found the next ring outside `scripts/` itself: the justfile's recipe bodies and `benchmarks/setup.sh` held eleven more unbounded fetches between them, and the knob-table check was hardwired to `BRINK_SETUP_*` so the `BRINK_REFRESH_*` table PR #2671 added was cross-checked by nothing. `scripts/check-scripts.mjs` (run in CI by `pnpm test:scripts` via its `.test.mjs` sibling, standalone as `pnpm check:scripts`) now runs **three** checks: checks 1 and 3 scan **every shell script discovered from the repo root** (pruning dependency/build output and nested checkouts, plus `.githooks/*`) **and the justfile's recipe bodies**, translated into a line-preserving shell view so the same tokenizer covers them — not just `scripts/`. Check 2a (the knob table) is `(path, prefix)`-parameterised over a `KNOB_TABLES` registry of **four** tables (`scripts/setup-dev.sh`:`BRINK_SETUP_`, `scripts/refresh-excluded-lockfiles.sh`:`BRINK_REFRESH_`, `justfile`:`BRINK_JUST_`, `benchmarks/setup.sh`:`BRINK_BENCH_`), each cross-checked against its own prefix, plus a discovery sweep (`findUnregisteredKnobTables`) that flags any `BRINK_*_TIMEOUT` knob nobody registered; check 2b (doc-pointers) is still `setup-dev.sh` alone, since the three delegating documents are about that one script. Check 1 flags any allowlisted network-touching command not lexically wrapped in `run_with_timeout`, unless it carries a reason-bearing `# check-scripts: allow-unbounded <reason>` waiver comment on the line above it — checked both ways, a minimum reason length and a stale-waiver report when the command below it becomes bounded or moves; `just studio-dev`'s long-running dev server is the one waiver in the repo today, and it is named in the header as a hole by construction. Check 3 flags any invoked command that is classified as neither a known network binary nor a known local one, forcing an explicit network-vs-local decision for anything new (#2666). Read its header before trusting it: it is a lexical scan over hand-maintained allowlists and states exactly what it cannot see — indirect fetches above all (`pnpm --version` downloads the pinned tarball through corepack's shim on a cache miss, and is caught only because `pnpm` is in the allowlist by hand), plus heredocs (a MEASURED cost, not an assumed one: blanking every heredoc body in the two test harnesses still leaves over a hundred false fetch reports, because the harnesses' own assertion prose names the tools), `*.test.sh` stub bodies (excluded from the scan for that reason), and trailing comments. Two surfaces are deliberately **not** scanned, stated rather than silently uncovered: `.github/workflows/*.yml` `run:` blocks (a fresh shell per step that has not sourced `run_with_timeout`, and Actions already bounds every job with its own `timeout-minutes`) and `packages/*/scripts/*.mjs` (Node ESM — a category error for a shell-line tokenizer; their `execSync` calls take `execSync`'s own `timeout` option instead).

- **Oracle regeneration is NOT needed.** The C# oracle (`tools/ink-oracle/`, `oracle/*.oracle.json`) requires `dotnet` and is only for producing new golden episodes. All `oracle.json` files consumed by tests are already checked in — do not install `dotnet` or run the oracle generator just to get the test suite working.
- **Expect the first full `cargo build`/`cargo test --workspace` to take minutes.** The workspace includes the `bevy-brink` crate tree (Bevy + its dependency graph), which is the dominant cost of a cold build. Subsequent builds are incremental and much faster.
- **The wasm gate (`crates/brink-web`, `@brink-lang/web`) needs `wasm-pack`.** `scripts/setup-dev.sh` installs it; without it, `wasm-pack build crates/brink-web --target web --out-dir www/pkg` (and anything depending on that output) will fail.
- **Build the wasm package before installing, and install via `pnpm install:checked`.** `@brink-lang/web` (`packages/wasm`) has a `file:` dependency on `crates/brink-web/www/pkg`. Every CI lane sequences correctly, but a bare `pnpm install --frozen-lockfile` does NOT reliably fail loudly when the ordering is skipped locally, and it has been observed failing in **two different shapes** for the same skipped ordering: exiting 0 with the link silently unresolved (#2479), and writing **no `node_modules` at all** while the only visible symptom was a bare `vitest: not found` from the *next* command (#2593). Which shape a machine got used to depend on the pnpm 10.x corepack resolved there, because the repo pinned only the major; **#2604 pins an exact version** (root `package.json`'s `packageManager` field — the single source `scripts/setup-dev.sh` and every `pnpm/action-setup` lane derive from, enforced by `scripts/check-pnpm-pin.mjs` via `pnpm test:scripts`). That makes the shape reproducible, not harmless — **never rely on `pnpm install`'s exit code to tell you the install happened.** No pnpm lifecycle hook can cover the gap either: pnpm skips every project lifecycle script when a per-package link fails, so a root `preinstall` is dead code in exactly this state (verified by direct probe on pnpm 10.34.5 for #2593, and earlier for #2479).
  - Local/fresh-worktree flow: `wasm-pack build crates/brink-web --target web --out-dir www/pkg`, then **`pnpm install:checked -- --frozen-lockfile`** (`scripts/guarded-install.mjs`). It runs the cause check *before* spawning pnpm — refusing outright, so no half-written tree appears — and re-verifies afterwards that `node_modules` and the `brink-web` link actually materialised, exiting non-zero when they did not **even if pnpm reported success**. Note the `--`: pnpm forwards it to the script, and `pnpm install -- <arg>` reads args after it as *package names to add*, so the script strips separators and rejects bare positionals rather than silently mutating `package.json`.
  - `pnpm check:wasm-pkg` (`scripts/check-wasm-pkg.mjs`) remains the standalone diagnostic, running two independent fast checks with different remediations — `checkWasmPkg` (does `crates/brink-web/www/pkg` hold a built wasm-pack output? → build wasm) and `checkWasmPkgLink` (did it resolve into `packages/wasm/node_modules/brink-web`? #2514 → reinstall). `install:checked` composes both; run `check:wasm-pkg` directly when diagnosing an already-broken tree.
  - Most CI lanes need neither: the ordering is CI-self-enforcing (#2504) via `every_pnpm_install_lane_builds_wasm_first_in_the_same_job` (`packages/brink-desktop/src-tauri/src/lib.rs`), which parses every job in every `.github/workflows/*.yml` file and fails if any `pnpm install --frozen-lockfile` step lacks a preceding `wasm-pack build crates/brink-web` step in the same job. That test guards the lanes it can see in workflow YAML — but `.github/workflows/book.yml` runs `just book-assets`, whose recipe body invokes `pnpm install:checked` from inside a `justfile` recipe rather than a literal workflow step, so the YAML parser has no visibility into it. `book-assets` is guarded precisely because of that blind spot; `install:checked` covers both the local path the CI-self-enforcing test cannot see and this one CI lane it cannot see either.

## Which gate covers which files

Pick the gate from the files you actually touched, not from the directory's name. The mapping is not guessable, and getting it wrong ships a change whose tests never ran — the commands below are correct, but none of them tells you *which* one owns a given file.

**A file is subject to every check its kind attracts, not just the one you thought of.** Derive the list from the diff; do not enumerate from memory. Any `.rs` file in the root workspace is subject to **three independent CI gates** — `cargo test`, `cargo fmt --all -- --check`, *and* `cargo clippy --workspace --all-targets -- -D warnings` (plus a separate `--all-features` clippy pass). Clippy is the one most often forgotten, and this repo denies warnings with pedantic on, so ordinary-looking additions fail it: a doc comment naming an identifier without backticks (`clippy::doc_markdown`), or a fixture/test function crossing 100 *logical* lines (`clippy::too_many_lines`) — both of which took a PR red in #2734 while its gate's `cargo test` and `cargo fmt` passed. For large driven/fixture functions the established convention is `#[expect(clippy::too_many_lines, reason = "…")]` (see `crates/bevy-brink/src/flow.rs`), not splitting.

| You edited | The gate that covers it | Trap |
|---|---|---|
| `scripts/*.mjs` | `pnpm test:scripts` | — |
| **`packages/*/scripts/*.mjs`** | **`pnpm --filter @brink/desktop test`** | ⚠ **NOT `pnpm test:scripts`.** That script is `node --test scripts/*.test.mjs` — a non-recursive glob rooted at the repo, blind to `packages/*/scripts/`. The three files in `packages/brink-desktop/scripts/` are tested from `packages/brink-desktop/src/__tests__/*.test.ts` under vitest, nowhere near their own directory. |
| `packages/brink-desktop/src-tauri/**` | `cd packages/brink-desktop/src-tauri && cargo test` **and** `cargo fmt --check` **in that same directory** | ⚠ A root `cargo test` does **not** run it. Not via the root `exclude` list (it isn't in it) — root `members` globs only `crates/…`, so `packages/**` is never matched, and `src-tauri/Cargo.toml` declares its own `[workspace]`. Outside by construction, twice over. ⚠ **The same exclusion applies to `cargo fmt --all`** — a root `cargo fmt --all -- --check` reports clean while `src-tauri` is unformatted. `desktop-smoke.yml`'s "Format check (src-tauri)" step runs `cargo fmt` *inside* the directory and will catch it. |
| `packages/ink-editor/**` | `pnpm --filter @brink-lang/editor test` (own suite, #2559) | ⚠ Until #2559, `@brink-lang/editor` had **no test script and zero `.test.ts(x)` files of its own** — every regression here was gated only by `packages/brink-studio/src/__tests__/*` reaching in through the studio's alias map, so a *different* published package's suite was the only thing that could catch a break. `pnpm --filter @brink-lang/editor test` is now real, needs no built `crates/brink-web/www/pkg` AT TEST-RUN TIME (its `vitest.config.ts` deliberately takes no wasm alias — see that file; the workspace install still needs it, #2479), and is the gate for files under this package. The four-plus studio tests that reach `ink-editor` internals (`inline-rename.test.ts`, `binder-seed-race.test.tsx`, `code-actions-apply-reachability.test.ts`, and `inline-name-input-seed.test.ts` — plus the shared non-test fixture module `select-calls.ts` they import) were deliberately **left in place, not migrated** — a change spanning both packages (editor internals + studio wiring) still needs both suites. |
| `packages/brink-studio/**`, the `brink-web` mock | `pnpm --filter @brink-lang/studio test` | Needs the wasm pkg built first (#2464). |
| `crates/brink-web/**` | `cargo test -p brink-web --lib` + the studio suite | Fixture changes are mirrored on the TS side; the Rust test alone is half the pin. |
| `.github/workflows/*.yml` | `cd packages/brink-desktop/src-tauri && cargo test` | The workflow-YAML guards (`every_pnpm_install_lane_builds_wasm_first_in_the_same_job`, `every_workflow_job_sets_timeout_minutes`) live in the **excluded** workspace, so a root `cargo test` misses them too. |

The `cargo fmt` half of the `src-tauri` row cost its own CI failure (#2724), *after* this table was first written: a gate ending `cd packages/brink-desktop/src-tauri && cargo test && cd ../../.. && cargo fmt --all -- --check` reported green while `desktop-smoke.yml`'s "Format check (src-tauri)" step went red. The `cargo test` half of the exclusion had been applied; the `cargo fmt --all` half had not. **Workspace exclusion applies to every `--all`-style cargo command, not just the one you got burned by.**

The `packages/*/scripts/` row is here because it cost a CI failure (#2702): a change to `ensure-cli-sidecar.mjs`/`ensure-wasm.mjs` ran a gate of `test:scripts` + `src-tauri cargo test` — neither of which executes those files' tests — and a dead-code predicate (`error.killed`, which `execSync` never sets on a timeout) reached CI. **A gate scoped narrower than the diff is not a green gate.** After writing a gate, list the files the diff touches and confirm each one has a row above.

## Key commands

```sh
cargo check --workspace                          # type-check
cargo test --workspace                            # run tests
cargo clippy --workspace --all-targets -- -D warnings  # lint
cargo fmt --all -- --check                        # format check
cargo fmt --all                                   # format fix

# Corpus report — triage tool
cargo test -p brink-test-harness --test corpus_report -- --nocapture

# Oracle snapshots — primary correctness test (insta snapshots)
cargo test -p brink-test-harness --test oracle_snapshots -- --nocapture

# Single case — filter by substring
BRINK_CASE=I002 cargo test -p brink-test-harness --test oracle_snapshots -- --nocapture

# Accept snapshot changes after intentional behavioral changes
INSTA_UPDATE=always cargo test -p brink-test-harness --test oracle_snapshots

# Effect-row ground-truth harness (required for PRs touching effects)
cargo test -p brink-test-harness --test t2_ground_truth_effects --features effect-trace -- --nocapture
```

Native surface + authoring:

```sh
# Editor acceptance gate — ratchet-equivalent standing for the editor path
cargo test -p brink-web --lib acceptance_gate::

# Native goldens (self-referential; NOT part of the oracle ratchet)
cargo test -p brink-test-harness --test tier1_native
cargo test -p brink-test-harness --test tier1_native_strict   # strict-findings baseline

# Compile-road divergence guard (compile_path vs brink_environment::compile)
cargo test -p brink-test-harness --test environment_parallel_gate

# Frontend
pnpm --filter @brink-lang/editor test          # vitest — ink-editor's own suite (#2559); no wasm pkg needed
pnpm --filter @brink-lang/studio test          # vitest (the largest suite; also needs the wasm pkg built, #2464)
pnpm --filter @brink/desktop test              # desktop shell (needs the wasm pkg built)
wasm-pack build crates/brink-web --target web --out-dir www/pkg   # rebuild wasm the frontend serves

# Desktop (packages/brink-desktop — src-tauri is workspace-EXCLUDED, run its gates directly)
pnpm --filter @brink/desktop dev                # preflights wasm freshness, then vite
# Runs on a fresh checkout/worktree: `binaries/brink-cli-<triple>` is gitignored, so
# src-tauri's build.rs stages a STUB sidecar for debug builds (#2617) via the same
# `scripts/ensure-cli-sidecar.mjs` + BRINK_SIDECAR_STUB that desktop-smoke.yml uses.
# Nothing here executes it; `pnpm --filter @brink/desktop build` stages the real binary.
cd packages/brink-desktop/src-tauri && cargo test
```

## Crate layout

| Crate | Path | Purpose |
|-------|------|---------|
| `brink-compiler` | `crates/brink-compiler/` | Pipeline driver |
| `brink-runtime` | `crates/brink-runtime/` | Bytecode VM |
| `brink-syntax` | `crates/internal/brink-syntax/` | Lexer, parser, CST, AST |
| `brink-ir` | `crates/internal/brink-ir/` | HIR, LIR, symbol tables, lowering |
| `brink-analyzer` | `crates/internal/brink-analyzer/` | Cross-file semantic analysis |
| `brink-codegen-inkb` | `crates/internal/brink-codegen-inkb/` | Bytecode codegen: LIR → StoryData |
| `brink-format` | `crates/internal/brink-format/` | Binary interface between compiler and runtime |
| `brink-test-harness` | `crates/internal/brink-test-harness/` | Episode exploration, diffing, corpus tests |
| `bevy-brink` | `crates/bevy-brink/` | Bevy 0.19 integration: plugin, assets, components, external-function bindings |
| `bevy-brink-derive` | `crates/internal/bevy-brink-derive/` | `#[derive(BrinkCommand)]` proc-macro for ink→engine command events |

**Native surface + authoring** (the crates the current work lives in):

| Crate | Path | Purpose |
|-------|------|---------|
| `brink-syntax-native` | `crates/internal/brink-syntax-native/` | **The `.brink` lexer/parser/CST** — the native surface |
| `brink-db` | `crates/internal/brink-db/` | Salsa query graph: the db-direct analysis road |
| `brink-ide` | `crates/internal/brink-ide/` | IDE queries (hover, completion, symbols, folding, semantic tokens, explain-match) |
| `brink-web` | `crates/brink-web/` | wasm bindings — `EditorSession`; hosts the **editor acceptance gate** |
| `brink-lsp` | `crates/brink-lsp/` | Language server |
| `brink-cli` | `crates/brink-cli/` | `brink` binary: compile, play, ide, fmt, xliff/locale |
| `brink-driver` | `crates/internal/brink-driver/` | Pipeline orchestration |
| `brink-environment` | `crates/internal/brink-environment/` | The determinism boundary; `Project::load`, the mounted `std/` |
| `brink-project-config` | `crates/internal/brink-project-config/` | `brink.toml` schema (`[project] entry`/`conventions`, `[lints]`) |
| `brink-source-tree` | `crates/internal/brink-source-tree/` | Source-tree abstraction |
| `brink-fmt` | `crates/internal/brink-fmt/` | Formatter |
| `brink-respell` | `crates/internal/brink-respell/` | ink → `.brink` re-emitter |
| `brink-intl` / `xliff2` | `crates/internal/` | Localization: line tables, XLIFF |

**TypeScript packages** (`packages/`, pnpm workspace):

| Package | Published as | Purpose |
|---------|--------------|---------|
| `ink-editor` | `@brink-lang/editor` | CM6 editor, `ProjectSession`, `FileProvider`, `OverlayPersistence` |
| `brink-studio` | `@brink-lang/studio` | `mountStudio` — the embeddable studio + playground (`?fixture=native`) |
| `wasm` | `@brink-lang/web` | TS wrapper over the wasm bindings |
| `wasm-types` | *(private)* | Hand-maintained TS mirrors of Rust wire shapes |
| `studio-shell` / `studio-ui` / `studio-store` | *(private)* | Shell regions + commands, components, state |
| `ink-operations` | *(private)* | Structural editing ops |
| `brink-desktop` | *(private)* | **Tauri desktop studio** — `src-tauri` is its OWN cargo workspace, deliberately excluded from the root one (`docs/desktop-shell-spec.md`) |

Per-area specs live in `docs/` (`compiler-spec.md`, `runtime-spec.md`, `format-spec.md`, `bevy-brink.md`, `intl-spec.md`, …).

## Rules

- **Flag silent data drops.** If a lowering pass silently drops data without a diagnostic, flag it immediately. Silent drops are always bugs until proven otherwise.
- **VM tests must not hang.** The runtime VM can infinite-loop on malformed bytecode. All VM tests and episode exploration use step limits. If a test hangs, it's a bug — do not increase timeouts, fix the root cause.
- **Dependencies** use `dep.workspace = true`. Versions in root `Cargo.toml`.
- **Lints:** `unsafe_code`, `unwrap_used`, `expect_used`, `panic`, `todo`, `print_stdout`, `print_stderr` are denied. Clippy pedantic is on. `clippy.toml` exempts tests for `unwrap_used`/`expect_used`/`dbg_macro`/`print_*` — but **`panic` has NO test carve-out**. A `panic!` or `unwrap_or_else(|| panic!(…))` in a test *helper* fails the required Static-checks gate (it cost two CI failures on 2026-08-07 alone). Use `assert!(cond, "msg {var:?}")` then `.expect("just asserted above")`.
- **Determinism matters.** Never iterate `HashMap` keys/values where order affects output. Sort or use `BTreeMap`. We've been burned by this — see analyzer label lookup, db file ordering, the retired converter's list items.
- **Commit after every fix.** Do not accumulate changes. Each fix is one commit. This makes bisecting easy and keeps the history clean.
- **Wasm-observable behavior needs a changeset.** Any PR (crates-only included) changing behavior observable through `@brink-lang/web` carries a `@brink-lang/web` patch changeset (decision 2026-07-11).
- **Never use `.ink.json` in the translation workflow.** The translation pipeline is `.ink` → compile → `.inkb` → export-xliff → `.xlf`. The `.ink.json` files are inklecate output kept only for oracle regeneration. They must never be used as input to `export-xliff`, `compile-locale`, or any other intl operation.
- **Never share TypeScript test fixtures by importing a `.test.ts`/`.test.tsx` file.** Vitest re-registers the imported file's `describe`/`it` blocks in the importer — measured in PR #2510: six tests ran twice (22 where 16 expected), silently passing under the enrolment suite's name. Extract shared fixtures/registries into a plain module (no `.test.` suffix) instead. Guarded by `packages/brink-studio/src/__tests__/no-test-file-imports.test.ts` (#2516), which scans every `packages/*/src` file — including `__tests__/` — for a static `from "..."`, dynamic `import("...")`, or `vi.importActual("...")` naming a `*.test.ts(x)` sibling. Mirrored in `.claude/skills/autonomous-pump/BRINK-CONFIG.md` "House rules".
- **A reproduced grammar quote must not claim `INLINE_WS+` (one-or-more) as current behavior.** `brink-syntax`'s parser has exactly one whitespace primitive, `Parser::skip_ws`, and it always matches zero-or-more — so any grammar-production quote reproducing `INLINE_WS+` present-tense is a lie about what the parser does, unless the surrounding prose (within `CONTEXT_WINDOW_LINES`, currently 8) carries one of `HISTORICAL_MARKER_RE`'s marker phrases (`now says`, `old, wrong`, `pre-#<issue>`/`predated #<issue>`, `used to`, `fixed separately`, `mismatch was fixed`, `no longer`) acknowledging the quote is superseded. Guarded repo-wide (docs, crates, packages — not just `brink-syntax`, since the drift travels wherever the grammar gets quoted) by `scripts/check-grammar-drift.mjs` (`scripts/check-grammar-drift.test.mjs`, run by `pnpm test:scripts` / CI's `frontend` job; standalone via `pnpm check:grammar-drift`). Mirrored in `.claude/skills/autonomous-pump/BRINK-CONFIG.md` "House rules".
- **No literal NUL byte in any `packages/*/src` file; use `JSON.stringify([...])` for a composite in-memory cache key.** A literal NUL byte as a key separator (e.g. `` `${a}\x00${b}` ``) makes `file` classify the file as `data` and makes `grep`/`rg` (without `-a`) classify it binary — a repo-wide text sweep silently skips it, hiding the defect (and every other match in the file) from hand review by construction (#2558, #2733, #2737). `JSON.stringify` of a fixed-arity array is an injective, collision-free encoding for a composite key. Guarded repo-wide over every `packages/*/src` file (no extension filter) by `scripts/check-no-nul-bytes.mjs` (`scripts/check-no-nul-bytes.test.mjs`, run by `pnpm test:scripts` / CI's `frontend` job; standalone via `pnpm check:no-nul-bytes`). Mirrored in `.claude/skills/autonomous-pump/BRINK-CONFIG.md` "House rules".
- **A PR touching `packages/{brink-studio,studio-ui,studio-shell,ink-operations,studio-store}/src` (excluding test-only files) must carry a changeset naming `@brink-lang/studio`.** Four of these five — `studio-ui`, `studio-shell`, `ink-operations`, `studio-store` — are `private: true`; `docs/publishing.md` records them as bundled into the published `@brink-lang/studio`, and `.changeset/config.json` sets `privatePackages.version: false` — so `@brink-lang/studio` is the only attribution route a change to any of them has. The fifth, `brink-studio`, is not private at all — it IS the published `@brink-lang/studio` package. "Not wasm-observable, so no changeset" is the wrong rule here (that one is about `@brink-lang/web`) — it has already been reached for, and been wrong, three times (PR #2787, #2798, #2817; issue #2820). A path under `__tests__/`, `__mocks__/`, `__fixtures__/`, or ending `.test.ts(x)`/`.spec.ts(x)` is exempt (test-support changes are not published behavior). The same guard also covers non-`src` bundle-shaping files, by a named allowlist (#2834): `packages/brink-studio/{package.json,tsup.config.ts,vite.config.ts,vite.config.embed.ts,alias-map.ts,index.html}` and `packages/studio-shell/package.json` — deliberately not a blanket `packages/brink-studio/**`, since that would demand a changeset for lockfile-driven version bumps and devDependencies edits that ship nothing; the two `package.json` files there each carve out a different, per-file key set — `packages/brink-studio/package.json` exempts only `version` (its `devDependencies` IS the bundle manifest: every bundled `@brink/*` private plus CodeMirror/zustand lives there, so a bump there is bundle-shaping), while `packages/studio-shell/package.json` exempts `version` and `devDependencies` (it has no `devDependencies` at all; its real deps live in `dependencies`, never carved out). Editing an EXISTING `.changeset/*.md` to add the `@brink-lang/studio` key satisfies the guard exactly like adding a new one (#2834 — the guard checks any changeset file with git status `A` or `M`, not additions only). Guarded by `scripts/check-studio-changeset.mjs` (unit-tested via `pnpm test:scripts`; the real diff check runs as its own `studio-changeset-guard` CI job on `pull_request` events, since it needs no wasm build or install). `packages/ink-editor` is NOT in this list — it publishes its own `@brink-lang/editor`.
- **Before trusting a locally measured corpus/bucket number, check for a shared-`CARGO_TARGET_DIR` collision with another live git worktree.** Cargo's `-C metadata` for a workspace member is derived from package name/version/features, not the absolute source path, so two live worktrees of the same package sharing a `CARGO_TARGET_DIR` write the same fingerprint and output path — whichever rebuilds last silently overwrites the other's artifact, or serves a different worktree's currently-different source under the same cache key (issue #2054; this is a local-measurement-only hazard, CI's isolated cache is unaffected). `scripts/check-target-freshness.mjs` (params: `--target-dir`, `--repo-root`, repeatable `--package`; defaults to `CARGO_TARGET_DIR`/cwd and the `corpus_report`/`full_corpus_sweep` dependency chain) detects a REAL collision and exits non-zero. Each tracked package (`brink-respell`, `brink-ir`, `brink-syntax-native`, `brink-runtime`, `brink-test-harness`) carries a `build.rs` (#2759) that writes its own worktree-specific `CARGO_MANIFEST_DIR` into a stamp file under `OUT_DIR` whenever cargo actually re-runs the build script for it (no `rerun-if-changed` is emitted — but cargo's real default without one is to rerun only when a file in the package changed, NOT on every invocation; the guarantee the design relies on is narrower and still holds: the stamp names the last worktree cargo actually re-ran the script for, and a no-op repeat build in that same worktree leaves it naming that same worktree, which is still correct) — the script compares that stamp against `git worktree list` and only goes red when a package's own stamp names a *different, still-live* worktree as its last builder. A missing/unreadable stamp, or one naming a worktree that's since been removed, is reported as safe/unverified rather than red — PR #2753's original version treated "a live sibling exists and *some* artifact exists" alone as the verdict, which held almost constantly in the pump's normal shared-cache mode and made the check a near-constant false positive; do not reintroduce that shape. Run standalone via `pnpm check:target-freshness`; `corpus_report` and `full_corpus_sweep` also print a pointer to it whenever `CARGO_TARGET_DIR` is set. Mirrored in `.claude/skills/autonomous-pump/BRINK-CONFIG.md`'s Cloud-sessions Disk section.
