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
- ⚠ **EVERY TS-touching gate MUST include `typecheck`, not just `test`** (2026-08-14, PR #2437). **Vitest strips types; it does not check them.** A wave gate of `pnpm --filter @brink/desktop test && pnpm --filter @brink-lang/studio test` passed a test file that `tsc --noEmit` rejected with `TS2339: Property 'mock' does not exist on type '(commandId: string, …) => boolean'` — `stubApi`'s `: QuitSaveApi` return annotation widened its `vi.fn()`s back to the plain interface, so `.mock` existed at runtime but not to the compiler. Only `desktop-smoke.yml`'s typecheck step caught it, and that lane is deliberately NON-REQUIRED, so it would have landed on main. Append `&& pnpm --filter <pkg> typecheck` for every TS package a batch entry touches — including `@brink/desktop`, which is easy to forget because it is private and takes no changeset.

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
- **Two PRs green individually can land a red `main` — nothing evaluates the merge result.** (2026-08-16, issue #2600, repaired by #2599.) #2583 and #2585 each added a `resolve_code_action_doc` method to `EditorSession` in the studio's `brink-web` mock, at lines 400 apart. Git merged both cleanly — distinct regions, no textual conflict — and each PR's CI had been green against a `main` that did not yet contain the other. The merged result was red on `TS2393 Duplicate function implementation`, taking `main` and **every open PR** with it, including docs-only ones. Worse than a build error: JS class semantics make the later definition win silently, so the faithful implementation became dead code and the mock's doc-handle op could never succeed.
  - **The orchestrator's part: file ownership must be re-checked when a review round widens a diff.** w164 assigned that mock to #2577 exclusively; #2578 added a method to it anyway, during a fix cycle. Ownership was stated at wave planning and never re-verified against the final diff. **Before landing, diff each PR against its stated ownership** — `git diff --name-only origin/main origin/auto/issue-N` versus the batch file's FILE OWNERSHIP block — and stop on any file claimed by a sibling in the same wave.
  - **Landing order is not free when two PRs in a wave touch the same file.** Land the first, then bring the second level with `main` and let CI re-run before landing it.
  - Whether to enforce this structurally (branch protection's "require branches up to date", or a merge queue) is a **maintainer call** — it trades CI minutes against a rare-but-total outage. Surfaced in #2600; do not decide it.
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
- **A green `vitest` run is NOT a typecheck.** If your change touches TypeScript, run `tsc --noEmit` for every package you touched even when the wave's gate string omits it — the gate is a floor, not a ceiling. PR #2438 did this unprompted ("not in the required gate but were run because this PR changes a published interface") and was clean; its sibling PR #2437 did not and shipped a `TS2339` that only the non-required desktop-smoke lane caught.
- **Verify an issue is still LIVE before building it.** Read the current source at `origin/main` and confirm the defect still exists; issues go stale silently when a sibling PR fixes them. #2291 was queued into two consecutive waves before a build agent's premise check found all three surfaces it named had been routed correctly a week earlier by PRs #2358/#2373. Report a false premise and STOP — that is a successful outcome, not a failed build.
- **When adding a CI job for a workspace-excluded crate with a committed lockfile, pass `--locked` to its cargo check/clippy/test invocations.** Without it, a stale lock is silently refreshed in-job and the run still exits green — the exact drift class the gate exists to catch, and the repo already fences this for its other excluded lockfiles.
- **Any new CI job running heavy builds (wasm-pack, release binaries, a full dependency graph) needs an explicit `timeout-minutes` backstop**, especially in a lane branch protection doesn't watch — a hang there burns the multi-hour default unnoticed.
- **When code arms a flag or reads/writes shared state around an awaited async call, guard both sides of the await.** Don't unconditionally clear state after the await completes without checking it wasn't changed while the await was in flight, and disarm any pre-await marker if the awaited call rejects — otherwise a race during the await silently drops data or permanently suppresses a later legitimate event.
- **When an implementation change alters behavior a spec's contract table documents** (what triggers a suppression, what the source of truth is), edit that table directly. A code comment or PR description is not the normative home for a contract the spec owns, and leaves the spec silently wrong for the next reader.
- **When an event source coalesces multiple operations on the same key into one flush** (a debounced file watcher, a batched queue), a consumer that arms one "self-originated" marker per operation must clear ALL other markers for that key whenever one is armed or consumed — not just the marker it's currently setting. Otherwise two same-key operations landing in one coalescing window leave a stale marker armed, and it silently swallows the next genuine external event.
- **When a PR's own description says a stale claim exists in multiple places, grep the whole repo for every copy before closing it** — fixing the copy in the diff's own file while sibling copies (a spec doc, a JSDoc comment, a contract table) still assert the old claim leaves the host-facing docs self-contradictory.
- **When a function grows a new parameter or branch, add a test that actually exercises it.** #2438's `renameFile` gained a 3-argument content-write path, but every existing test called the 2-argument form and hit an early `return` around the new code — "50 pre-existing tests still passed" proved nothing about the code that shipped, because none of them ran it.
- **A deadline/budget loop must check that enough budget remains for the NEXT action before taking it, not just that the deadline hasn't passed yet** (e.g. `deadline - now >= interval`, not bare `now < deadline`). #2437's quit-retry loop could fire a fresh redispatch in the same instant the deadline check next failed, starting a write with zero time left to finish before the window closed — the exact torn-write race the retry logic existed to prevent.
- **Timing-dependent tests need a generous margin or fake timers, not a tight real-timer race between two async ops.** A test asserting "op A lands before op B fires" with a 10ms-vs-40ms window is intermittently red under any CI jitter — indistinguishable from a real regression when it fails.
- **Don't leave an assertion in a test that passes regardless of the fix.** #2437 shipped `expect(getDirtyFiles()).toEqual([])` against a stub that clamps to empty unconditionally — it passed with or without the change under test and only added a false sense of coverage.
- **When a change narrows a behavior from unconditional to conditional (e.g. "always re-baselines" becomes "re-baselines only the unchanged subset"), grep for every doc/comment/spec bullet asserting the old unconditional wording — module headers and embedder-facing spec sections included — and correct them in the same PR.** Fixing the code without updating the prose that describes it leaves confidently false documentation for the next reader.
- **Before claiming "no changeset needed," check the specific package's own `package.json` (version, `private` field) and whether ITS observable surface changed** — don't infer from a differently-scoped precedent (e.g. a change to an unconsumed internal type shipping without one does not excuse a change to a published package's public API/state shape).
- **Verify PR-body claims about UI/menu/keybinding wiring against the actual binding code before writing them** — grep the real registration site (menu builder, keymap, command palette) rather than describing the affordance the feature "should" have; a doc comment in that same file often already states what is deferred.
- **When a test checks a cross-reference by string/substring match** (e.g. a workflow condition mentioning `steps.<id>`), also assert the referenced id/key actually exists at its declaration site — otherwise deleting the declaration silently breaks the link while the string-match test still passes.
- **Before writing "N properties are enforced by tests" (or any coverage claim) in a spec or doc, count the actual test functions and match the number.** An inflated claim leaves the uncounted property with no drift guard and reads as covered.
- **Confirm a regression test exercises the actual branch/routing under test** — the specific CST/HIR path, the specific enabled flag — rather than an output that could equally come from an untouched code path. Filtering to the wrong `kind`/category makes a test pass while never touching the changed code (#2436: the test filtered to `structural` folds, which are HIR/projection-driven and never consult the CST root the fix was about).
- **Verify a test's own comment matches what its body actually does** — that it decodes real values rather than checking bounds, and that the fixture has the structure claimed (nesting, choices). A false claim inside a test is worse than no comment: it hides the gap from the next reviewer.
- **State implicit cross-component contracts explicitly in the interface's own doc the moment a fix depends on one** (e.g. "`readFile` must return persisted, not staged, content"). An unstated assumption lets a differently-implemented sibling silently violate it and turn the fix into a no-op.
- **When a PR adds a new cross-package invariant/guard, name it in the spec/doc that already governs that invariant** (the test file, its exceptions, and the consequence of violating it) — an undocumented guard is spec drift the moment it lands, even though the guard code itself is correct.
- **When N hand-maintained copies of a value exist and you add a guard claiming to cover "all of them," count the actual copies first.** PR #2460's alias-map guard covered four playground copies while a fifth (`tsconfig.build.json`, with its own already-drifted exception) went unmentioned — an exhaustive-sounding claim that isn't exhaustive is worse than an honest "N-1 covered, here's the gap."
- **Name a test after what it actually exercises, not the production behavior it's meant to stand in for.** #2462's `ensure-cli-sidecar` test titled "honours CARGO_TARGET_DIR" only ever passed an explicit override — `sidecarPaths` never reads `process.env` — so the one env-sensitive code path the refactor could break was silently unguarded behind a passing, misleadingly-named test.
- **A directional error/remediation message ("X should have propagated, bump Y") must be checked against which side is actually ahead before it ships.** #2462's version-drift gate always blamed the root for not bumping, but reproducing the current lockfiles showed src-tauri was the one ahead on all three overlapping crates — the message pointed the fix in the wrong direction for the likelier real case.
- **When a loop checks two mirrored/symmetric sides of a comparison (e.g. "root" vs "mine" dependency requirements), apply identical strictness to both sides.** #2462's drift gate asserted the "mine" side parsed successfully but let the "root" side silently `continue` past a parse failure, shrinking the checked set with the test still green — the same silent-data-drop class as an unguarded `HashMap` iteration, just via a lenient loop instead.
- **When a comment justifies why a specific input is included in a filter/gate list, update that comment the moment new test coverage starts relying on the same input.** #2462 added a new assertion over root `Cargo.lock` without touching two comments that still justified its presence solely via the older tests — a future trim reasoning from the stale comment would silently disarm the new gate.
- **A "this build/step is the only one affected" rationale must be checked against every step in the actual CI lane, not the one you assumed.** PR #2474 justified removing three `CARGO_PROFILE_RELEASE_*` env vars by claiming the sidecar build was the lane's only release build — in comment, spec, and a test assertion — while the same lane's `wasm-pack build ... --target web` step (no `--dev`/`--profiling`) also produces a release build the vars were flattening, at 3+ minutes of build time. Trace the whole lane's steps before writing "only" or "no longer applies" anywhere.
- **A test guarding an env var's value must assert the exact string the consuming code compares against, not just that the key is set.** #2474's `sets_key("BRINK_SIDECAR_STUB")` matched any `BRINK_SIDECAR_STUB: "..."` line, but the script only opts in on the literal `"1"` — a workflow with `"0"` would pass the guard while silently restoring the full build the guard exists to prevent.
- **When a spawned-subprocess test inherits `...process.env`, sanitize any env var whose default now changes behavior — not just the ones already on the delete list.** #2474 added a `BRINK_SIDECAR_STUB` default and deleted `CARGO_TARGET_DIR` from a subprocess test's env for the same reason, but left the new var ambient, so the test could pass by matching real developer environments instead of exercising the code path it claims to cover.
- **Verify a PR-body architectural claim ("X is the canonical coordinator/consumer of Y") against actual call sites before writing it**, not against Y's existence or exported API. #2473's description claimed `OverlayPersistence` was the desktop's save coordinator; the desktop's own provider comment and a repo-wide grep showed no production instantiation of it at all — the claim was invisible to a reader who trusted the description instead of grepping.
- **When a CI trigger's `paths:` filter is meant to cover everything a test enumerates over a directory (e.g. "every `.github/workflows/*.yml`"), use a glob over that directory, not a list of the files you happened to touch.** #2509 named `ci.yml` and `desktop-smoke.yml` explicitly while its new test reads every workflow file — a reordered or newly-added workflow (e.g. a new `pnpm install --frozen-lockfile` lane) would silently skip the gate and only fail post-merge on main, exactly the gap the sibling coverage test exists to close.
- **When a PR changes something a spec or doc comment counts or enumerates ("N properties are asserted," "the filter lists every input"), update that count/claim in the same PR.** #2509 added a fourth asserted property and a new exemption without touching `desktop-shell-spec.md`'s "Three properties... are asserted" and "the path filter lists every input" bullets, leaving the spec self-contradicting the moment the PR landed.
- **Don't attribute a requirement to an issue, spec, or other source unless it is literally written there — quote only what the source actually says.** #2509's test comment and PR body claimed issue #2504 "instructed" the exact fix shape and enumeration requirement; #2504 said "fix shape: not prescribed" and contained no such instruction. Misattributed authority survives review because the next reader trusts the citation instead of opening the source.
- **A guard only has teeth if it lives on a REQUIRED/blocking CI lane — check whether the check you're adding can actually stop a bad merge, not just turn a lane red.** #2509 added a workflow-consistency assertion to `desktop-smoke.yml`, which is itself non-required by ruling, so a PR that broke the exact invariant being guarded could still merge past an advisory-only failure.
- **When a comment or spec claims a two-directional invariant ("state X is cleared on both close and reopen," "the old ticker doesn't survive a new one"), write a test for each direction, not just the one that's easy to trigger.** #2512's autosave test covered only "reopen replaces the timer"; relocating the actual `clearInterval` out of `closeProject` into `openProject` still left that test green, so the untested half (close-without-reopen) could leak a live timer indefinitely.
- **A new test file/script must be wired into an actual CI job, not just runnable locally.** #2492 added `scripts/check-wasm-pkg.test.mjs` but never added `pnpm run test:scripts` (or equivalent) to any workflow — its structurally parallel sibling (`ensure-wasm.test.ts`) IS gated. Also watch for glob-based runners (`node --test scripts/*.test.mjs`) that exit 0 with "# tests 0" when the glob matches nothing, so even a rename or typo silently "passes" forever.
- **A validation/guard step must run BEFORE the operation it protects, not after.** #2492's setup script printed `pnpm install --frozen-lockfile && pnpm check:wasm-pkg` — the check only reports after the broken half-linked `node_modules` it exists to prevent is already on disk. Order guard-then-act, never act-then-report.
- **A "keep these two files in sync" comment needs a test that actually reads both files and diffs them — a comment alone enforces nothing.** #2492 added `REQUIRED_FILES` with a comment pointing at `copy-wasm.mjs`'s `files` list, but nothing asserted the two stayed equal, and only one side's comment named the other; add the pointer comment on both files, not just the one hosting the test.
- **When quoting an issue, PR, or external claim inside a tracked comment or doc, preserve the qualifying word that carries the claim's actual scope** (e.g. "pays for a build it never executes" vs "pays for a fully-optimised `--release` build it never executes" are different claims) — and never bake a GitHub issue/PR's open-or-closed state into the wording, since that state can change after the comment is written and merged.
- **When two guards are logical mirrors of each other** (one guard's existence is *why* the other assertion holds, e.g. a stub-is-wired guard and a "these paths must stay excluded" guard), cross-reference them in both remediation messages — a future author fixing one side per its own guard's instructions must be told the other side needs the opposite change, or the pair silently goes out of sync.
- **When verifying a CI step, re-run it against the ACTUAL pinned tool/action version the workflow uses, not the version installed locally.** #2488's cargo-deny step was verified with the locally-installed 0.20.2, where `--config` is a top-level flag; the pinned action image ships 0.19.8, where the same flag placement is a `check`-subcommand-only option, so the assembled command line failed to parse (`error: unexpected argument '--config' found`, exit 2) and `continue-on-error: true` silently swallowed it — the step ran zero checks in CI while local verification looked clean.
- **Before pinning an exact string/value in a regression test, confirm the value is actually correct — don't copy whatever the file currently contains.** #2488's own guard test asserted the literal `arguments:` string that makes the CI step exit 2 before any check runs, so it cemented the bug it was meant to catch instead of catching it; a future SHA bump would keep passing the test while the step stayed dead.
- **When a new test requires a build artifact (e.g. a wasm-pack output directory) that the package's prior tests didn't need, update every doc that enumerates which commands need that artifact.** #2490 added an `existsSync` check over `crates/brink-web/www/pkg` to the studio unit suite, but CLAUDE.md's key-commands notes still name only `@brink/desktop test` as needing the wasm pkg first — a fresh clone now fails `pnpm --filter @brink-lang/studio test` against docs that call it self-contained.
- **When a PR's own doc makes a quantitative or absolute claim ("100% permissive", "N errors", "CI enforces X"), re-verify that claim against the PR's actual effect before merging, not against what was true before the change.** #2488 kept `deny.toml`'s "100% permissive, no copyleft obligations" comment and `desktop-shell-spec.md`'s "21 errors" figure unchanged while the same PR's own audit run produced 5 MPL-2.0 findings and a 22nd (unlicensed) error — the doc a future reader checks first was the one left wrong.

- **A comment claiming a bug class is fixed must be checked against every call still on the path it describes, not just the one that changed.** #2523's retained-frame callback added a comment asserting "only focus/select are deferred, as neither can lose input" while `input.select()` stayed unconditional — the exact defect the PR closed, reintroduced one line below the comment that says it's gone.
- **A CI path-filter gating a guard must list the file the guard's policy actually lives in, not just the code that consumes it.** #2522's `cargo-deny` guard reads `deny.toml`, but `deny.toml` was absent from the smoke lane's `pull_request.paths` — a PR that only edits the policy file (deletes an exception, admits a new license) never triggers the check that exists to catch exactly that edit.
- **A test parsing structured config text (TOML/YAML arrays, etc.) must be checked against a legal reformatting of that text, not just its current on-disk shape.** #2522's guard only recognized a multi-line `allow = [` block; collapsing it to one line made the parser return an empty list and the "does not contain MPL" assertion pass vacuously — verified by mutating the real file and watching both assertions stay green with MPL-2.0 admitted.
- **When the same command/check is meant to run identically in two places (a CI job and its local-script mirror, or two workspace legs of one job), diff the actual flags passed in each, not just confirm both exist.** #2525's local mirror ran bare `cargo deny check` while CI's job defaulted to `--all-features` via the action's own arguments — same command name, different resolved dependency set.
- **A "reinstall on version mismatch" step must gate what runs next on reinstall SUCCESS, not on a weaker proxy like "the tool is present."** #2525 printed "install failed; skipping audit" but then gated the audit block on `command -v cargo-deny` — true in exactly the case that triggered the reinstall — so the audit silently ran under the wrong pinned version while its own message claimed parity with CI.
- **A doc/ledger citation anchored to a line range must be updated in the same commit as any diff that adds or removes lines above it.** #2523 added 14 lines above a ruling-ledger citation that was exact on `main`, leaving the citation pointing at the wrong block the moment the PR merges — grep for line-number citations to the file you're editing before landing the diff.
- **Never share test fixtures by importing a `.test.ts` file** — vitest re-registers the imported file's `describe`/`it` blocks in the importer (measured: all 6 tests ran twice, 22 where there should have been 16; PR #2510 fixed by extracting a plain `save-paths.ts` module instead). A repo scan at PR #2510's head found no current violations, but nothing stops it recurring. Share fixtures/registries through a plain module (no `.test.ts` suffix); guard against new imports via `packages/brink-studio/src/__tests__/no-test-file-imports.test.ts` (#2516), which scans every `packages/*/src` file — `__tests__/` included, since the #2510 incident importer and the file it imported were both inside `__tests__/` — for a static `from "..."`, dynamic `import("...")`, or `vi.importActual("...")` naming a `*.test.ts(x)` sibling. Mirrored in `CLAUDE.md` § Rules.
- **When a set that a spec bullet, guard/dependant table, or array enumerates verbatim gains or loses a member, update EVERY enumeration of that set in the same PR, including a test's own array literal.** #2553 renamed/added a CI setup step (`check_wasm_pkg`) that changed which steps gate which checks: the desktop-shell-spec.md bullet and a CLAUDE.md bullet still listed the old step set, and the `dependants` guard table the PR itself edited had no entry for the new step — so the new step's gate was asserted by nothing while a comment in that same test claimed it was covered transitively.
- **Before writing a comment or doc claiming HOW a resolution/alias/build mechanism works, trace the actual call sites for every road that uses it, not just the one you assumed.** #2553's workflow comment claimed `tsc` resolves the `brink-web` alias "through node_modules" — true only for the Vitest lane, false for the tsc/tsup roads that actually govern the check it was documenting; a plausible-sounding mechanism claim is still a false one if it's wrong for the case that matters.
- **Before shipping a regex/pattern-based guard, grep the codebase for the ACTUAL convention it must match — don't write it against a hypothetical syntax the repo never uses.** #2551's import guard matched `from "./x.test.ts"`, a form that cannot compile under this repo's `bundler` moduleResolution (zero real occurrences); every real relative import uses `.js`, so the regex missed the incident form it existed to catch.
- **When building a regression guard for a named incident, confirm the guard's own scope/directory filter does not exclude the incident's actual file path.** #2551's guard excluded `__tests__/` from scanning while the #2510 incident it cites happened entirely inside `__tests__/` — the guard could never have caught the bug it was written for.
- **Don't generate one test per file/item when a single aggregate assertion (listing every offender) carries the same signal.** #2551 expanded into 148 per-file `it` cases that all pass or fail identically, inflating the suite's reported test count without adding coverage a single assertion wouldn't already carry.
- **When a PR closes a gap that a spec documents by name (a "known non-compliant paths" list, a "known gaps" bullet), strike or rewrite that entry in the same PR, and place any new warning in (or cross-reference it from) the spec that actually owns the code path it describes.** #2564 fixed the inline-rename refusal-toast bug but left `docs/studio-shell-spec.md` §7.5 still describing, verbatim, the mechanism the PR had just closed — a spec bullet that outlives the bug it names is confidently false the moment the PR merges.
- **Verify a claimed repro/trigger path (PR body, changeset, test header comment) actually fires through the real production code, not just through a test's mock.** #2564's test comment claimed "press F2 in a `-> hello` divert" produces a refusal; in production `prepareRename` returns a range for that site and the rename succeeds — only the test's unconditionally-null mock made the claimed path refuse. Trace the real gating function before writing "X causes Y."
- **When adding an exemption/escape-hatch to an enforcement test or guard, add a test that exercises the exempt path itself.** #2565's `SELECT-INVARIANT-EXEMPT` mechanism was structurally unusable — the enrolment loop and the completeness check together rejected an exempt marker whether or not its id was registered, a deadlock for the first author who followed the documented escape hatch. An escape hatch nobody can pass through is a bug, not a valid opt-out.
- **A justification comment citing code position or a runtime invariant ("guarded by the check on the line above/below," "runs synchronously in this mount effect, no window for the user to act first") must be verified against the literal code at that spot, including its actual control flow (effect deps, uncontrolled-input remount behavior) — not assumed by analogy with a similar comment elsewhere.** #2565 shipped two near-identical markers pointing opposite directions ("line above" vs "line below" for the same construct) and a third claiming a synchronicity guarantee an effect with a changing dependency and no `key`-based remount doesn't actually provide.
- **Before writing "X can never happen because of mechanism Y" in a doc, trace Y in the actual config/code — don't infer it from a similarly-named but different exclusion.** #2566 claimed a Rust crate was unreachable "because pnpm-workspace.yaml excludes it," but the glob includes it (pinned by an existing test); the real fences were a hard-coded `src` subpath and a JS/TS-only file filter. A wrong stated cause survives review looking authoritative and misdirects the next person who tries to "fix" it.
- **When a PR changes behavior a spec section describes (new helpers added, a fixture's scope widened, a scan's exclusion list changed), amend that spec section in the same PR — don't leave it asserting the pre-change shape.** Four PRs in one wave (#2583, #2582, #2585, #2584) each shipped a real behavior change while a `docs/*.md` section a reader would trust kept describing the old mechanism (a two-helper list after a third helper was added, "every workspace src/ tree" after a SKIP_DIRS exclusion existed, a stale "not enabled in studio" table row next to code that enables it, an audit description silent about a new timeout). Grep the spec/doc that owns the mechanism before closing the PR, not after a reviewer catches the drift.
- **A mock or test double standing in for a real error/refusal payload must be verified against the actual production output, not authored from what "should" happen.** #2583's `brink-web.ts` mock invented `"unknown variant \`Nonsense\`"` under a comment claiming it was "what serde answers" — the real serde message (confirmed by building a throwaway crate against the same `serde`/`#[serde(tag=...)]` shape) reads differently, and a shape-pinning test then locked in the fabrication as if it were production's vocabulary. When a mock reimplements production logic with edge cases (trimming, defaults, normalization), diff it against the real function's behavior line-by-line, not just its happy path.
- **Never read `$?` immediately after a negated (`!`) shell conditional or pipeline — it reports the negation's status, not the wrapped command's.** #2584's `if ! run_with_timeout 60 ...; then audit_exit_code=$?` always captured 0, so the `-eq 124` timeout branch was permanently dead code and a real hang printed a false "reported findings, exit 0" instead of the timeout message it was built to guarantee. Capture the real code with `rc=0; cmd || rc=$?`, never `if ! cmd; then rc=$?`.
- **A doc/spec claim about reachability ("refusals always reach X", "Y never happens", "this path forwards unconditionally") needs a test that exercises the real call chain, not a stub that bypasses the branch being described.** #2585 asserted two such claims about `applyCodeAction`/`onApplyStructural` forwarding in embedder-facing docs while the one test near that seam stubbed the function the claim was about, leaving both directions unverified.

- **Before pinning an error/message string in a mock or test, trace it from the real production call path and copy it verbatim — never from a plausible guess or a neighbouring function's wording.** A mismatched string passes the test while lying about what users see, and the test then locks the fabrication in as if it were production vocabulary. #2602 invented `entry file '${entry}' not found` where `CompileEntryError::EntryNotFound` actually renders `entry file not found in session: {0}` (`crates/internal/brink-ide/src/session.rs:127`), and pinned the invention in two tests. This is the third instance in three waves (#2583's fabricated serde message, #2599's shadowed `"unknown handle"`, this) — and #2603 records a fourth still live on `main`, where the parity fixture pins the *mock's* wording rather than production's, so the guard cannot see the drift it exists to catch.
- **Read the remediation text a guard prints, as a user would.** It must not recommend the exact command the guard just refused. #2596's refusal path let `check-wasm-pkg.mjs`'s own block print a bare `pnpm install --frozen-lockfile` *above* its `REFUSING TO INSTALL` message — telling a blocked developer to run the thing being blocked. Same class: when an error tells the user to avoid something, name the actual escape hatch (`--flag=value`), not an unrelated remedy (`pnpm add`) that doesn't apply.
- **"Pre-existing on main" is a claim with a shelf life — re-check it against CURRENT main before merging, and rebase rather than waving it through.** Four PRs in w165 each correctly diagnosed the same red check as pre-existing, but that argument expired the moment #2599 landed; every one of them needed a branch update before it could go green. Correct at diagnosis time ≠ correct at merge time.
- **Don't universalise an assertion across every observed entry when only some share the causal mechanism under test.** #2597's caret loop asserted end-parking over all probe entries including `defaultValue` rows, whose reading is inherited from the preceding `.value` write — encoding React's internal write ORDER as though it were platform behaviour. Scope the assertion to the subset the mechanism actually explains (`filter(e => e.prop === "value")`), or a future failure gets misattributed to the platform.
- **When a doc comment, spec bullet, or PR body describes ANOTHER module's mechanism, verify it against that module's current source in the same PR.** #2602's docstring named `compile_over_tree`/`Project::load` for a path that actually runs `EditorSession::compile_project → IdeSession::compile → CompileEntryError`, and claimed a `brink.toml [project] entry` misconfiguration could reach a refusal that `applyProjectConfig` makes unreachable. Self-contradicting drift — a comment describing behaviour the PR itself changed, or a claim the same file refutes elsewhere — is its own catchable failure class.
- **A measurement recorded in prose needs a test that fails when it changes, or it is indistinguishable later from an assumption.** #2595 existed *because* §7.7.1 cited a real-browser reading that had only ever been measured in jsdom. Its own fix then pinned the Chromium column with an e2e control block but left two jsdom cells resting on a probe run that was never checked in — while the spec described them as measured. Recording an unverified reading inside the fix for an unverified reading is the same trap one level down (closed by `seed-path-caret-jsdom.test.ts`).
- **When a PR widens what a script, fixture, or guard covers, update the spec section that owns that mechanism in the same PR.** This recurred four times in one wave (#2611, #2610, #2613, #2614): a spec kept describing the old, narrower behavior — an "ambient 10.x" framing after a version got pinned, "two guard halves" after a third was added, two typecheck programs after a third was wired in, "CI needs neither" after a lane started running the guarded path — leaving the spec confidently wrong the moment the code merged. Grep the spec for the mechanism's name before closing the PR, not just the code paths.
- **When collapsing two different failure causes into one code path (e.g. "tool not found" vs "tool ran and exited non-zero"), distinguish them by the actual signal (`error.code === 'ENOENT'`), not by both landing on the same `null`/falsy result — and never discard `stderr` in the process.** #2611's `resolvePnpmVersion` returned `null` for both "pnpm absent" and "pnpm ran and failed," reporting the wrong cause and discarding the real error; its own test then treated the failure case as a skip instead of a hard failure, because it keyed off the same collapsed `null`.
- **A "does every fixture entry have a real call site" canary must track consumption by KEY, not by matching the entry's VALUE.** #2610's guard used a `Set` of message strings where multiple fixture keys shared the identical string, so deleting one key (or replacing its call site with a hand-typed literal) still passed — any other key with the same value covered for it. Populate a `Set<key>` from inside the production-call helper and diff it against the fixture's key list.
- **A guard's docstring/comment must state the exact check it performs, not the strongest-sounding version of it.** Two cases in one wave: a "so any other coinage is red at the source" claim on a guard that only flags strings containing the literal substring `"handle"` (#2610), and a "validated against what's on disk" claim borrowed from a sibling guard that actually does a name-for-name listing diff, on a new guard that only checks `include` contains a substring and a directory has ≥1 matching file (#2613). Write the comment from the code's actual filter/assertion, never copied from a stronger relative.
- **When repointing every occurrence of a bad pattern, check against the issue's own list of sites (or the underlying failure condition), not a single grep string for one variant of it.** #2614's search for `pnpm install --frozen-lockfile` silently skipped a documented site written as bare `pnpm install` — the same #2593 failure sequence, just without the flag. Then extend the existing regression-guard test (the doc-reachability / positive-negative pattern) to cover each newly-repointed site, or it can silently revert with a green gate.

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
