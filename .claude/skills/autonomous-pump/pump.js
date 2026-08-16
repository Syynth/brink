// autonomous-pump — Workflow template.
//
// USE: COPY this file, fill the CONFIG block below with literals, and run
// Workflow({scriptPath: "<your copy>"}). Inline config is the ONLY supported
// path — the Workflow tool's `args` reaches scripts unparsed in this build,
// so an args-reading fallback just wastes a launch. Adapt prompts to the
// project, but keep the quality steps (reachability proof, conventions lint,
// adversarial review, serial merge, no-silent-drops reconciliation).
//
// Flow: PER-ITEM PIPELINE (v3 — no head-of-line blocking): each issue runs
// build -> review -> land INDEPENDENTLY; a finished build goes straight to ITS
// adversarial review while slower siblings keep building. The ONLY
// serialization is the TRAIN QUEUE: merge/fix agents share one persistent
// train worktree and each merge advances main, so landings run one-at-a-time
// in COMPLETION order. Lessons + scope reconciliation close the wave.

export const meta = {
  name: "autonomous-pump",
  description: "Per-item pipeline (build -> adversarial review -> serial train landing, no cross-item barriers) + lessons + scope reconciliation, over a batch of issues.",
  phases: [
    { title: "Build" },
    { title: "Review" },
    { title: "Merge train" },
    { title: "Fix loop" },
    { title: "Lessons" },
    { title: "Retro / scope reconciliation" },
  ],
};

// ── CONFIG (fill per run — literals only) ────────────────────────────────────
const REPO = "OWNER/REPO";
// CLOUD: true in a Claude-Code-on-the-web session, false for a local run.
// This is not cosmetic — a cloud session has NO `gh` CLI, so every prompt below
// that shells out to `gh` dies on its first command. It also has a fixed disk
// allowance and an approval-gated permission layer. The project config
// documented all of this and the PROMPTS still said `gh`; the doc is not the
// part agents obey. Setting this flag rewrites the prompts.
const CLOUD = true;
// CACHE is prepended to every gate invocation so all agents share one build
// cache — the pump's single biggest speed lever.
// CARGO_INCREMENTAL=0: every build agent works in a FRESH worktree, so the
// incremental dep-graph cache is written and never read — pure disk write for
// zero benefit, multiplied by ~5 agents per wave across thousands of builds.
// (Local human iteration is unaffected; this only applies to agent gates.)
//
// ⚠ THESE TWO ARE REPO-SPECIFIC AND MUST STAY FILLED IN. They shipped as
// generic template defaults (`TURBO_CACHE_DIR`, `pnpm turbo run test typecheck`)
// and nobody noticed for a long time, because BRINK-CONFIG.md's per-item `gate:`
// overrides masked it: light-lane entries always carry one, so only entries
// WITHOUT an explicit `gate:` ever reached the default — and those are the
// heavyweight opus items. This repo has no turbo (no `turbo.json`, no
// dependency), so `pnpm turbo run test typecheck` exits "Command not found".
// `gateFor` is used at THREE points — build, merge-train and fix-loop — so the
// broken default reached all three, and each agent improvised its own
// substitute. That makes the gate not a floor but a per-agent choice, which is
// the opposite of what a gate is for. Caught 2026-08-16 when two w166 agents
// independently reported the command does not exist. Values below are
// BRINK-CONFIG.md's documented "Rust (default GATE)" and its CACHE prefix; a TS
// entry still overrides via `gate:` (see BRINK-CONFIG.md "TS entries").
const CACHE = "export CARGO_TARGET_DIR=/tmp/pump-cargo-target-brink CARGO_INCREMENTAL=0";
const GATE =
  "cargo fmt --all -- --check && cargo clippy --workspace --all-targets -- -D warnings && cargo nextest run --workspace && cargo test -p brink-test-harness --test oracle_snapshots";
const MILESTONE = null; // optional milestone name for scope reconciliation
const LEDGER = null; // optional: standing wave-ledger issue number (durable-by-default rule) — brink: 967
const WAVE_ID = "wN"; // fill per wave when LEDGER is set
// Match the harness's configured co-author line for this model/session.
const TRAILER = "Co-Authored-By: Claude <noreply@anthropic.com>";
const CONV = "(set CONV: language, quotes, file style, token source, PR footer)";
// Seed RULES from the previous wave's `lessons` output — that's the learning loop.
const RULES = "";
// Batch entries: { n, hint, closes?: [n, ...], lane?: "light", gate?: "<override>", model?: "opus" }
//  - closes: all issues this ONE PR honestly closes (clustered same-hot-file work);
//    defaults to [n].
//  - lane "light": docs/comments/banners-level changes — lower agent effort and
//    (via gate override) a cheaper gate, e.g. typecheck-only. Default lane: full.
const BATCH = [];
// ─────────────────────────────────────────────────────────────────────────────

