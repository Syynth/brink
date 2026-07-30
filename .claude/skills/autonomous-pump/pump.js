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
// Gate: build shared deps -> test -> typecheck. CACHE is prepended to every
// gate invocation so all agents share one build cache (turbo/nx: unchanged
// packages become cache hits — this is the pump's single biggest speed lever).
// CARGO_INCREMENTAL=0: every build agent works in a FRESH worktree, so the
// incremental dep-graph cache is written and never read — pure disk write for
// zero benefit, multiplied by ~5 agents per wave across thousands of builds.
// (Local human iteration is unaffected; this only applies to agent gates.)
const CACHE = "export TURBO_CACHE_DIR=/tmp/pump-turbo-cache CARGO_INCREMENTAL=0";
const GATE = "pnpm install --prefer-offline && pnpm turbo run test typecheck";
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

const DISK = `PRE-FLIGHT: run \`df -h /\` FIRST — if free space is under 15GiB, STOP and return ok:false with reason "ENOSPC risk" instead of building (a full wave of worktrees on a near-full disk killed every agent at once; other repos' sessions share this disk, so never assume yesterday's free space). WORKTREE ECONOMY: don't cold-install. If the primary checkout's node_modules matches your lockfile, clone it into the worktree first (macOS/APFS: \`cp -c -R <primary>/node_modules ./node_modules\`; Linux: \`cp --reflink=auto -R\` or \`cp -al\`), then let the install command true it up (near-instant). Always run gates with the shared cache prefix so sibling agents' builds become cache hits.
⚠ NEVER \`git stash\` — all worktrees share ONE stash stack, so a sibling agent can pop YOUR work-in-progress (this has happened). To set WIP aside, commit it to your own branch and amend/reset later.`;

