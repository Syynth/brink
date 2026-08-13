# Brink-specific pump configuration (proven values, waves 1–3, 2026-07-11)

Fill pump.js's CONFIG from these when running the pump on this repo.

## Gates
- **Rust (default GATE)**: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo test -p brink-test-harness --test oracle_snapshots`
  - ⚠ **`cargo nextest run`, not `cargo test`** (measured 2026-07-28, issue #1695): identical results (6590 passed, 0 failed under both) at **35s vs 2m52s**. `cargo test` averaged **~56% CPU** — effectively running the 183 test binaries one at a time — while nextest averaged **~507%**, i.e. the process pool actually working. This is the per-round cost that multiplies across every fix cycle.
  - **Doctests are deliberately NOT in the per-round gate.** nextest does not run them, and `cargo test --workspace --doc` costs **101s to execute exactly ONE real doctest** (21 others are `ignore`d). That belongs on a pre-merge/CI gate, not on every agent iteration. If you need it: `cargo test --workspace --doc`.
  - Requires `cargo install cargo-nextest --locked` once (~2m07s). Already installed on the local machine.
- **TS entries (gate override)**: `wasm-pack build crates/brink-web --target web --out-dir www/pkg && wasm-pack test --node crates/brink-web && pnpm install --frozen-lockfile && pnpm --filter @brink-lang/editor typecheck && pnpm --filter @brink-lang/studio typecheck && pnpm --filter @brink-lang/studio test && pnpm --filter @brink-lang/editor build`
- **Demo lane (gate override)**: `cd demos/compound && cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test && cd ../.. && cargo check --workspace` — oracle-free, minutes; the workspace check proves the demo stays excluded.
- ⚠ 2026-07-18: `wasm-pack test --node` joined the TS gate because a real wasm-leg bug (PR #1017) passed both cargo test (native) and vitest (mocked) — the wasm32 target was a local blind spot.
- CACHE prefix: `export CARGO_TARGET_DIR=/tmp/pump-cargo-target-brink` — ⚠ this cache reached 53GiB and caused two ENOSPC incidents (see #533): bound it, sweep it between waves, and have the pre-flight measure it explicitly.

## House rules (RULES seed — earned, don't drop)
- Oracle ratchet is sacred on any crates PR: run oracle_snapshots, report CASES/EPISODES verbatim; 5,608 episodes must not move. (⚠ This number is `RATCHET_EPISODE_COUNT` in `crates/internal/brink-test-harness/tests/oracle_snapshots.rs` — read it there, don't trust this line. It sat stale at 5,598 from ~w110 to w147.)
- Any PR changing behavior observable through @brink-lang/web needs a @brink-lang/web patch changeset (even crates-only PRs). Changesets name ONLY real public packages; never name external consumer projects.
- When adding a method to a raw wasm/native session type, add the parallel public wrapper method (incl. cache-invalidation calls) — internal-only levers are dead code for consumers.
- Run ALL gates as FOREGROUND blocking commands; never park the task waiting on a backgrounded run (four separate agents stalled on this).
- Never add app.register_type for #[derive(Reflect)] types; never touch .github/workflows/release.yml; VM tests must not hang (keep step limits).
- Studio work is dropped; issues below ~#300 are presumed stale and need justification.
- If an issue's own body says needs-design/deferred, DECLINE and report — do not implement architecture unilaterally (the #458 precedent).
- A ruling lands in a SPEC, not only in docs/decision-log.md. The log is HISTORY (what was decided, when, why); a spec is the CURRENT normative statement. A decision-log PR that amends no spec leaves the ruling invisible to every future reader — that is how five rulings got re-derived from scratch in one week, and how the ledger audit (2026-07-27, 296 rulings) found 29 ORPHANED / 15 CONTRADICTED. Reviewers check for this explicitly; name the spec file+section.

## Merge trains
- **Landing policy (RULED 2026-08-13, after w148): agents ARM AUTO-MERGE; they never merge directly.** Two failures in one wave forced this. A fix agent called `merge_pull_request` on #2412 while the full CI run (Static checks, Test, E2E) was still IN PROGRESS — it landed green by luck, since a local gate is not the repo's required checks. Its sibling was then BLOCKED outright by the safety classifier, which read an agent self-merging its own review fixes as lacking sign-off, stranding finished work. Arming auto-merge fixes both at once: GitHub lands only on green required checks, and no agent issues a merge.
  - ⚠ **This re-introduces a known loss mode.** "Armed" is NOT "landed" — #1659 and #1666 were both lost to auto-merge that armed, later conflicted, and left work *looking* landed while its issue stayed open. So a train agent's `merged: true` now means "armed or landed", and **the retro MUST verify each PR's real state** rather than trusting the wave's own table. That verification is the only thing holding the hole shut.
  - **CORRECTED same day, after w149 (user-ruled): landing is THREE cases, not two.** The first draft said "never merge directly; park if arming fails" — but GitHub REFUSES to arm auto-merge once a PR's checks have completed green ("Auto-merge only applies when checks are pending"), so the *better-behaved* a PR was, the more likely it stranded itself. PR #2420 parked exactly that way carrying an approving review with zero findings and a 957-test green gate. The policy existed to stop merges that OUTRAN CI; it was instead blocking merges that had WAITED for it.
    1. Checks pending/running → **arm auto-merge**, never merge directly.
    2. All required checks completed green **and** the review verdict was `approve` (or `changes` with every finding applied and the gate re-run) → **merge directly**; arming is impossible in this state and parking only strands finished work.
    3. A red check, a `reject` verdict, conflicts, or arming failing for any *other* reason (auto-merge disabled on the repo) → **park it**, `merged:false`, reason in `detail`. Never merge to clear a red check or an unresolved review.
  - Both things the policy actually protects still hold: never merge while checks are pending, never merge unreviewed work.
- Unique TRAIN_WT per wave (e.g. /tmp/pump-merge-train-brink-w4).
- npm "Version Packages" bot PR merges LAST — but do not starve it: if a consumer is waiting on a released fix, merge it immediately (the 0.9.1 lesson). Bot force-pushes don't trigger CI — close/reopen to kick, or admin-merge version-only PRs.

## Recurring build-quality rules (the lessons loop — appended by the Lessons phase)

> **Why this section exists.** Measured over 671 pump agents: **build 44.2% of tokens, review 22.1%, fix 18.3%** — and there were **133 fix agents against 165 reviews, i.e. ~80% of reviewed PRs needed a fix cycle**. Each cycle is a full re-read, re-gate and re-push. The findings that trigger them recur, so every rule here that lands is spend removed from the 18.3%. The Lessons phase appends here automatically; a human still reviews the PR.

- **A regression test must FAIL without the fix.** Revert the production diff and watch it go red before you commit it. A test that passes on both commits proves nothing about the change it claims to guard.
- **Never state a number, `file:line`, symbol, or PR/issue attribution you did not just read at the ref you are citing.** A PR was rejected for claiming "ratchet verified at commit X = 5577" when that ref read 5607, with every coordinate in its audit wrong. Unverified claims are worse than omissions because the next agent trusts them.
- **When you add a field or variant to an existing type, grep EVERY consumer** — especially heap-size/size-estimator accumulators and serialization sites. Exhaustive-match guards do NOT catch a consumer that silently ignores a new field.
- **When you add a guard to one function, check its structurally parallel siblings.** A guard on `rename` but not `prepare_rename`, or on one arm of a matched pair, silently breaks the pair.
- **Before citing a caller, spec section, or "this is handled downstream" in a doc comment, verify it exists and does that.** Several PRs shipped confidently false claims about their own seams.
- **Assert the value a CONSUMER actually receives**, not an internal enum an intermediate layer holds. A test asserting the wrong layer passes while the user-visible behavior is broken.
- **Cross-check sibling unreleased changesets for claims your change invalidates.** Contradictory unreleased release notes are worse than none.
- **A ruling lands in a SPEC, not only in docs/decision-log.md.** The log is history; a spec is the current normative statement. A ruling that lives only in the log is invisible to everyone reading the specs — that is how five rulings got re-derived from scratch in one week.
- **Commit AND PUSH before running the long gate.** This rule existed and was violated anyway, costing work three times; the pump deletes worktrees between waves, so an unpushed branch is one prune from gone.
- **When adding a CI job for a workspace-excluded crate with a committed lockfile, pass `--locked` to its cargo check/clippy/test invocations.** Without it, a stale lock is silently refreshed in-job and the run still exits green — the exact drift class the gate exists to catch, and the repo already fences this for its other excluded lockfiles.
- **Any new CI job running heavy builds (wasm-pack, release binaries, a full dependency graph) needs an explicit `timeout-minutes` backstop**, especially in a lane branch protection doesn't watch — a hang there burns the multi-hour default unnoticed.
- **When code arms a flag or reads/writes shared state around an awaited async call, guard both sides of the await.** Don't unconditionally clear state after the await completes without checking it wasn't changed while the await was in flight, and disarm any pre-await marker if the awaited call rejects — otherwise a race during the await silently drops data or permanently suppresses a later legitimate event.
- **When an implementation change alters behavior a spec's contract table documents** (what triggers a suppression, what the source of truth is), edit that table directly. A code comment or PR description is not the normative home for a contract the spec owns, and leaves the spec silently wrong for the next reader.
- **When an event source coalesces multiple operations on the same key into one flush** (a debounced file watcher, a batched queue), a consumer that arms one "self-originated" marker per operation must clear ALL other markers for that key whenever one is armed or consumed — not just the marker it's currently setting. Otherwise two same-key operations landing in one coalescing window leave a stale marker armed, and it silently swallows the next genuine external event.
- **When a PR's own description says a stale claim exists in multiple places, grep the whole repo for every copy before closing it** — fixing the copy in the diff's own file while sibling copies (a spec doc, a JSDoc comment, a contract table) still assert the old claim leaves the host-facing docs self-contradictory.

## RULES additions (waves 4–5 lessons, 2026-07-14)
- Every call-dispatch path (direct fn-value call, CallValue, divert-target variable call) must independently enforce arity/argc in gradual mode.
- When an opcode operand doesn't carry a count needed for correctness, add the operand — never derive it from arity math.
- Value-stack imbalance from mis-dispatch is silent corruption (no end-of-turn balance detector): verify new dispatch paths leave no stray values.
- Before claiming "the checker catches this per §spec" in comments/docs, read the actual check path — don't assert guarantees the implementation doesn't provide.
- When replacing verbatim-passthrough rendering with structured re-rendering, audit for trivia (comments) attached as non-field siblings — passthrough removal silently drops them.
- When fixing a scope/resolution bug on one access path, grep all sibling paths sharing the resolve function (write/assign, inc-dec, dotted access) before closing the class.
- Don't add defensive branches the caller's guard makes unreachable — dead branches mask real fallthrough.

## Disk rule (2026-07-14 incident: 68G of invisible variant caches)

### 2026-07-28 incident: disk hit **0 bytes**, wedging the host mid-wave

Six concurrent agents (two opus, all touching analyzer/IR) exhausted **49G in a single wave**. The shell then could not run *at all* — the Bash tool failed writing its own output file — so the session could not even clean up after itself; a human had to run `rm -rf` by hand.

**The leak was structural, not bad luck.** The between-waves reset only ever cleared the SHARED `CARGO_TARGET_DIR`. But when the shared target gets contaminated (a known, recurring failure — see the cross-contamination notes), agents correctly switch to a PRIVATE target dir — and **nothing ever cleans those up**. They accumulate in `/tmp/brink*` and in the session scratchpad, invisible until the disk is gone. Post-incident sweep found **26G inside the session scratchpad alone** (one private target at 15G) plus ~190 abandoned `/tmp/brink-*` review/build dirs going back ~30 waves.

**Between EVERY wave, sweep all three, not just the shared target:**
```
rm -rf "$SHARED_TARGET" /tmp/pump-merge-train-brink-*
for d in /tmp/brink* /tmp/pr* /tmp/rev*; do rm -rf "$d"; done   # stale review/build dirs
for d in "$SCRATCHPAD"/*/; do rm -rf "$d"; done                  # agent private targets
git -C <repo> worktree prune
```

**Before launching, assert headroom and cap concurrency:**
- Require **≥60G free** before a wave starts; below that, sweep first and re-check rather than launching hopefully.
- A build-heavy wave (analyzer/IR/runtime) is **≤4 items with at most one opus build**. Six concurrent builds against one shared target is what produced this.

**⚠ Audit before deleting anything with a `.git` in it** (standing rule: never destroy work without a push-state audit). Check whether HEAD is contained in any remote branch — reviewer clones of PR heads look "unpushed" but are merged work, while a genuinely orphaned commit must be pushed to a branch before its directory is removed.
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
- ⚠ **CORRECTED 2026-08-13.** The previous rule here said `wasm-pack
  build/test` FAIL in the sandbox because there is "no proxy route to GitHub
  release assets", and degraded the cloud wasm gate to `cargo check -p
  brink-web --target wasm32-unknown-unknown`. **The diagnosis was wrong.**
  There IS a proxy route — `curl` fetches the 91MB binaryen asset fine. The
  actual cause is narrower: wasm-pack's *internal* downloader honors neither
  `HTTPS_PROXY` nor the custom CA bundle, and binaryen/wasm-opt is the ONLY
  thing it fetches that way (crates.io and wasm-bindgen-cli both succeed).
  Pre-seed `wasm-opt` on PATH — `scripts/setup-dev.sh` now does — and
  wasm-pack logs `found wasm-opt at …`, skips its own download, and the FULL
  gate passes (verified end-to-end: `wasm-pack build crates/brink-web
  --target web --out-dir www/pkg` → "Done in 3m 19s").
- So the cloud wasm gate is the REAL one, not a degraded check. Run
  `scripts/setup-dev.sh` first; don't claim the wasm legs are CI-only.
- ⚠ `cargo nextest` is NOT preinstalled in a cloud container and
  `setup-dev.sh` did not install it until 2026-08-13. The pump's GATE is
  `cargo nextest run --workspace`; run `scripts/setup-dev.sh` before the
  first wave or every agent's gate command is missing.
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

## Keeping the tracker honest (2026-07-29 — six stale-issue incidents in one month)

Six issues in July had tracker state that lied about the code: #1592 and #1667
were already fully implemented; #1449 was recorded DONE having delivered half;
#1211 and #1213 presented as open north-star design questions a week after a
ruling settled both; companion modules got re-derived from scratch against an
existing ruling. Each one burned a build agent to rediscover.

**Every one was a WRITE-side failure.** The instinct is to add a read-side
check ("verify the premise before building"), and that check is worth having —
but it treats the symptom. The tracker is unreliable because we are not writing
to it, and a read-side check leaves the next reader to pay the same cost again.

- **A `Part of #N` owes a tracked remainder.** House rule 19e correctly stopped
  agents writing false `Closes`. But an honest `Part of` with nothing tracking
  the undelivered half reads *identically to "not started"* — that is exactly
  how #1592, #1679 and #1449 went stale. So: in the same session, either file
  the follow-up issue or name the existing one that covers the remainder, AND
  comment on #N stating what is left and what blocks it. Honest partial
  delivery is fine; **untracked** partial delivery is not.
- **A PR or ruling that supersedes an issue's premise updates that issue in the
  same PR.** This is the tracker-facing twin of "a ruling lands in a spec, not
  only in the decision log." #1211/#1213 had their premise superseded on
  2026-07-21 by a ruling that updated `block-effect-model.md` §11 — and nothing
  pointed back at the issues, so the question got re-opened twice.
- **Verify the merge actually completed.** Auto-merge armed on a PR that later
  conflicts leaves the work *looking* landed while its issue stays open. #1659
  and #1666 were both lost this way and surfaced only by an audit. Poll until
  the PR reports MERGED, or say plainly that it did not.
- **If you find a premise already false, fix the tracker** — comment naming the
  delivering PR, and close the issue if nothing remains. Reporting it only to
  the pump means the next wave rediscovers it.

**These rules are per-agent discipline, and per-agent discipline gets violated.**
So the structural fix is that **scope reconciliation owns this** — it runs once
per wave, terminal, with every item's issue/PR/merge state in hand and `gh`
access already. It was only ever pointed *forward* ("did building reveal work
the plan didn't capture?"), which is why six write-side failures walked past a
phase built to catch exactly them. `pump.js`'s reconciliation prompt now has two
parts: **Part 1 reconciles the record backward and ACTS**, Part 2 is the
existing forward-looking scope proposal.

The distinction that makes acting safe: **proposing new scope or milestone
structure is a human call; recording what already happened is bookkeeping.**
Closing an issue whose PR merged, or commenting the remainder on a `Part of`,
does not restructure the plan — it stops the plan from lying. Reconciliation
returns `trackerActions` (what it changed) alongside its proposal.

The read-side premise check stays in the build prompt as a backstop. It is a
backstop, not the fix.