// ── GitHub access layer (cloud has no `gh` CLI) ──────────────────────────────
// Every GitHub interaction in every prompt below routes through this object, so
// the cloud/local difference is expressed ONCE instead of being sprinkled across
// ten prompt strings that silently rot.
const GH = CLOUD
  ? {
      pre: `GITHUB ACCESS — ⚠ THERE IS NO \`gh\` CLI IN THIS ENVIRONMENT. Any \`gh …\` command fails with "command not found"; there is also no direct GitHub API access and no \`hub\`. Use the GitHub MCP tools (\`mcp__github__*\`) for ALL GitHub work.
⚠ Those tools are DEFERRED: their schemas are not loaded, so calling one directly fails with InputValidationError. Load what you need FIRST, in one call, e.g.:
\`ToolSearch({query: "select:mcp__github__issue_read,mcp__github__issue_write,mcp__github__create_pull_request,mcp__github__add_issue_comment,mcp__github__pull_request_read,mcp__github__list_issues", max_results: 10})\`
Then call them normally. \`git\` itself (fetch/push/merge/worktree) works fine — it uses a separate credential path — so keep using plain git for code movement and MCP only for issues/PRs/comments/merges.`,
      issueView: (n) => `read issue #${n} with \`mcp__github__issue_read\` (method "get"; owner/repo from ${REPO})`,
      issueComment: (n, what) => `post ${what} with \`mcp__github__add_issue_comment\` on issue #${n}`,
      prCreate: `open the PR with \`mcp__github__create_pull_request\` (base "main", head your branch, draft false)`,
      prDiff: (pr) => `\`mcp__github__pull_request_read\` (method "get_diff") on PR ${pr}`,
      prView: (pr) => `\`mcp__github__pull_request_read\` (method "get") on PR ${pr} — the head branch is \`head.ref\``,
      prComment: (pr, what) => `post ${what} with \`mcp__github__add_issue_comment\` on PR ${pr} (PRs accept issue comments)`,
      prMerge: (pr) => `LAND PR ${pr} per the three-case LANDING POLICY below: read its check runs first — checks still pending → \`mcp__github__enable_pr_auto_merge\` (mergeMethod "MERGE"); checks all completed green AND review approved/fixed → PARK IT for the orchestrator to merge (you have NO merge authority — never call \`mcp__github__merge_pull_request\`); anything else → park it`,
      issueList: (extra) => `\`mcp__github__list_issues\`${extra ? ` (${extra})` : ""}`,
      // Cloud permission layer can PARK a subagent's privileged call with no
      // human watching. Parked is recoverable state — but only if it is durable.
      parkNote: `⚠ PERMISSION PARKING: this session is approval-gated. If a merge/push-adjacent call is refused or parked, do NOT retry it in a loop and do NOT silently drop the work. Record the exact intended action in your returned \`detail\`/\`summary\` AND (if you can comment at all) on the PR, then return with landedState:"parked". The coordinator lands parked merges. A parked action that was never written down is the only unrecoverable kind.`,
    }
  : {
      pre: `GITHUB ACCESS: the \`gh\` CLI is available and authenticated.`,
      issueView: (n) => `\`gh issue view ${n} --repo ${REPO}\``,
      issueComment: (n, what) => `post ${what}: \`gh issue comment ${n} --repo ${REPO} --body "…"\``,
      prCreate: `\`gh pr create --repo ${REPO} --base main --head <branch> --title "<concise>" --body "…"\``,
      prDiff: (pr) => `\`gh pr diff ${pr} --repo ${REPO}\``,
      prView: (pr) => `\`gh pr view ${pr} --repo ${REPO} --json headRefName -q .headRefName\``,
      prComment: (pr, what) => `post ${what}: \`gh pr comment ${pr} --repo ${REPO} --body "…"\``,
      prMerge: (pr) => `LAND PR ${pr} per the three-case LANDING POLICY below (detach first so local branch deletion can't fail): checks still pending → \`gh pr merge ${pr} --repo ${REPO} --merge --auto --delete-branch\`; checks all completed green AND review approved/fixed → PARK IT for the orchestrator to merge (you have NO merge authority — never run \`gh pr merge\` without \`--auto\`, and never call \`mcp__github__merge_pull_request\`); anything else → park it`,
      issueList: (extra) => `\`gh issue list --repo ${REPO}${extra ? ` ${extra}` : ""}\``,
      parkNote: ``,
    };

// Cloud disk + durability deltas, folded into every agent prompt that builds.
// Earned across three ENOSPC incidents and one container snapshot revert that
// destroyed a finished, unpushed wave.
const CLOUD_DISK = CLOUD
  ? `
CLOUD DISK — MANDATORY ON EVERY CARGO INVOCATION: prefix with \`CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0\` and pass \`-j 4\`. Full debuginfo caused two same-day ENOSPC crashes here. The disk is a FIXED PER-SESSION ALLOWANCE, so \`df\` "Avail" misleads: Avail at 0 with low Used means the allowance is spent, not that the machine is broken. On ENOSPC, deletes still succeed — STOP and report; never self-clean shared state.
⚠ ONE full workspace gate at a time across ALL agents. Two parallel gates exceeded the allowance twice. If the coordinator gave you a GO token, hold it for the duration of your gate and report the moment you release it.
CLOUD DURABILITY — THE CONTAINER IS NOT STORAGE: it can be reverted to an older snapshot without warning, taking .git, every worktree and the object store with it (a complete READY-FOR-GATE three-commit wave was lost exactly this way). PUSH IMMEDIATELY AFTER YOUR FIRST COMMIT and after every subsequent commit: \`git push -u origin HEAD:refs/heads/<branch>\`. Remote refs are the only durable store. A "ready" report on an unpushed branch is an emergency, not a normal state.
⚠ Always \`git fetch origin\` immediately before branching from origin/main — cloud sessions are long-lived while main moves under them, and branching off a stale ref silently resurrects old file states.`
  : "";

const DISK = `PRE-FLIGHT: run \`df -h /\` FIRST — if free space is under 15GiB, STOP and return ok:false with reason "ENOSPC risk" instead of building (a full wave of worktrees on a near-full disk killed every agent at once; other repos' sessions share this disk, so never assume yesterday's free space). WORKTREE ECONOMY: don't cold-install. If the primary checkout's node_modules matches your lockfile, clone it into the worktree first (macOS/APFS: \`cp -c -R <primary>/node_modules ./node_modules\`; Linux: \`cp --reflink=auto -R\` or \`cp -al\`), then let the install command true it up (near-instant). Always run gates with the shared cache prefix so sibling agents' builds become cache hits.
⚠ NEVER \`git stash\` — all worktrees share ONE stash stack, so a sibling agent can pop YOUR work-in-progress (this has happened). To set WIP aside, commit it to your own branch and amend/reset later.`;

