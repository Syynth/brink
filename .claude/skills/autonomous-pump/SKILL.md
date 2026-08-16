---
name: autonomous-pump
description: >-
  Run an autonomous multi-agent build loop to work through a backlog of well-scoped
  changes on a green-gated repo — triage → parallel worktree builds → adversarial
  review → serial merge-train → fix-loop — paired with human "drive-it" verification.
  Invoke when the user wants to autonomously implement many issues/features with
  parallel subagents while keeping main green, asks to "run the pump" / work
  "work-pump style", or wants multi-agent orchestration over a backlog. Requires
  git, a CI-style gate (build/test/typecheck), and ideally a GitHub issue board.
---

# Autonomous build pump

A method for chewing through a large backlog of changes with parallel subagents while keeping `main` green and quality high. The **human steers direction and verifies by *using* the result**; **you turn direction into issues, run the loop, keep main green, and report.**

## When to use / not use
- **Use** when: many well-scoped changes; a repo with a fast green gate (build + test + typecheck); the user has opted into multi-agent orchestration (the Workflow tool).
- **Don't** use for: a single small change (just do it), exploratory/unclear-domain work (design first — see Gate 0), or anything without a reliable gate.

## The contract
- **Human**: sets direction, makes design calls, verifies by driving the real app.
- **You**: turn direction into issues, run the pump, keep `main` green, surface findings, report at each checkpoint, and **escalate design decisions instead of guessing**.
- Treat an interrupt as a course-correction, not a rejection. Report faithfully — failures with output, parked work with reasons.

## Gate 0 — triage BEFORE building (push quality left)
1. **Everything is a GitHub issue on the board** (labeled, assigned). Findings → issues; nothing lives in chat or scratch docs.
2. **Classify each issue: build-ready vs `needs-design`.** Only build-ready enters the pump. Anything that mirrors an existing tool/domain, or changes architecture / UX *feel*, is `needs-design` until the model is locked **with the human**. *(Hard-won: building a feature before understanding the domain — e.g. how the reference tool actually models it — gets thrown away and rebuilt. Study the reference + lock the model first.)*
3. **Refactor for parallelism first.** If many issues would edit one hot file, do a behavior-preserving refactor into disjoint files (e.g. a data-driven registry) and verify it's identical before merging. Then agents won't collide.
4. **Cluster related issues**: combine same-file ones into a single PR via the batch entry's `closes: [n, …]` field (the template generates the honest multi-`Closes` body); serialize ones that share a hot file; split oversized issues so the closes list stays honest.
5. **Lane each issue.** Docs/comments/banner-level changes get `lane: "light"` (lower agent effort + a cheaper per-entry `gate` override, e.g. typecheck-only) — full adversarial ceremony on a README edit is wasted tokens. Default lane is full.

