# Brink-specific pump configuration (proven values, waves 1–3, 2026-07-11)

Fill pump.js's CONFIG from these when running the pump on this repo.

## Gates
- **Rust (default GATE)**: `cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo test --workspace && cargo test -p brink-test-harness --test oracle_snapshots`
- **TS entries (gate override)**: `wasm-pack build crates/brink-web --target web --out-dir www/pkg && pnpm install --frozen-lockfile && pnpm --filter @brink-lang/editor typecheck && pnpm --filter @brink-lang/studio typecheck && pnpm --filter @brink-lang/studio test && pnpm --filter @brink-lang/editor build`
- CACHE prefix: `export CARGO_TARGET_DIR=/tmp/pump-cargo-target-brink` — ⚠ this cache reached 53GiB and caused two ENOSPC incidents (see #533): bound it, sweep it between waves, and have the pre-flight measure it explicitly.

## House rules (RULES seed — earned, don't drop)
- Oracle ratchet is sacred on any crates PR: run oracle_snapshots, report CASES/EPISODES verbatim; 5,577 episodes must not move.
- Any PR changing behavior observable through @brink-lang/web needs a @brink-lang/web patch changeset (even crates-only PRs). Changesets name ONLY real public packages; never name external consumer projects.
- When adding a method to a raw wasm/native session type, add the parallel public wrapper method (incl. cache-invalidation calls) — internal-only levers are dead code for consumers.
- Run ALL gates as FOREGROUND blocking commands; never park the task waiting on a backgrounded run (four separate agents stalled on this).
- Never add app.register_type for #[derive(Reflect)] types; never touch .github/workflows/release.yml; VM tests must not hang (keep step limits).
- Studio work is dropped; issues below ~#300 are presumed stale and need justification.
- If an issue's own body says needs-design/deferred, DECLINE and report — do not implement architecture unilaterally (the #458 precedent).

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