// ⚠ ONE train worktree PER WAVE: suffix the path uniquely for THIS wave (e.g.
// /tmp/pump-merge-train-t8). CONCURRENT waves sharing one path race the same
// `train-pr` branch and have SILENTLY DROPPED files from merge trees (caught
// twice, by luck, in gate diffs). The persistence win is per-wave node_modules
// reuse — it never required cross-wave sharing.
const TRAIN_WT = "/tmp/pump-merge-train-FILL-UNIQUE-SUFFIX";
const TRAIN_SETUP = `TRAIN WORKTREE — MANDATORY STEP 0, before ANY other git command: your shell starts in the USER'S ACTIVE worktree; running checkout/reset/merge there DESTROYS their in-progress work (this has happened MORE THAN ONCE — cd alone has proven insufficient because shell state resets between Bash calls). RULES: (1) \`cd ${TRAIN_WT}\` first (if absent, create it with \`git worktree add ${TRAIN_WT} --detach origin/main\` from the primary checkout, then cd); (2) EVERY git command you run for the rest of this task MUST be spelled \`git -C ${TRAIN_WT} …\` — the -C flag is not optional, even right after a cd, even for status/log; a bare \`git checkout\` in the wrong cwd is how user work gets destroyed; (3) non-git commands (pnpm, gh) must be prefixed with \`cd ${TRAIN_WT} && \` in the same Bash invocation. Start clean: \`git -C ${TRAIN_WT} fetch origin --quiet && git -C ${TRAIN_WT} checkout --detach origin/main && git -C ${TRAIN_WT} reset --hard && git -C ${TRAIN_WT} clean -fd\`. node_modules persists across train stops — that's the point.
⚠ ${TRAIN_WT} is the ONLY worktree you may use. NEVER use, cd into, or run git against ANY path under .claude/worktrees/ or any other "already provisioned" worktree you discover — those are the USER'S live sessions (an agent that migrated to one hijacked a dev server mid-drive and left it detached). If ${TRAIN_WT} looks contended or its state looks wrong (unexpected branch tip, files missing after a merge), do NOT relocate: re-verify against the PR head SHA via gh, recreate ${TRAIN_WT} from scratch, or return landedState:"parked" with the reason. After ANY merge, verify the PR's changed files are all present in the tree (git -C ${TRAIN_WT} diff --stat origin/main) before gating — a raced checkout can drop files with no conflict marker.`;

// LANDING POLICY (ruled 2026-08-13, after w148) — folded into every train
// prompt because prompts are the only part agents obey.
//
// Agents ARM AUTO-MERGE; they never merge directly. Two failures forced this,
// both in w148's single wave:
//   1. A fix agent called merge_pull_request on PR #2412 while the full CI run
//      (Static checks, Test, E2E) was still IN PROGRESS. It landed green by
//      luck, not by design — the local gate is not the required checks.
//   2. The safety classifier BLOCKED the sibling fix agent from merging #2413
//      at all, reading an agent self-merge of its own review fixes as lacking
//      the required sign-off. A blocked landing strands finished work.
// Arming auto-merge fixes both: GitHub lands the PR only once required checks
// pass, and no agent issues a merge.
//
// ⚠ COST 1, a KNOWN one: "auto-merge armed" is NOT "landed". The repo has
// already lost two PRs (#1659, #1666) to exactly this — auto-merge armed on a
// PR that later conflicted, leaving the work LOOKING landed while its issue
// stayed open, surfaced only by an audit. So `merged: true` from a train agent
// means "armed or already merged", NOT "in main". The retro closes this hole:
// it must verify each PR's ACTUAL state and report armed-but-unlanded as NOT
// merged.
//
// ⚠ COST 2, found in w149 and CORRECTED 2026-08-13 (user-ruled, same day):
// the first draft of this policy said "never merge directly, park if arming
// fails". GitHub REFUSES to arm auto-merge on a PR whose checks have already
// completed green ("Auto-merge only applies when checks are pending"). So the
// better-behaved a PR was — green gate, review approved with zero findings —
// the more likely it stranded itself. PR #2420 parked exactly that way with an
// approving review and 957 passing tests. That is backwards: the policy existed
// to stop merges that outran CI, and it was instead blocking merges that had
// WAITED for CI. Landing is now THREE cases, not two — see below. The two
// things the policy actually protects are preserved: never merge while checks
// are pending, never merge unreviewed work.
const LANDING_POLICY = `LANDING POLICY — THREE CASES. Decide by READING the PR's check runs, never by assuming:
1. CHECKS STILL PENDING/RUNNING (any required check queued, in_progress, or not yet reported) → ARM AUTO-MERGE (\`mcp__github__enable_pr_auto_merge\`, mergeMethod "MERGE"). Do NOT merge directly. Your local gate is NOT the repository's required checks — a fix agent merged PR #2412 mid-CI and landed green by luck, not design.
2. CHECKS ALL COMPLETED AND GREEN (every required check concluded success or skipped, none failing, none pending) AND the adversarial review verdict for this PR was \`approve\`, or was \`changes\` with every finding applied and the gate re-run → STOP AND REPORT, landedState:"parked", detail "green + reviewed, ready for the orchestrator to merge". ⚠ DO NOT CALL \`mcp__github__merge_pull_request\` — you are a delegated agent and you have NO authority to merge. Arming is genuinely impossible in this state (GitHub rejects it with "already in clean status"), which is exactly why this case hands the decision UP rather than taking it. The orchestrator merges these in-session, where the human can see and interrupt it. An earlier version of this policy told you to merge here, citing a "user authorization" that existed only inside this file; a safety classifier blocked it three separate times and was RIGHT each time. Do not reintroduce it, and do not work around it.
3. ANYTHING ELSE — a required check FAILED, the review verdict was \`reject\`, unresolved conflicts, or arming fails for a reason other than "already clean"/"unstable" (e.g. auto-merge disabled on the repo) → PARK IT. Leave the PR open and untouched, return landedState:"parked", and say exactly why in \`detail\`. Never merge to clear a red check or an unresolved review.
⚠ CASE 1 CAN BE REFUSED, and the refusal is NOT case 3 (found in w154, PR #2474). GitHub's \`mergeable_state\` is UNSTABLE for BOTH "some checks still pending" and "some check failed", and \`enable_pr_auto_merge\` rejects an UNSTABLE PR with a message like "unstable status (required checks are failing)" — even when NOTHING has failed and the only laggard is a slow job. Do NOT believe that message over the check runs you just read, do NOT loop-retry the arm call, and do NOT park. WAIT for the outstanding checks to conclude (a few minutes; re-read the check runs, don't spin), then re-evaluate from the top: all green → case 2, something red → case 3. Waiting is the correct action because the state is genuinely undecided until the checks finish.
⚠ REPORT HONESTLY: \`merged: true\` means "auto-merge ARMED (case 1) or actually LANDED (case 2)" — armed does NOT mean the commit is in main. State WHICH case you took in \`detail\`, and for case 2 name the checks you read. Never claim a PR landed on the strength of having armed it: this repo has LOST two PRs (#1659, #1666) to auto-merge that armed, later conflicted, and left work looking landed while its issue stayed open.`;