## Model tiers (credit control)
Subagents inherit the session model unless overridden — on a top-tier session that burns premium credits on mechanical work. The template pins: **builds/train/fix = sonnet** (light lane = haiku), **adversarial review = opus** (the quality bar — don't cheap out where the bugs get caught), per-entry `model:` override for known-hard builds. Rationale: three consecutive waves' reviews found real bugs the gate missed; nothing else in the pipeline needs the top tier.

## The pump — a Workflow
A ready template lives at **`pump.js`** (next to this file). **Copy it and fill the inline CONFIG block with literals** (repo, gate + shared-cache prefix, trailer, conventions, house rules, batch of `{n, hint, closes?, lane?, gate?}`), then `Workflow({scriptPath})` — inline config is the only supported path (the Workflow tool's `args` reaches scripts unparsed in this build; don't add an args fallback). Adapt the prompts to the project, but keep the quality steps below.

**v3 flow — per-item pipeline, no head-of-line blocking.** Each issue runs build → review → land **independently** (`pipeline()`): a finished build goes straight to ITS adversarial review while slower siblings keep building. (The old all-builds barrier once parked two green PRs for ~an hour behind one slow sibling's disconnect-retries.) The ONLY serialization is the **train queue** — a promise chain through which merge AND fix agents run one-at-a-time in *completion order* (they share the one train worktree, and each merge advances main). Verdicts and landings ride the pipeline WITH their item, so there's no cross-phase PR-string matching. Lessons + scope reconciliation remain terminal (they genuinely need the whole wave's findings).

- **Build** (parallel across items, **worktree-isolated**, one issue → one PR off `origin/main`). Each agent must:
  - read the issue, implement TDD, pass the **gate** (build shared/workspace deps FIRST, then test → typecheck → build);
  - report gate evidence as a **`gateResults` array, one `{command, result}` row per gate command, `minItems` pinned to that item's own gate command count** (`gateFor(b)` split on `&&`) — a schema-enforced replacement for free-text `gateOutput` (#2645). Two earlier attempts (`required` + `minLength`, then a schema description demanding per-command coverage) were both satisfiable by asserting harder; a short array now fails at the tool-call layer and forces a retry (**verified by direct probe, not inferred** — see "Harness enforcement of the build schema" below). This makes a MISSING command mechanically impossible to submit — it does **not** verify a reported result is TRUE, and cannot tell a real green from a fabricated one; the adversarial reviewer's job is still to disbelieve it. `gateOutput` stays as a free-text field for preflight/disk notes that belong to no single command; both are shown to the reviewer.
  - **prove reachability** before opening the PR — state the user path that exercises the feature, not just "tests pass" (catches dead / unwired code);
  - run a **conventions lint** (e.g. grep new design tokens against the real token file; no invented identifiers; house style);
  - **flag scope overflow** — if the real work exceeds the issue (hidden coupling, a missing prerequisite, an under-sized issue, obvious adjacent follow-ups), report it as `scopeNotes` instead of silently growing the PR. Keep `Closes #N` honest; the overflow becomes a candidate issue, not a bloated diff.
  - stage explicit paths, commit, push, open a PR with an **honest** `Closes #N`.
- **Review** (per item, starts the moment that item's build returns; one **adversarial** reviewer per PR). Prompt it to REFUTE — find bugs, dead code, scope gaps, regressions; **and call out where the issue/milestone *under-captured* the work** (scope the plan missed); default to "request changes" if materially off. This is your real quality bar and *will* catch what the gate didn't.
- **Land** (**the train queue** — serial in completion order; keeps main green). An `approve` verdict enqueues a merge agent; a `changes` verdict enqueues a fix agent carrying the reviewer's **exact** findings (apply-fix → re-gate → merge); `reject` parks for the human. Both train agents report their re-gate as the **same `gateResults` array the build does**, `minItems` pinned to that item's own gate command count (#2664) — this phase re-runs the identical gate on the commit that actually lands on main, so a free-text claim here was the same unevidenced-claim hole #2612 → #2645 → #2657 closed at BUILD, one phase later and at higher stakes. A park that aborts *before* gating still returns a row per command saying `"not run — merge aborted before the gate"`: silence is what the array exists to prevent, and an explicit "not run" is honest. The rows render through `formatGateEvidence` into the wave's returned `landed`/`awaitingChecks`/`parked` entries, and a compact `merge gate rows k/N` ratio rides the retro's tracker table. Runs in **ONE persistent train worktree** (created once, node_modules survives across stops — a fresh worktree + install per merge was pure critical-path overhead). Per landing: update against main, re-gate, combine additive conflicts (registry/index appends — ⚠ diff3/union styles can duplicate/drop closing braces when combining; recheck brace balance, the gate is the arbiter), merge with `--delete-branch` (detach first so local deletion can't fail). Park what won't cleanly land. `log()` a landed-count pulse per stop.
- **Reconciliation** (end of run): assert every built+approved PR was **merged OR parked-with-a-reason** — no silent drops. (v3: verdict + landing ride each pipeline item, so this is a per-item read — never match PR identifiers across phases on decorated strings; that once mis-parked approved PRs.)

## Harness enforcement of the build schema — probed, not assumed (#2665)

Three rounds of work (#2612 → #2645 → PR #2657) rested on one unverified
assumption: that the agent harness's structured-output validator actually
**rejects** a tool call violating the BUILD schema, rather than accepting it
and leaving `minItems`/`minLength` decorative. Documented is not verified —
that distinction is the whole subject of the saga. It was probed directly on
**2026-08-16 (wave w169, issue #2665)**.

**This record's home is `SKILL.md`** — deliberately, and future issues in this
area should say so (#2673). #2665 asked for the result in `BRINK-CONFIG.md`;
that file is owned by the lessons phase's per-wave PR and every build fence
forbids touching it, so the record landed here instead. Naming the wrong home
cost one round of fence conflict; it should not cost a second.

**Probed under CLI version `2.1.233`** (`claude --version`, the native
launcher at `/opt/node22/bin/claude` — the binary that actually serves the
session). Recorded 2026-08-16 alongside the probe wave. ⚠ The *source*
corroboration below was read from a **different artifact**: the npm bundle
`@anthropic-ai/claude-code@2.1.42` at
`/opt/node22/lib/node_modules/@anthropic-ai/claude-code/cli.js`. Both carry the
`Output does not match required schema` string, but they are not the same
build, so treat the Ajv reading as corroboration of a *shape*, not as proof
about the running binary. The **probes** are the evidence; the source is
secondary, and now explicitly so.

**RE-PROBE TRIGGER.** Re-run the probe when `claude --version` no longer
matches the version pinned above. The pump evaluates this every wave: the retro
prompt in `pump.js` runs `claude --version`, compares it to this section, and
recommends a re-probe on drift. ⚠ **Nothing in-tree can detect a harness that
stopped enforcing** (see the boundary paragraph below and
`scripts/pump-gate-schema.test.mjs`'s header) — a version comparison is a proxy
for "the thing I probed may no longer be the thing running", not a test of
enforcement. It is also only as reliable as the retro agent obeying a prompt;
that is the same trust every other instruction here runs on, and it is worth
saying rather than implying. The recipe is one deliberately-short `gateResults`
array and one row whose `result` is under 8 characters — see both probes below.

**The probe.** A live build agent whose per-item schema was
`buildSchemaFor({gate: "node --check … && pnpm run test:scripts"})` —
`gateResults.minItems: 3` — deliberately submitted its `StructuredOutput` call
with **2** rows and everything else valid. The harness answered:

```
Output does not match required schema: /gateResults: must NOT have fewer than 3 items
```

The call was **rejected**, the result was **not** recorded, and the agent was
free to retry with a complete 3-row array — exactly the behaviour #2645/#2657
assumed. **`minItems` is enforced.**

**The second probe (`minLength`, #2612's half).** Separately observed, because
it is a separate claim: the same agent then submitted a complete 3-row array
whose first row's `result` was `"exit 0"` — 6 characters against
`minLength: 8` — and nothing else invalid. The harness answered:

```
Output does not match required schema: /gateResults/0/result: must NOT have fewer than 8 characters
```

Rejected likewise. **`minLength` is enforced, on nested array items, by
JSON-Pointer path.** #2612's fix is not decorative either.

**Corroboration (source, secondary to the probes).** The harness constructs the
`StructuredOutput` tool by compiling the supplied JSON Schema with Ajv
(`new Ajv({allErrors: true})`, `validateSchema` then `compile`), and its
`call()` throws `Output does not match required schema: <instancePath>:
<message>` when the compiled validator rejects the input — which is exactly the
shape of both messages above. (Seen in the installed CLI bundle at
`@anthropic-ai/claude-code/cli.js`.) That the errors carry Ajv's JSON-Pointer
`instancePath` and Ajv's own wording is why the two probes generalise: the full
draft vocabulary is honoured, not a hand-rolled subset.

**What this does and does not establish.** It establishes that a **missing**
command row is mechanically impossible to submit. It establishes nothing about
whether a reported `result` is TRUE — an agent can still write a fabricated
"36 passed" for a command it never ran. That remains the adversarial reviewer's
job, and `formatGateEvidence`'s INCOMPLETE banner remains the reviewer-facing
signal for the shortfall the schema now prevents at the tool-call layer.

**The in-tree / harness boundary.** `scripts/pump-gate-schema.test.mjs` checks
the half this repo owns — that pump.js *generates* a schema carrying
`minItems` equal to the item's own gate command count, `minLength` on every
gate-evidence string, `required` on each row, and no typo'd keyword a validator
would silently ignore; plus that the build call site passes that schema. It
**cannot** check the harness's validator: there is nothing to import, and a
future harness release that stopped honouring `minItems` would go undetected
in-tree. Re-run the probe if that assumption ever needs re-confirming; the
recipe is one deliberately-short `gateResults` array, and one row whose
`result` is under 8 characters.

**The floor is pinned; the ceiling is deliberately open (#2672).** `minItems`
has no `maxItems` counterpart, and adding one would be a regression, not the
missing half of a symmetry. `gateCmds` is a crude `&&` split, not a shell
parser, so it **under-counts** any gate hiding a step behind `;` or a
subshell. `minItems` fails *safe* under that under-count — the floor only ever
comes out too low, never stricter than the gate actually is. `maxItems` would
invert exactly that property: an honest agent that ran and reported **more**
steps than the split could see would be rejected at the tool-call layer and
pushed to **delete evidence** to satisfy the schema. Extra rows are also often
legitimate (a preflight `df -h /`, a leg re-run after a fix). Over-evidence was
never the hole this saga was closing; under-evidence was. What #2672 fixed
instead is the reviewer-facing wording: `formatGateEvidence`'s banner is now
**direction-aware**, so an over-long array reads *"this is OVER-COMPLETE, NOT
incomplete"* rather than the old `results.length !== expected` banner that
called padded evidence "INCOMPLETE" and sent the reviewer hunting for a command
that was never missing. The under-length wording is unchanged — it is the
load-bearing direction, and the one signal that survives if a future harness
stops enforcing `minItems` at all. Row rendering is also bounded now
(`GATE_ROWS_CAP`, `GATE_CMD_CAP`), and when a cap bites the output **says how
many rows it dropped** — #2645's lesson was that truncation may shorten
evidence but must never make a command silently disappear.

## Close the learning loop
The template now closes it mechanically: a final **Lessons** agent distills the wave's review findings into generalizable, paste-ready house-rule candidates (returned as `lessons`). Feed them into the next wave's `RULES` — with human review, since not every finding generalizes. The pump should get *smarter* each cycle, not repeat the same mistakes (e.g. "use only tokens from tokens.css", "wire the feature into the UI, not just the hook").

## Scope reconciliation — let the plan catch up to reality
Milestone scope is an **estimate made before building**; building reveals work the plan didn't capture. Treat scope as **provisional** and reconcile it, so discovered work neither bloats PRs nor evaporates in chat:
- **Surface it continuously.** Every build emits `scopeNotes` (work beyond the issue) and every review flags under-captured scope. Aggregate these alongside the usual findings.
- **Reconcile after each milestone.** When a milestone's batches finish, run a **scope-reconciliation pass**: compare what actually shipped vs. what the milestone planned, fold in all `scopeNotes`/scope-gap findings, and assess — *was this milestone's scope accurate, or did it under/over-capture?* Produce a concrete proposal: **file new issues**, **expand** the current or a later milestone, or **add a new milestone** for a coherent chunk of newly-surfaced work.
- **Milestone structure is a human call.** The pump *proposes* (with rationale + the candidate issues); the human approves before any milestone is added/expanded or issues are filed and re-triaged through Gate 0 (build-ready vs `needs-design`). Don't silently absorb scope into open PRs, and don't autonomously rewrite the roadmap.

## The drive-it gate (non-negotiable)
**Tests passing ≠ working.** After the pump, the human runs the app and *uses* it — lived-UX and reachability problems are invisible to green tests. Make the app **automatedly drivable early** (real screenshots / interaction smoke tests) so the human gate spends itself on *taste*, not on catching broken basics. Be honest when you can't verify (e.g. a headless browser that mis-renders) and defer to the human.

## Operational hygiene
- **Cloud sessions**: approval-gated (not bypass-permissions) — several local assumptions fail (agent merges park, no gh CLI, fixed disk allowance, wasm-pack sandbox limits, MCP token expiry). See the project config's "Cloud sessions" section for the full delta list; fold it into agent prompts, not just the plan.
- Worktree isolation fills the disk fast — prune stale worktrees + `git worktree prune` between **every** cycle.
- **Ban `git stash` in agent prompts** — all worktrees share ONE stash stack; two concurrent agents stash-popping have swapped each other's WIP (recovered only by forensics). WIP goes on the agent's own branch. (The template's `DISK` preamble now carries this.)
- Build workspace/shared deps before testing consumers.
- Serialize merges to main; never run two agents editing the same hot file in parallel.

### Disk — sharing caches across worktrees
Worktree isolation duplicates `node_modules` + build outputs per agent. The big costs are almost always **un-pruned worktrees** and **non-hard-linkable build dirs** (e.g. Rust `target/`, tens of GB) — not JS deps. Mitigate:
- **Keep worktrees on the same volume as the package store.** pnpm's global content-addressable store hard-links package content into each `node_modules`, so N worktrees share inodes and real free space (`df`) is far below the per-worktree `du`. Cross-volume → it *copies* instead. (npm/yarn-berry caches benefit similarly.)
- **Don't reinstall — clone `node_modules`** when the lockfile is unchanged: macOS/APFS `cp -c -R src/node_modules wt/node_modules` (clonefile = copy-on-write: instant, ~0 extra space, safe — writes diverge); Linux `cp -al …` (hard links) or `cp --reflink=auto …` (btrfs/xfs). Skips the install step too.
- **Share build-output caches** (the real hogs — never hard-linked): Rust → one shared `CARGO_TARGET_DIR` (or `sccache`); TS/JS monorepo → Turborepo/Nx local cache (hash-keyed, shared across worktrees) + incremental `tsc --build`. A single shared Rust `target/` can save tens of GB vs per-worktree rebuilds.
- **The template enforces both**: `pump.js`'s CONFIG has a `CACHE` prefix (on brink: `export CARGO_TARGET_DIR=… CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0 CARGO_BUILD_JOBS=4`) prepended to every gate invocation, and a `DISK` preamble in every build prompt instructing the node_modules clone. ⚠ `CACHE` carries the cloud-disk vars deliberately: the prompt calls them mandatory on every cargo invocation, and putting them in the prefix is what makes that true without an agent hand-editing the gate string. A shared `CARGO_TARGET_DIR` also grows across waves — it once hit 53GiB (#533) — so the `DISK` preamble measures it and the orchestrator sweeps it at wave close. Advice that lives only in this doc never reaches the agents — prompts are the only part they obey. This isn't just disk: the shared build cache is the pump's single biggest **wall-clock** lever (the serial merge-train's re-gates become cache hits).
- **Bound peak disk:** prune every cycle, cap batch size (fewer concurrent worktrees = lower peak), and for short-lived build worktrees consider a tmpfs/RAM-disk mount (auto-freed). Watch `df` mid-run; `cargo clean` / clear stale `wf_*`/`agent-*` worktrees on an `ENOSPC`.

## Per-project setup checklist (fill before the first run)
- **Gate command** — build shared deps → test → typecheck → build.
- **Repo + default branch**; **board id + label + assignee**.
- **Conventions string** — language, quotes, export/file style, design-token source.
- **House rules** — seed empty; grow from review findings.
- **Reference tool/domain** to study for any mirroring feature (feeds Gate 0).
- **How to run + drive the app** for the human verification gate.

## The outer loop — ledger reconciliation (a different axis)

Scope reconciliation is **forward-looking**: "what did *this batch's* building reveal?" It cannot see a decision that was ruled and never built, because no agent in the inner loop ever reads the decision log end to end. That is a **backward-looking** question on a slower clock, and it needs its own pass.

There are **three sources of truth**, and each pairwise gap is a distinct failure mode:

| gap | failure | how it hides |
|---|---|---|
| log ↔ issues | **ruled, never tracked** | no issue exists, so nothing surfaces it |
| issues ↔ code | **built, still open** | discovered only by assigning it and finding nothing to do — burns a wave slot |
| log ↔ code | **ruled, silently contradicted** | the worst: no issue, no failure, the code just quietly disagrees |

**The highest-risk entries are the ones with an assumed home** — "folds into B0.8", "batch the bump with X", "rides the v6 bump". They *look* owned. When the named home closes without delivering, nobody notices. (Real case: the block-as-expression checker was ruled 2026-07-20 "folding into B0.8"; B0.8 closed without it and it had no owner for a week.)

**Run it:** the `ruling-ledger-audit` workflow — extract forward commitments from the log (filtering out entries that merely *record* a change, which cannot orphan) → trace each against issues, PRs, and code → classify DELIVERED / TRACKED / **ORPHANED** / **CONTRADICTED** / SUPERSEDED / UNKNOWN → write `docs/ruling-ledger.md`. The ledger is a **derived view**; the decision log stays immutable (it records what was decided *when*; the ledger records what happened to it). Because the ledger is checked in, subsequent runs are incremental and **its diff is the report**.

**Cadence — two triggers:**
- **After every design sitting.** That is when rulings are minted and the ruling→issue gap is *created*; catching it there is cheapest.
- **Every ~10 waves**, plus on track/epic closure, for drift.

**⚠ This task shape invites fabrication** — it is a plausible-sounding audit over material nobody will re-check. Every DELIVERED or CONTRADICTED verdict must cite a merged PR or a `file:line` actually read, and **UNKNOWN must be a permitted verdict**. A confident wrong ledger is worse than an incomplete one, because the next agent trusts it.

## Run rhythm
Propose the issue list + the first parallel batch and **wait for the human's OK** before spending tokens. Then: triage → refactor-for-parallelism → pump a batch → reconcile → human drives → file new findings → repeat. **At each milestone boundary, run scope reconciliation** and propose any new/expanded milestones for the human to approve before starting the next one. Report at every checkpoint; stop and surface design (and scope/milestone) decisions rather than guessing.