// ⚠ ONE train worktree PER WAVE: suffix the path uniquely for THIS wave (e.g.
// /tmp/pump-merge-train-t8). CONCURRENT waves sharing one path race the same
// `train-pr` branch and have SILENTLY DROPPED files from merge trees (caught
// twice, by luck, in gate diffs). The persistence win is per-wave node_modules
// reuse — it never required cross-wave sharing.
const TRAIN_WT = "/tmp/pump-merge-train-FILL-UNIQUE-SUFFIX";
const TRAIN_SETUP = `TRAIN WORKTREE — MANDATORY STEP 0, before ANY other git command: your shell starts in the USER'S ACTIVE worktree; running checkout/reset/merge there DESTROYS their in-progress work (this has happened MORE THAN ONCE — cd alone has proven insufficient because shell state resets between Bash calls). RULES: (1) \`cd ${TRAIN_WT}\` first (if absent, create it with \`git worktree add ${TRAIN_WT} --detach origin/main\` from the primary checkout, then cd); (2) EVERY git command you run for the rest of this task MUST be spelled \`git -C ${TRAIN_WT} …\` — the -C flag is not optional, even right after a cd, even for status/log; a bare \`git checkout\` in the wrong cwd is how user work gets destroyed; (3) non-git commands (pnpm, gh) must be prefixed with \`cd ${TRAIN_WT} && \` in the same Bash invocation. Start clean: \`git -C ${TRAIN_WT} fetch origin --quiet && git -C ${TRAIN_WT} checkout --detach origin/main && git -C ${TRAIN_WT} reset --hard && git -C ${TRAIN_WT} clean -fd\`. node_modules persists across train stops — that's the point.
⚠ ${TRAIN_WT} is the ONLY worktree you may use. NEVER use, cd into, or run git against ANY path under .claude/worktrees/ or any other "already provisioned" worktree you discover — those are the USER'S live sessions (an agent that migrated to one hijacked a dev server mid-drive and left it detached). If ${TRAIN_WT} looks contended or its state looks wrong (unexpected branch tip, files missing after a merge), do NOT relocate: re-verify against the PR head SHA via gh, recreate ${TRAIN_WT} from scratch, or return merged:false with the reason. After ANY merge, verify the PR's changed files are all present in the tree (git -C ${TRAIN_WT} diff --stat origin/main) before gating — a raced checkout can drop files with no conflict marker.`;

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
const MERGE = {
  type: "object", additionalProperties: false, required: ["pr", "merged", "detail"],
  properties: { pr: { type: "string" }, merged: { type: "boolean" }, detail: { type: "string" } },
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
${DISK}
STEPS (Bash):
1. ${closesList(b).map((x) => `\`gh issue view ${x} --repo ${REPO}\``).join(" + ")} — read full scope + acceptance.
2. \`git fetch origin --quiet && git checkout -B auto/issue-${b.n} origin/main\`. Clone node_modules per WORKTREE ECONOMY, then install + build shared/workspace deps FIRST (consumers resolve built deps from dist).
3. Implement TDD. SCOPE DISCIPLINE: your own new files + minimal wiring; avoid shared hot files unless the hint names them; if you must touch one, make a minimal additive change so sibling PRs merge cleanly.
4. PROVE REACHABILITY: confirm the change is actually reached by a user (wired into the UI / called), not just covered by a unit test. State the exact user path in \`reachability\`. Dead/unwired code is a FAIL.
5. CONVENTIONS LINT: no invented identifiers/tokens — grep new design tokens etc. against the real source of truth; honor house rules.
6. Run the GATE to GREEN: \`${gateFor(b)}\`. Capture counts.
7. VERIFY THE DIFF IS REAL: \`git diff origin/main --stat\` must be NON-EMPTY and match what you believe you built — an empty diff means your edits didn't land (wrong cwd/worktree); STOP and fix rather than opening a hollow PR (one shipped with a fully fabricated body and was caught by review — an instant reject and a wasted cycle). Then stage explicit paths (NOT git add -A), commit (end the message with "${TRAILER}"), \`git push -u origin auto/issue-${b.n}\`.
8. \`gh pr create --repo ${REPO} --base main --head auto/issue-${b.n} --title "<concise>" --body "${closesLine(b)}\\n\\n<what + how + gate counts + reachability>"\` — only an HONEST closes list (if you delivered N of M deliverables, drop the unearned Closes and say so).
9. SCOPE OVERFLOW: if the work revealed anything BEYOND ${closesList(b).map((x) => "#" + x).join("/")} (hidden coupling, a missing prerequisite, an under-sized issue, obvious adjacent follow-ups), record it in \`scopeNotes\` AND post it as ONE comment on issue #${b.n} (\`gh issue comment ${b.n} --repo ${REPO} --body "Scope discovered beyond this fence: …"\`) so the record survives the session (durable-by-default rule, decision 2026-07-18). Do NOT grow this PR to cover it.
Return ok, issue, pr (number/url), gateGreen (true only if step 6 fully passed), gateOutput, reachability, summary, scopeNotes.`,
      { label: `build#${b.n}`, phase: "Build", effort: effortFor(b), model: buildModelFor(b), isolation: "worktree", schema: BUILD }),
  // REVIEW — starts the moment THIS item's build returns; siblings unaffected.
  (build, b) => {
    if (!build || !build.ok || !build.pr) return { build: build ?? null, verdict: null };
    return agent(`Adversarially review PR ${build.pr} (issue #${b.n}) of ${REPO} for an autonomous merge decision. Try to REFUTE it. ${CONV}${RULES ? `\nHOUSE RULES:\n${RULES}\n` : ""}
Build reported: gateGreen=${build.gateGreen}; reachability="${build.reachability}"; ${build.summary}
READ-ONLY MANDATE: you have NO worktree — your shell cwd is the USER'S LIVE session worktree; NEVER run \`gh pr checkout\`, \`git checkout\`, \`git reset\`, \`git stash\`, or ANY state-mutating git command anywhere (a reviewer that checked out a PR branch in the user's worktree hijacked their live session). Inspect via \`gh pr diff\`/\`gh api\`/\`gh pr view\` and Read files at their committed paths; if you must run code, clone/fetch into a fresh dir under /tmp.
DO: \`gh pr diff ${build.pr} --repo ${REPO}\` + read changed files in context. Judge: correct + COMPLETE for #${b.n}? Actually REACHABLE/wired (not dead code)? In scope (no unrelated churn)? Conventions + house rules honored (no invented tokens)? Bugs / missing tests / regressions? Gate genuinely green? **SPEC DRIFT** — does this PR leave a SPEC disagreeing with reality? Two shapes: (a) a decision-log PR that rules something but amends NO spec, so the ruling's only home is history nobody reads; (b) an implementation PR whose behavior contradicts, or is absent from, the spec that owns that territory. A 2026-07-27 ledger audit of 296 rulings found 29 ORPHANED and 15 CONTRADICTED precisely here — including a same-day ruling PR that touched only the decision log while the spec kept carrying the spelling that ruling rejected. Name the spec file+section in your findings. Decide approve | changes (list SPECIFIC actionable fixes) | reject (fundamentally wrong). Default to "changes" if materially off.
DISCIPLINE: every \`findings\` entry must be a REAL, actionable fix to THIS PR's diff — never placeholders. If your only material notes are follow-up scope (work outside this PR's fence), the decision is "approve" and those notes go in \`scopeGaps\`; "changes" with an empty/junk findings list wastes a fix-agent run.
ALWAYS post your verdict as ONE comment on the PR (\`gh pr comment ${build.pr} --repo ${REPO} --body "…"\`) — decision, findings, and scope gaps, INCLUDING approvals (durable-by-default rule, decision 2026-07-18: an approval's scope gaps are exactly the context that evaporates otherwise). Commenting is allowed under the read-only mandate; git state mutation is not. ALSO: note in \`scopeGaps\` anything the issue (or its milestone) UNDER-captured — work that clearly belongs but no issue covers. Return {pr, decision, findings, scopeGaps}.`,
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
${TRAIN_SETUP}
STEPS: 1. In the train worktree: \`git fetch origin --quiet && git checkout -B train-pr origin/$(gh pr view ${base.build.pr} --repo ${REPO} --json headRefName -q .headRefName)\`. 2. \`git merge origin/main\` — combine ADDITIVE conflicts (registry/index appends: keep ALL entries; keep both wirings). ⚠ diff3/union conflict styles can duplicate or drop closing braces and adjacent lines when combining — after resolving, recheck brace balance and that BOTH sides' entries survived; the gate is the arbiter. If untangleable, \`git merge --abort\` + merged:false. 3. Run the GATE to GREEN: \`${gateFor(b)}\`. Fix semantic conflicts. 4. \`gh pr diff ${base.build.pr} --repo ${REPO}\` sanity-check. 5. If green + MERGEABLE: \`git push origin train-pr:$(gh pr view ${base.build.pr} --repo ${REPO} --json headRefName -q .headRefName)\`, \`git checkout --detach origin/main\`, then \`gh pr merge ${base.build.pr} --repo ${REPO} --merge --delete-branch\` (detached first so the local-branch deletion can't fail). Else leave open + comment + merged:false. NOTEWORTHY-ONLY comment rule (durable-by-default, decision 2026-07-18): if the landing involved anything a future bisect would want — conflict resolutions, semantic fixes, files verified after a raced checkout — post ONE PR comment describing it; routine clean merges stay SILENT (no noise).
Return {pr, merged, detail (conflicts + gate counts + result, or park reason)}.`,
          { label: `merge#${b.n}`, phase: "Merge train", effort: effortFor(b), model: TRAIN_MODEL, schema: MERGE })
      : () => agent(`Fix the review-blocking issues in PR ${base.build.pr} (${closesLine(b)}) of ${REPO}, then land it. The PR is otherwise good; apply the reviewer's SPECIFIC fixes only. ${CONV}${RULES ? `\nHOUSE RULES:\n${RULES}\n` : ""}
${TRAIN_SETUP}
REVIEWER FINDINGS — these ARE the adversarial review's verdict. They ride this workflow and are NOT posted on GitHub, so an empty \`gh api .../reviews\` or \`.../comments\` result means NOTHING — never treat it as evidence the findings aren't real (this dismissal has let a real stack-corruption bug merge). Treat each finding as authoritative and apply it. Skip an INDIVIDUAL finding only if it references files/lines absent from this PR's diff (a misattributed finding), and list every skipped finding with its reason in your report:\n- ${(base.verdict.findings ?? []).join("\n- ")}
STEPS: in the train worktree, \`git checkout -B train-fix origin/$(gh pr view ${base.build.pr} --repo ${REPO} --json headRefName -q .headRefName)\`; merge origin/main (combine; recheck brace balance per the diff3 caveat); apply ONLY the findings' fixes + tests that guard them; GATE to GREEN: \`${gateFor(b)}\`; commit (end with "${TRAILER}") + push back to the PR branch; THEN post ONE disposition comment on the PR (\`gh pr comment\`) tying commits to findings — which findings you applied and which you skipped with the misattribution reason (durable-by-default rule); if MERGEABLE, detach then \`gh pr merge ${base.build.pr} --repo ${REPO} --merge --delete-branch\`. If a finding needs a human decision (scope split, design call), DON'T guess — leave the PR open, comment, and report merged:false with what's needed.
Return {pr, merged, detail}.`,
          { label: `fix#${b.n}`, phase: "Fix loop", effort: effortFor(b), model: TRAIN_MODEL, schema: MERGE });
    return enqueueTrain(job).then((m) => {
      const land = { kind: decision === "approve" ? "merge" : "fix", ...(m ?? { merged: false, detail: "no result" }) };
      if (land.merged) landedCount += 1;
      log(`train: #${b.n} ${land.merged ? "LANDED" : "PARKED"} via ${land.kind} (${landedCount} landed so far)`);
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
  const state = r.land?.merged ? "MERGED" : (r.build?.pr ? "NOT MERGED" : "no PR opened");
  return `#${b.n} -> ${r.build?.pr ?? "(no PR)"} | ${state} | verdict ${r.verdict?.decision ?? "-"} | claims: ${cl}`;
}).join("\n");

const scopeReconciliation = await agent(`You are running this wave's RETRO for ${REPO}${MILESTONE ? ` — milestone "${MILESTONE}"` : ""}. An agent landed a deliverable for each item below. Your job is to TRUE UP what was actually built against what was actually requested, item by item, and leave both the tracker and the plan honest.

THIS WAVE'S ITEMS (issue -> PR -> merge state -> what the PR claims to close):
${trackerFacts || "(none)"}

DISCOVERED-SCOPE SIGNALS reported by the builds and reviews:
${scopeSignals.length ? scopeSignals.map((s) => "- " + s).join("\n") : "(none reported)"}

For EACH item, compare DELIVERED against REQUESTED and land the difference somewhere real:
- **Delivered the whole fence and the PR merged** — close the issue if it is still open.
- **Delivered less than the fence** (the PR says "Part of", or the review/scope notes name a missing piece) — the remainder is INVISIBLE unless you record it. Comment on the issue with what shipped, what is left, and what blocks it, and file the follow-on issue (or name the existing one that covers it). A "Part of" with an untracked remainder reads exactly like "not started" — that is how six issues in one month ended up with state that lied about the code.
- **PR did not merge** — say why in one line: conflict, auto-merge armed but never completed, or review parked it. An auto-merge that arms and then conflicts leaves work LOOKING landed while its issue stays open; that has silently lost two PRs here.
- **The build found the premise already false** (already implemented, already ruled) — verify the delivering PR, comment naming it, and close the issue if nothing remains. Reporting it only to the pump means the next wave rediscovers it at the cost of another build agent.
- **Something this wave ruled or shipped supersedes a DIFFERENT open issue's premise** — update that issue too. A ruling that lands only in a doc leaves the tracker lying.
- **The item NEVER RAN** (agent died mid-flight) — say so plainly and recommend a re-queue. Do NOT report the wave as scope-held on the strength of items that never executed; absence of a result is absence of evidence, not evidence of health.\n- **Genuinely new work no existing issue covers** — file it, after deduping against the board (\`gh issue list --repo ${REPO}${MILESTONE ? ` --milestone "${MILESTONE}" --state all` : ""}\`; read the roadmap/plan doc if the repo has one).

You MAY act on all of the above — it is truing up the record of work that already happened, not rewriting the plan. The one thing you may NOT do unilaterally is restructure MILESTONES (add or expand one): propose that with rationale and let the human decide.

Finally judge the batch as a whole: did its scope HOLD, UNDER-capture, or OVER-capture? Recommend exactly one: scope-held | file-issues | expand-milestone | add-milestone, with a crisp \`proposal\` the human can act on.${LEDGER ? `
Then (durable-by-default rule) append ONE compact wave-ledger comment to issue #${LEDGER} (\`gh issue comment ${LEDGER} --repo ${REPO}\`): wave id ${WAVE_ID}; the batch; landed/parked; the lessons harvested this wave (passed below if any); your true-up actions and scope assessment. Issues filed here carry the pump:scope label; graduated lessons carry pump:lesson.` : ""}
Return {assessment, trackerActions, discoveredWork:[{title,why,suggestedMilestone}], recommendation, proposal}.`,
  { label: "retro", phase: "Retro / scope reconciliation", effort: "high", model: "sonnet", schema: SCOPE });

// Reconciliation: every built+ok PR must be accounted for — merged, or parked
// WITH a reason. No silent drops. (v3: verdict + landing ride each item, so
// this is a straight per-item read — no cross-phase matching.)
const landedList = results
  .filter((r) => r.land?.merged)
  .map((r) => ({ issue: r.issue, pr: r.land.pr ?? r.build?.pr, via: r.land.kind, detail: r.land.detail }));
const parked = results
  .filter((r) => r.build && r.build.ok && r.build.pr && !r.land?.merged)
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
  landed: landedList,
  parked,        // open PRs needing attention (with reasons / findings) — review these
  buildFailed,   // never reached a PR
  lessons: lessons?.lessons ?? [],  // paste-ready house-rule candidates for the next wave's RULES
  scopeReconciliation, // PROPOSAL for the human: did scope hold? new issues / milestone to add or expand?
  counts: { batch: BATCH.length, landed: landedList.length, parked: parked.length, buildFailed: buildFailed.length, lessons: (lessons?.lessons ?? []).length, scopeDiscovered: scopeReconciliation?.discoveredWork?.length ?? 0 },
};