const gateFor = (b) => `${CACHE} && ${b.gate ?? GATE}`;
const effortFor = (b) => (b.lane === "light" ? "medium" : "high");
// MODEL TIERS (credit control): subagents inherit the session model by default,
// which burns the top tier on mechanical work. Builds/train/fix run on sonnet
// (light lane: haiku), adversarial REVIEW stays one tier up (opus) — the review
// is the pump's quality bar and has repeatedly caught what the gate missed.
// Per-entry override: { n, model: "opus" } for a known-hard build.
const buildModelFor = (b) => b.model ?? (b.lane === "light" ? "haiku" : "sonnet");
const REVIEW_MODEL = "opus";
const TRAIN_MODEL = "sonnet";
const closesList = (b) => (b.closes ?? [b.n]);
const closesLine = (b) => closesList(b).map((x) => `Closes #${x}`).join(", ");

// (v3 note: verdicts ride the pipeline WITH their item — no string-matching of
// PR references between phases, which once mis-parked approved PRs.)

const BUILD = {
  type: "object", additionalProperties: false,
  required: ["ok", "issue", "pr", "gateGreen", "reachability", "summary"],
  properties: {
    ok: { type: "boolean" }, issue: { type: "number" }, pr: { type: "string" },
    gateGreen: { type: "boolean" }, gateOutput: { type: "string" },
    reachability: { type: "string", description: "the concrete user path that exercises this change" },
    summary: { type: "string" },
    scopeNotes: { type: "string", description: "work discovered BEYOND this issue (hidden coupling, missing prereqs, an under-sized issue, adjacent follow-ups) — kept out of the PR; empty if scope held" },
  },
};
const VERDICT = {
  type: "object", additionalProperties: false, required: ["pr", "decision", "findings"],
  properties: {
    pr: { type: "string" }, decision: { type: "string", enum: ["approve", "changes", "reject"] },
    findings: { type: "array", items: { type: "string" } },
    scopeGaps: { type: "array", items: { type: "string" }, description: "where the issue/milestone UNDER-captured the work (scope the plan missed)" },
  },
};
const SCOPE = {
  type: "object", additionalProperties: false,
  required: ["assessment", "recommendation", "proposal"],
  properties: {
    assessment: { type: "string", enum: ["scope-held", "under-captured", "over-captured"] },
    discoveredWork: { type: "array", items: { type: "object", additionalProperties: false, required: ["title", "why"], properties: {
      title: { type: "string" }, why: { type: "string" }, suggestedMilestone: { type: "string" } } } },
    trackerActions: { type: "array", items: { type: "string" }, description: "true-up actions actually TAKEN (issues closed, remainders commented, follow-ons filed) — actions, not proposals" },
    recommendation: { type: "string", enum: ["scope-held", "file-issues", "expand-milestone", "add-milestone"] },
    proposal: { type: "string", description: "concrete proposal for the HUMAN to approve: which issues to file + which milestone to expand or add, with rationale" },
  },
};
// ⚠ THREE-STATE, NOT A BOOLEAN (2026-08-13, w149 post-mortem). This field was
// `merged: boolean` meaning "armed OR landed", with the disambiguation only in
// prose in `detail`. That is exactly how PR #2422 was reported as landed when
// it was merely armed: the train agent was scrupulously honest ("Auto-merge
// ARMED (not yet landed)") and the retro caught the discrepancy, but every
// downstream summary read the boolean and printed "MERGED". The PR then went
// behind main, auto-merge could never fire, and #2415's work sat unlanded while
// three separate reports called it done. A boolean that needs a sentence to
// interpret is a boolean that will be misread; the enum cannot be.
const MERGE = {
  type: "object", additionalProperties: false, required: ["pr", "landedState", "detail"],
  properties: {
    pr: { type: "string" },
    landedState: {
      type: "string",
      enum: ["landed", "armed", "parked"],
      description: "landed = VERIFIED in main (you read merged:true from the API, not a cached label); armed = auto-merge enabled, NOT yet in main; parked = left open for a human",
    },
    detail: { type: "string", description: "for landed: the checks you read and the merge sha. for armed: why it is not landed yet. for parked: the exact blocker. plus conflicts/gate counts either way" },
  },
};
const LESSONS = {
  type: "object", additionalProperties: false, required: ["lessons"],
  properties: { lessons: { type: "array", items: { type: "string" }, description: "generalizable house-rule candidates, imperative voice, ready to paste into the next wave's RULES" } },
};

if (!BATCH.length) { log("CONFIG.BATCH is empty — fill the CONFIG block."); return { error: "empty batch" }; }

