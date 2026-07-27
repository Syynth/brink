# Brink-specific pump configuration (proven values, waves 1–3, 2026-07-11)

Fill pump.js's CONFIG from these when running the pump on this repo.

## Gates
- **Rust (default GATE)**: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo test -p brink-test-harness --test oracle_snapshots`
- **TS entries (gate override)**: `wasm-pack build crates/brink-web --target web --out-dir www/pkg && wasm-pack test --node crates/brink-web && pnpm install --frozen-lockfile && pnpm --filter @brink-lang/editor typecheck && pnpm --filter @brink-lang/studio typecheck && pnpm --filter @brink-lang/studio test && pnpm --filter @brink-lang/editor build`
- **Demo lane (gate override)**: `cd demos/compound && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cd ../.. && cargo check --workspace` — oracle-free, minutes; the workspace check proves the demo stays excluded.
- ⚠ 2026-07-18: `wasm-pack test --node` joined the TS gate because a real wasm-leg bug (PR #1017) passed both cargo test (native) and vitest (mocked) — the wasm32 target was a local blind spot.
- CACHE prefix: `export CARGO_TARGET_DIR=/tmp/pump-cargo-target-brink` — ⚠ this cache reached 53GiB and caused two ENOSPC incidents (see #533): bound it, sweep it between waves, and have the pre-flight measure it explicitly.

## House rules (RULES seed — earned, don't drop)
- Oracle ratchet is sacred on any crates PR: run oracle_snapshots, report CASES/EPISODES verbatim; 5,598 episodes must not move.
- Any PR changing behavior observable through @brink-lang/web needs a @brink-lang/web patch changeset (even crates-only PRs). Changesets name ONLY real public packages; never name external consumer projects.
- When adding a method to a raw wasm/native session type, add the parallel public wrapper method (incl. cache-invalidation calls) — internal-only levers are dead code for consumers.
- Run ALL gates as FOREGROUND blocking commands; never park the task waiting on a backgrounded run (four separate agents stalled on this).
- Never add app.register_type for #[derive(Reflect)] types; never touch .github/workflows/release.yml; VM tests must not hang (keep step limits).
- Studio work is dropped; issues below ~#300 are presumed stale and need justification.
- If an issue's own body says needs-design/deferred, DECLINE and report — do not implement architecture unilaterally (the #458 precedent).
- A ruling lands in a SPEC, not only in docs/decision-log.md. The log is HISTORY (what was decided, when, why); a spec is the CURRENT normative statement. A decision-log PR that amends no spec leaves the ruling invisible to every future reader — that is how five rulings got re-derived from scratch in one week, and how the ledger audit (2026-07-27, 296 rulings) found 29 ORPHANED / 15 CONTRADICTED. Reviewers check for this explicitly; name the spec file+section.

## Merge trains
- Unique TRAIN_WT per wave (e.g. /tmp/pump-merge-train-brink-w4).
- npm "Version Packages" bot PR merges LAST — but do not starve it: if a consumer is waiting on a released fix, merge it immediately (the 0.9.1 lesson). Bot force-pushes don't trigger CI — close/reopen to kick, or admin-merge version-only PRs.

## RULES additions (waves 4–5 lessons, 2026-07-14)
- Every call-dispatch path (direct fn-value call, CallValue, divert-target variable call) must independently enforce arity/argc in gradual mode.
- When an opcode operand doesn't carry a count needed for correctness, add the operand — never derive it from arity math.
- Value-stack imbalance from mis-dispatch is silent corruption (no end-of-turn balance detector): verify new dispatch paths leave no stray values.
- Before claiming "the checker catches this per §spec" in comments/docs, read the actual check path — don't assert guarantees the implementation doesn't provide.
- When replacing verbatim-passthrough rendering with structured re-rendering, audit for trivia (comments) attached as non-field siblings — passthrough removal silently drops them.
- When fixing a scope/resolution bug on one access path, grep all sibling paths sharing the resolve function (write/assign, inc-dec, dotted access) before closing the class.
- Don't add defensive branches the caller's guard makes unreachable — dead branches mask real fallthrough.

## Disk rule (2026-07-14 incident: 68G of invisible variant caches)
- Agents must use ONLY the provided shared CARGO_TARGET_DIR or the worktree-local ./target (which dies with the worktree). NEVER mint variant /tmp cache dirs (pump-cargo-target-brink-issueN / -fuzz / -prN / verify-target-*) — they defeat boundary sweeps and accumulated 68G across three waves. If the shared cache misbehaves (stale sibling binaries, phantom errors), `cargo clean -p <crate>` it or fall back to ./target; never a third path.
- Review agents needing to run code clone into the worktree they were given or /tmp/brink-review-<pr> and DELETE it before returning.
- Boundary sweep checklist: worktrees (all completed waves) → /tmp/pump-* glob (not exact path) → /tmp/brink-* clones → shared cache if no wave imminent.

## Durable communication (decision 2026-07-18: durable-by-default)
- Pump agents post substantive outputs to GitHub as they work; workflow-internal messages orchestrate but are never the only record.
- Reviewers ALWAYS comment their verdict on the PR (approvals included, with scope gaps). Build agents comment scope-overflow on their issue. Fix agents comment applied/skipped dispositions on the PR. Merge agents comment ONLY noteworthy events (conflict resolutions, semantic fixes) — clean merges stay silent.
- Standing wave ledger: **issue #967** (`LEDGER = 967` in CONFIG) — the scope-reconcile agent appends one comment per wave (wave id, batch, landed/parked, lessons verbatim, scope assessment).
- Labels: `pump:ledger` (the ledger issue), `pump:scope` (issues filed from scope reconciliation), `pump:lesson` (graduated house-rule issues).

## Quiet-window measurement (decision 2026-07-17, night-shift rule)
- Perf baselines / BH-B numbers are CANONICAL only from a SOLO run while nothing else executes on the machine (no wave, no sibling builds). Harness code and provisional in-wave numbers may land with their slice, labeled provisional; canonical numbers are re-collected in the inter-wave gap (wave completes → boundary sweep → solo measurement agent → commit baselines → next wave) and committed to crates/bevy-brink/benches/baselines/.
- Measurement agents: gates may use the shared cache, but BENCH runs use the worktree-local target dir; record machine context (cores, thread-pool size, OS, rustc) in the baseline .md.

## Demo lane (drive-it loop, 2026-07-18)
- Drive-session findings on demos/compound are filed with the `drive-it` label and ride ordinary waves as demo-lane items: gate override = `DEMO_GATE` (fmt/clippy/test inside demos/compound + root `cargo check --workspace` proving the demo stays excluded). No oracle, no wasm build — minutes.
- Design-shaped findings do NOT go straight to the pump: ruling conversation → drive-app-plan § amendment → then wave-able (design-before-implementation, applied to the demo).
- Phase-1 migration ports are wave items (one entity per issue, friction-journal requirement in the brief, DEMO_GATE + full GATE both — ports touch bevy-brink).

## Agent-liveness rules (2026-07-18 incidents)
- NEVER end a turn waiting on a backgrounded command — the wake will not come; run gates FOREGROUND (two agents parked mid-wave on backgrounded cargo test; recovered via resume-with-corrective).
- USER INTERRUPTS KILL ALL BACKGROUND TASKS (waves, agents, watchers). After any interrupt: probe the task registry + journal mtimes, then resume waves via resumeFromRunId and relaunch agents — worktrees survive.

## Disk sweep, widened (2026-07-18: dp-review + stray-clone incidents)
- Boundary sweeps audit ANY /tmp dir over 500M (pattern-blind — glob-matching missed 4G of orphans for days) AND hunt ~/code for stray clones/target dirs (`find ~/code -maxdepth 4 -type d -name target`) — a measurement agent once left a 26G target in a stray clone at ~/code/rs/.
- The shared cache regrows ~20-45G per wave era: wipe it at EVERY boundary where no wave is imminent, not just when tight.

## Cloud sessions (Claude Code on the web) — deltas from local (2026-07-19, earned across three ENOSPC incidents, one token expiry, a merged-PR train, and a container snapshot revert that destroyed a finished unpushed wave)

Local runs use bypass-permissions; **cloud runs are approval-gated** and several
local assumptions silently fail. When the pump runs in a cloud session, override
the config above as follows:

### Durability: push after every commit — the container is not storage
- **The container can be reverted to an older snapshot without warning**,
  taking `/home/user/<repo>/.git`, every worktree, and the object store with
  it. We lost a complete, READY-FOR-GATE three-commit wave this way — its
  worktree and objects were simply gone; only its report survived (in the
  agent transcript, which lives outside the repo filesystem).
- Therefore: **every agent pushes its branch immediately after its FIRST
  commit and after every subsequent commit** (`git push -u origin
  HEAD:refs/heads/<branch>`). "Commit granularly" already applied; the push
  now rides along. Remote refs on GitHub are the only durable store.
- The coordinator pushes the moment an agent reports done if the agent
  couldn't (permission-parked pushes get landed by the coordinator, same as
  merges). A READY FOR GATE report with an unpushed branch is an
  **emergency**, not a normal state.
- Recovery when a revert does hit: remote branches + agent transcripts are
  the recovery sources. Resume the building agent with a message — it can
  re-apply its own wave from transcript knowledge far faster than a fresh
  agent; recreate its worktree from current `origin/main` first.

### Permissions & privileged operations
- Merge/push-adjacent actions inside subagents can be **parked by the
  permission layer** even where local would allow them. Design for it: train
  agents VERIFY and post durable verdicts; **the coordinator lands merges,
  arms auto-merge, and edits PR bodies itself** via GitHub MCP. A parked agent
  is recoverable state, not a failure — the durable-comms rule is what makes
  it recoverable.
- **No `gh` CLI.** GitHub MCP tools only (`mcp__github__*`, loaded via
  ToolSearch — put this in every agent prompt that touches GitHub).
- **The GitHub MCP token can expire mid-session and cannot re-auth headlessly.**
  git push/fetch keep working (separate credential path) — keep landing code,
  queue the GitHub-side ops (PR edits, comments, merges) in the scratchpad,
  notify the user to re-authorize the connector, retry on a timer.

### Disk (fixed per-session allowance, ~35–40G observed; `df` misleads)
- Shared cache is **`/home/user/<repo>/target`** — NOT a /tmp path.
  Worktrees live under **`.claude/worktrees/`** — NEVER /tmp.
- **MANDATORY on every cargo invocation, every agent**:
  `CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0` and `-j 4`.
  Full debuginfo caused two same-day ENOSPC crashes; the flags shrink
  artifacts severalfold. Put this in the prompt, not the doc (prompts are the
  only part agents obey).
- **ONE full workspace gate at a time across ALL agents** — two parallel
  gates exceeded the allowance twice. Serialize explicitly (coordinator holds
  a GO token); cargo's lock is queuing, not budget control.
- On ENOSPC: deletes still succeed. Trim order: `target/debug/incremental` →
  `target/release` + wasm target trees → deleted-worktree leftovers. An agent
  that hits ENOSPC should STOP and report, never self-clean shared state.
- **Stale-binary hazard**: the shared target can serve test binaries compiled
  from a since-DELETED sibling worktree (baked `CARGO_MANIFEST_DIR` → phantom
  insta snapshot-not-found failures). `cargo clean -p <crate>` cures; suspect
  it whenever snapshot failures appear only in shared-cache runs.

### Gates
- `wasm-pack build/test` FAIL in the sandbox at the wasm-opt/binaryen
  download (no proxy route to GitHub release assets). Cloud wasm gate =
  `cargo check -p brink-web --target wasm32-unknown-unknown`; the full
  wasm-pack legs are CI-only — say so in the PR body rather than skipping
  silently.
- Oracle/corpus gates run fine; expect the first cold build to take minutes.

### Liveness & events
- `sleep`/polling are blocked. Use send_later check-ins as the heartbeat;
  background tasks re-invoke the coordinator on completion — but a task
  killed by ENOSPC dies SILENTLY (no wake). Check-ins must probe liveness
  (process table + transcript mtime), not just wait.
- **A user interrupt kills all background tasks.** After any interrupt:
  probe, then resume (workflows via resumeFromRunId; agents via
  SendMessage-resume — worktrees survive).
- PR webhooks deliver CI failures and comments but NOT success/pushes/merge
  transitions: arm auto-merge for the happy path + a check-in for the rest.
  `rerun_failed_jobs` 403s while the run is still in progress — wait for
  run completion, then re-kick.

### Ephemerality
- The container is reclaimed after inactivity: **anything unpushed dies**.
  Push at every stable point; if a wave must pause, push its worktree state
  to a `wip/` branch first. Scratchpad artifacts worth keeping get committed
  or posted to GitHub before the session ends.
- **Always `git fetch origin` immediately before branching from
  `origin/main`.** Cloud sessions are long-lived while remote main moves
  under them (auto-merges land between your commands); branching from a
  stale ref silently resurrects old file states. (Caught in the act while
  writing this section.)