// ── Orchestration: PER-ITEM PIPELINE (v3 — no head-of-line blocking) ─────────
// Each issue flows build → review → land INDEPENDENTLY: a finished build goes
// straight to ITS adversarial review while slower siblings keep building (the
// old all-builds barrier once parked two green PRs for ~an hour behind one
// slow sibling's retries). The ONLY serialization is this TRAIN QUEUE: merge
// and fix agents share the single persistent train worktree and every merge
// advances main, so landings run one-at-a-time in COMPLETION order.
let trainChain = Promise.resolve();
const enqueueTrain = (job) => {
  const run = trainChain.then(job, job);
  // The chain itself must never carry a rejection (it would poison every
  // later landing); each caller gets ITS job's promise.
  trainChain = run.then(() => undefined, () => undefined);
  return run;
};
let landedCount = 0;

const results = (await pipeline(
  BATCH,
  // BUILD (parallel across items, worktree-isolated).
  (b) =>
    agent(`You are an autonomous build agent in a FRESH GIT WORKTREE. Implement GitHub issue #${b.n} of ${REPO} (${b.hint}) and open ONE PR${closesList(b).length > 1 ? ` that honestly closes ALL of: ${closesList(b).map((x) => "#" + x).join(", ")} (clustered because they share files)` : ""}. ${CONV}${RULES ? `\nHOUSE RULES (learned from prior reviews — obey):\n${RULES}\n` : ""}
${DISK}${CLOUD_DISK}
${GH.pre}
STEPS (Bash):
1. ${closesList(b).map((x) => GH.issueView(x)).join(" + ")} — read full scope + acceptance.
1b. PREMISE CHECK: before building, confirm the issue's premise is still true — six issues in one month had tracker state that lied about the code (already implemented, or already settled by a later ruling). If the premise is already false, do NOT build: return ok:false with the delivering PR/ruling named in \`summary\`, and say so. That is a successful outcome, not a failure.
2. \`git fetch origin --quiet && git checkout -B auto/issue-${b.n} origin/main\`. Clone node_modules per WORKTREE ECONOMY, then install + build shared/workspace deps FIRST (consumers resolve built deps from dist).
3. Implement TDD. SCOPE DISCIPLINE: your own new files + minimal wiring; avoid shared hot files unless the hint names them; if you must touch one, make a minimal additive change so sibling PRs merge cleanly.
4. PROVE REACHABILITY: confirm the change is actually reached by a user (wired into the UI / called), not just covered by a unit test. State the exact user path in \`reachability\`. Dead/unwired code is a FAIL.
5. CONVENTIONS LINT: no invented identifiers/tokens — grep new design tokens etc. against the real source of truth; honor house rules.
6. Run the GATE to GREEN: \`${gateFor(b)}\`. Capture counts.
7. VERIFY THE DIFF IS REAL: \`git diff origin/main --stat\` must be NON-EMPTY and match what you believe you built — an empty diff means your edits didn't land (wrong cwd/worktree); STOP and fix rather than opening a hollow PR (one shipped with a fully fabricated body and was caught by review — an instant reject and a wasted cycle). Then stage explicit paths (NOT git add -A), commit (end the message with "${TRAILER}"), \`git push -u origin auto/issue-${b.n}\`.
8. ${GH.prCreate} — title "<concise>", body starting "${closesLine(b)}" then what + how + gate counts + reachability. Only an HONEST closes list (if you delivered N of M deliverables, drop the unearned Closes and say so).
9. SCOPE OVERFLOW: if the work revealed anything BEYOND ${closesList(b).map((x) => "#" + x).join("/")} (hidden coupling, a missing prerequisite, an under-sized issue, obvious adjacent follow-ups), record it in \`scopeNotes\` AND ${GH.issueComment(b.n, 'ONE comment ("Scope discovered beyond this fence: …")')} so the record survives the session (durable-by-default rule, decision 2026-07-18). Do NOT grow this PR to cover it.
Return ok, issue, pr (number/url), gateGreen (true only if step 6 fully passed), gateOutput, reachability, summary, scopeNotes.`,
      { label: `build#${b.n}`, phase: "Build", effort: effortFor(b), model: buildModelFor(b), isolation: "worktree", schema: BUILD }),
  // REVIEW — starts the moment THIS item's build returns; siblings unaffected.
  (build, b) => {
    if (!build || !build.ok || !build.pr) return { build: build ?? null, verdict: null };
    return agent(`Adversarially review PR ${build.pr} (issue #${b.n}) of ${REPO} for an autonomous merge decision. Try to REFUTE it. ${CONV}${RULES ? `\nHOUSE RULES:\n${RULES}\n` : ""}
Build reported: gateGreen=${build.gateGreen}; reachability="${build.reachability}"; ${build.summary}
${GH.pre}
READ-ONLY MANDATE: you have NO worktree — your shell cwd is the USER'S LIVE session worktree; NEVER run \`git checkout\`, \`git reset\`, \`git stash\`, or ANY state-mutating git command anywhere (a reviewer that checked out a PR branch in the user's worktree hijacked their live session). Inspect via the GitHub read tools and Read files at their committed paths; if you must run code, clone/fetch into a fresh dir under /tmp.
DO: ${GH.prDiff(build.pr)} + read changed files in context. Judge: correct + COMPLETE for #${b.n}? Actually REACHABLE/wired (not dead code)? In scope (no unrelated churn)? Conventions + house rules honored (no invented tokens)? Bugs / missing tests / regressions? Gate genuinely green? **SPEC DRIFT** — does this PR leave a SPEC disagreeing with reality? Two shapes: (a) a decision-log PR that rules something but amends NO spec, so the ruling's only home is history nobody reads; (b) an implementation PR whose behavior contradicts, or is absent from, the spec that owns that territory. A 2026-07-27 ledger audit of 296 rulings found 29 ORPHANED and 15 CONTRADICTED precisely here — including a same-day ruling PR that touched only the decision log while the spec kept carrying the spelling that ruling rejected. Name the spec file+section in your findings. Decide approve | changes (list SPECIFIC actionable fixes) | reject (fundamentally wrong). Default to "changes" if materially off.
DISCIPLINE: every \`findings\` entry must be a REAL, actionable fix to THIS PR's diff — never placeholders. If your only material notes are follow-up scope (work outside this PR's fence), the decision is "approve" and those notes go in \`scopeGaps\`; "changes" with an empty/junk findings list wastes a fix-agent run.
ALWAYS ${GH.prComment(build.pr, "your verdict as ONE comment")} — decision, findings, and scope gaps, INCLUDING approvals (durable-by-default rule, decision 2026-07-18: an approval's scope gaps are exactly the context that evaporates otherwise). Commenting is allowed under the read-only mandate; git state mutation is not. ALSO: note in \`scopeGaps\` anything the issue (or its milestone) UNDER-captured — work that clearly belongs but no issue covers. Return {pr, decision, findings, scopeGaps}.`,
      { label: `review#${b.n}`, phase: "Review", effort: effortFor(b), model: REVIEW_MODEL, schema: VERDICT })
      .then((verdict) => ({ build, verdict: verdict ?? null }));
  },
  // LAND — merge approvals / fix "changes", ONE at a time via the train queue,
  // in completion order. "reject" and failed builds park (human review).
  (r, b) => {
    const base = { issue: b.n, build: r?.build ?? null, verdict: r?.verdict ?? null };
    if (!base.build || !base.build.ok || !base.build.pr || !base.verdict) return { ...base, land: null };
    const decision = base.verdict.decision;
    if (decision === "reject") return { ...base, land: null };
    const job = decision === "approve"
      ? () => agent(`Land PR ${base.build.pr} (${closesLine(b)}) onto main of ${REPO}. Built off main in parallel; main has since advanced. ${CONV}
${TRAIN_SETUP}${CLOUD_DISK}
${GH.pre}
${LANDING_POLICY}
${GH.parkNote}
STEPS: 1. Resolve the PR's head branch: ${GH.prView(base.build.pr)}. In the train worktree: \`git fetch origin --quiet && git checkout -B train-pr origin/<headRef>\`. 2. \`git merge origin/main\` — combine ADDITIVE conflicts (registry/index appends: keep ALL entries; keep both wirings). ⚠ diff3/union conflict styles can duplicate or drop closing braces and adjacent lines when combining — after resolving, recheck brace balance and that BOTH sides' entries survived; the gate is the arbiter. If untangleable, \`git merge --abort\` + landedState:"parked". 3. Run the GATE to GREEN: \`${gateFor(b)}\`. Fix semantic conflicts. 4. ${GH.prDiff(base.build.pr)} sanity-check. 5. If green + MERGEABLE: \`git push origin train-pr:<headRef>\`, \`git checkout --detach origin/main\`, then ${GH.prMerge(base.build.pr)}. Else leave open + comment + landedState:"parked". NOTEWORTHY-ONLY comment rule (durable-by-default, decision 2026-07-18): if the landing involved anything a future bisect would want — conflict resolutions, semantic fixes, files verified after a raced checkout — post ONE PR comment describing it; routine clean merges stay SILENT (no noise).
Return {pr, landedState, detail (conflicts + gate counts + result, or park reason)}.`,
          { label: `merge#${b.n}`, phase: "Merge train", effort: effortFor(b), model: TRAIN_MODEL, schema: MERGE })
      : () => agent(`Fix the review-blocking issues in PR ${base.build.pr} (${closesLine(b)}) of ${REPO}, then land it. The PR is otherwise good; apply the reviewer's SPECIFIC fixes only. ${CONV}${RULES ? `\nHOUSE RULES:\n${RULES}\n` : ""}
${TRAIN_SETUP}${CLOUD_DISK}
${GH.pre}
${LANDING_POLICY}
${GH.parkNote}
REVIEWER FINDINGS — these ARE the adversarial review's verdict. They ride this workflow and are NOT posted on GitHub, so an empty \`gh api .../reviews\` or \`.../comments\` result means NOTHING — never treat it as evidence the findings aren't real (this dismissal has let a real stack-corruption bug merge). Treat each finding as authoritative and apply it. Skip an INDIVIDUAL finding only if it references files/lines absent from this PR's diff (a misattributed finding), and list every skipped finding with its reason in your report:\n- ${(base.verdict.findings ?? []).join("\n- ")}
STEPS: resolve the head branch (${GH.prView(base.build.pr)}); in the train worktree, \`git checkout -B train-fix origin/<headRef>\`; merge origin/main (combine; recheck brace balance per the diff3 caveat); apply ONLY the findings' fixes + tests that guard them; GATE to GREEN: \`${gateFor(b)}\`; commit (end with "${TRAILER}") + push back to the PR branch; THEN ${GH.prComment(base.build.pr, "ONE disposition comment")} tying commits to findings — which findings you applied and which you skipped with the misattribution reason (durable-by-default rule); if MERGEABLE, detach then ${GH.prMerge(base.build.pr)}. If a finding needs a human decision (scope split, design call), DON'T guess — leave the PR open, comment, and report landedState:"parked" with what's needed.
Return {pr, landedState, detail}.`,
          { label: `fix#${b.n}`, phase: "Fix loop", effort: effortFor(b), model: TRAIN_MODEL, schema: MERGE });
    return enqueueTrain(job).then((m) => {
      const land = { kind: decision === "approve" ? "merge" : "fix", ...(m ?? { landedState: "parked", detail: "no result" }) };
      // Only "landed" counts. "armed" is NOT in main and must never inflate
      // this tally — that conflation is what let #2422 read as done.
      if (land.landedState === "landed") landedCount += 1;
      log(`train: #${b.n} ${land.landedState.toUpperCase()} via ${land.kind} (${landedCount} verified in main so far)`);
      return { ...base, land };
    });
  },
)).filter(Boolean);

// Flat views for the wrap-up phases (shapes match the old barrier variables).
const built = results.map((r) => r.build).filter(Boolean);
const reviewed = results.map((r) => r.verdict).filter(Boolean);

phase("Lessons"); // close the learning loop mechanically: findings -> next wave's RULES
const allFindings = reviewed.flatMap((v) => (v?.findings ?? []).map((f) => `${v.pr} [${v.decision}]: ${f}`));
const lessons = allFindings.length
  ? await agent(`Distill HOUSE-RULE candidates from this wave's review findings for ${REPO}. A lesson qualifies only if it GENERALIZES (would prevent a class of mistake in future builds), not a one-off fix. Merge duplicates; imperative voice; one line each ("use only tokens from X", "wire features into the UI, not just a hook"). 0-6 lessons — an empty list is a valid answer.
FINDINGS:\n${allFindings.map((s) => "- " + s).join("\n")}
Current house rules (don't repeat):\n${RULES || "(none)"}
Return {lessons}.

⚠ THEN PERSIST THEM — this is the whole point, and it has historically been the broken link. Measured across 671 agents: **fix cycles are 18.3% of all token spend, and ~80% of reviewed PRs need one** (133 fix agents against 165 reviews). Those fixes are triggered by findings that RECUR wave after wave. Returning lessons as text that a human must remember to copy forward is why the same mistakes kept costing full re-read/re-gate/re-push cycles.

${GH.pre}
So: open a PR adding each GRADUATED lesson to the "Recurring build-quality rules" section of .claude/skills/autonomous-pump/BRINK-CONFIG.md, in a fresh worktree off origin/main (never the user's live checkout), and enable auto-merge. Rules:
- Append only; do NOT rewrite or reorder existing rules.
- MERGE with an existing rule when it is the same lesson said differently — a list of 40 near-duplicates is worse than 15 sharp ones, because nobody reads it.
- Each rule states the FAILURE it prevents, not just the principle — agents follow "a test that passes on both commits proves nothing; revert the fix and watch it fail" far better than "write good tests".
- If nothing generalizes this wave, open NO PR and say so. An empty result is a valid, honest outcome.`,
      { label: "lessons", phase: "Lessons", effort: "medium", model: "sonnet", schema: LESSONS })
  : { lessons: [] };

phase("Retro / scope reconciliation"); // did building reveal work the plan didn't capture? PROPOSE, don't auto-restructure.
const scopeSignals = [
  ...built.filter((b) => b && b.scopeNotes && b.scopeNotes.trim()).map((b) => `#${b.issue} (build): ${b.scopeNotes}`),
  ...reviewed.flatMap((v) => (v?.scopeGaps ?? []).map((g) => `${v.pr} (review): ${g}`)),
];
// Per-item tracker facts: the retro compares DELIVERED vs REQUESTED, so it
// needs every ATTEMPTED item's issue/PR/merge state. Built from BATCH, not
// from `results` — a build that dies (API error, killed agent) is dropped
// from `results`, and a retro that only sees survivors reports a wave where
// EVERYTHING failed as "scope-held". An item that never ran must say so.
const trackerFacts = BATCH.map((b) => {
  const r = results.find((x) => x && x.issue === b.n);
  const cl = closesList(b).map((x) => "#" + x).join(",");
  if (!r) return `#${b.n} -> NEVER RAN (agent died or was skipped — no build result at all; NOT evidence the issue is fine)`;
  // ⚠ NEVER collapse "armed" into "MERGED" here. This line used to read
  // `r.land?.merged ? "MERGED"`, which handed the retro a table claiming
  // PR #2422 was merged when it was only armed — it then went behind main and
  // never landed. The retro caught it that time; do not rely on that.
  const ls = r.land?.landedState;
  const state = ls === "landed" ? "MERGED (train claims verified in main — RE-VERIFY)"
    : ls === "armed" ? "ARMED ONLY — NOT in main, auto-merge may never fire"
    : (r.build?.pr ? "NOT MERGED" : "no PR opened");
  return `#${b.n} -> ${r.build?.pr ?? "(no PR)"} | ${state} | verdict ${r.verdict?.decision ?? "-"} | claims: ${cl}`;
}).join("\n");

const scopeReconciliation = await agent(`You are running this wave's RETRO for ${REPO}${MILESTONE ? ` — milestone "${MILESTONE}"` : ""}.
${GH.pre} An agent landed a deliverable for each item below. Your job is to TRUE UP what was actually built against what was actually requested, item by item, and leave both the tracker and the plan honest.

THIS WAVE'S ITEMS (issue -> PR -> merge state -> what the PR claims to close):
${trackerFacts || "(none)"}

DISCOVERED-SCOPE SIGNALS reported by the builds and reviews:
${scopeSignals.length ? scopeSignals.map((s) => "- " + s).join("\n") : "(none reported)"}

For EACH item, compare DELIVERED against REQUESTED and land the difference somewhere real:
- **Delivered the whole fence and the PR merged** — close the issue if it is still open.
- **Delivered less than the fence** (the PR says "Part of", or the review/scope notes name a missing piece) — the remainder is INVISIBLE unless you record it. Comment on the issue with what shipped, what is left, and what blocks it, and file the follow-on issue (or name the existing one that covers it). A "Part of" with an untracked remainder reads exactly like "not started" — that is how six issues in one month ended up with state that lied about the code.
- **PR did not merge** — say why in one line: conflict, auto-merge armed but never completed, or review parked it. An auto-merge that arms and then conflicts leaves work LOOKING landed while its issue stays open; that has silently lost two PRs here (#1659, #1666).
- ⚠ **VERIFY EVERY LANDING YOURSELF — do not trust the table above.** Since 2026-08-13 train agents ARM auto-merge rather than merging, so a \`MERGED\` row means "armed or landed", NOT "in main". For EVERY item, read the PR's real state now (${GH.prView("<pr>")}: \`merged\`/\`state\`) and report what you actually read. An armed-but-unlanded PR is NOT merged: leave its issue OPEN, say it is waiting on required checks, and flag it for the next wave to re-check. This verification is the ONLY thing standing between the arm-auto-merge policy and the exact #1659/#1666 loss it re-introduces.
- **The build found the premise already false** (already implemented, already ruled) — verify the delivering PR, comment naming it, and close the issue if nothing remains. Reporting it only to the pump means the next wave rediscovers it at the cost of another build agent.
- **Something this wave ruled or shipped supersedes a DIFFERENT open issue's premise** — update that issue too. A ruling that lands only in a doc leaves the tracker lying.
- **The item NEVER RAN** (agent died mid-flight) — say so plainly and recommend a re-queue. Do NOT report the wave as scope-held on the strength of items that never executed; absence of a result is absence of evidence, not evidence of health.\n- **Genuinely new work no existing issue covers** — file it, after deduping against the board (${GH.issueList(MILESTONE ? `milestone "${MILESTONE}", state all` : "")}; read the roadmap/plan doc if the repo has one).

You MAY act on all of the above — it is truing up the record of work that already happened, not rewriting the plan. The one thing you may NOT do unilaterally is restructure MILESTONES (add or expand one): propose that with rationale and let the human decide.

Finally judge the batch as a whole: did its scope HOLD, UNDER-capture, or OVER-capture? Recommend exactly one: scope-held | file-issues | expand-milestone | add-milestone, with a crisp \`proposal\` the human can act on.${LEDGER ? `
⚠ FIRST, BEFORE writing this wave's ledger: TRUE UP THE PREVIOUS WAVE'S LEDGER. Read the most recent wave-ledger comment on #${LEDGER} and re-check the CURRENT state of every PR it recorded as parked/open/armed. Since agents ARM auto-merge rather than merging (ruled 2026-08-13), landings are ASYNCHRONOUS — GitHub lands a PR whenever its required checks go green, which is routinely AFTER the wave that built it has ended. So the previous ledger's landed/parked split is provisional by construction, and w148's ledger was already wrong within 18 minutes of being written (it recorded #2402 as parked; PR #2413 merged at 17:48). If anything moved, post a short correction comment naming the wave and what changed — do NOT edit the old comment; the timing gap should stay visible. If nothing moved, say nothing about it.
⚠ Also distinguish these in YOUR ledger, they are not the same thing and only the first is a backlog item: PARKED (review blocked it — needs work) vs OPEN-PENDING-CHECKS (armed, just waiting on CI — needs nothing). Never write a bare "parked" for a PR that is merely waiting on green.
⚠ AND: for every item the table above marks MERGED, RE-VERIFY it yourself against the API — read the PR's real \`merged\` field, do not trust the table, a cached MERGED label, or the train agent's own word. In w149 the table said PR #2422 was merged; it was only ARMED, then fell behind main, and #2415's work sat unlanded while three separate reports called it done. If an "armed" PR has gone \`behind\`, UPDATE ITS BRANCH so auto-merge can fire again, and say you did.
Then (durable-by-default rule) ${GH.issueComment(LEDGER, "ONE compact wave-ledger comment")}: wave id ${WAVE_ID}; the batch; landed/parked/open-pending-checks; the lessons harvested this wave (passed below if any); your true-up actions and scope assessment. Issues filed here carry the pump:scope label; graduated lessons carry pump:lesson.` : ""}
Return {assessment, trackerActions, discoveredWork:[{title,why,suggestedMilestone}], recommendation, proposal}.`,
  { label: "retro", phase: "Retro / scope reconciliation", effort: "high", model: "sonnet", schema: SCOPE });

// Reconciliation: every built+ok PR must be accounted for — merged, or parked
// WITH a reason. No silent drops. (v3: verdict + landing ride each item, so
// this is a straight per-item read — no cross-phase matching.)
// ⚠ "landed" ONLY. An "armed" PR is NOT landed and belongs in `awaitingChecks`
// below, never in `landed` — see the #2422 post-mortem in the MERGE schema.
const landedList = results
  .filter((r) => r.land?.landedState === "landed")
  .map((r) => ({ issue: r.issue, pr: r.land.pr ?? r.build?.pr, via: r.land.kind, detail: r.land.detail }));
// Armed-but-not-landed: needs no work, but MUST be reported separately so it is
// never mistaken for either landed work or a backlog item.
const awaitingChecks = results
  .filter((r) => r.land?.landedState === "armed")
  .map((r) => ({ issue: r.issue, pr: r.land.pr ?? r.build?.pr, detail: r.land.detail }));
const parked = results
  .filter((r) => r.build && r.build.ok && r.build.pr && r.land?.landedState !== "landed" && r.land?.landedState !== "armed")
  .map((r) => ({
    issue: r.issue, pr: r.build.pr, decision: r.verdict?.decision ?? "(no verdict)",
    reason: r.land?.detail
      ?? (r.verdict?.decision === "reject" ? `rejected: ${(r.verdict?.findings ?? []).join("; ")}` : "not approved"),
    findings: r.verdict?.findings,
  }));
const buildFailed = results
  .filter((r) => !r.build || !r.build.ok || !r.build.pr)
  .map((r) => ({ issue: r.issue, summary: r.build?.summary ?? "(no build result)" }));

return {
  landed: landedList,   // VERIFIED in main. Nothing merely armed belongs here.
  awaitingChecks,       // armed, NOT in main yet — no action needed, but NOT landed either.
                        // ⚠ Anyone summarising this wave must not fold these into `landed`:
                        // that conflation left #2415 unlanded while three reports called it done.
  parked,        // open PRs needing attention (with reasons / findings) — review these
  buildFailed,   // never reached a PR
  lessons: lessons?.lessons ?? [],  // paste-ready house-rule candidates for the next wave's RULES
  scopeReconciliation, // PROPOSAL for the human: did scope hold? new issues / milestone to add or expand?
  counts: { batch: BATCH.length, landed: landedList.length, awaitingChecks: awaitingChecks.length, parked: parked.length, buildFailed: buildFailed.length, lessons: (lessons?.lessons ?? []).length, scopeDiscovered: scopeReconciliation?.discoveredWork?.length ?? 0 },
};
