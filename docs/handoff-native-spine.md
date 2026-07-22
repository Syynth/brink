# Handoff — native spine (first light + B0.10 pipeline wiring)

**Written:** 2026-07-22. **Author:** coordinating agent (session paused by an MCP outage).
**Audience:** a fresh agent picking up this work. Read this top-to-bottom first, then act.

This session drove the native `.brink` frontend from "parser exists" to **first
light achieved (9/9 fixtures episode-byte-identical)** and wired it through the
real compiler pipeline. All work is committed and pushed to branches. The
session then lost its GitHub connector mid-flight, so **nothing has been merged
yet** — the immediate job is to land the stack.

---

## 0. TL;DR — do these in order

1. **Confirm GitHub API access.** You need the `mcp__github__*` tools (or `gh`).
   The previous session had them disconnect; verify yours work (`get_me`, or
   `list_pull_requests` on `Syynth/brink`). `git` push/fetch works over the
   proxy regardless.
2. **Land the stack in the order in §3.** Each is reviewed-clean (§4). Every
   merge must keep the oracle ratchet at **5577 / 1027 / 0** (§6) — that is the
   sacred invariant.
3. **Then resume the build pump** with the next waves in §5 (B0.10b if not
   pushed, #57 hardening, B0.10c CLI).

Repo scope: `Syynth/brink`. Default branch: `main`. Development happens on
`auto/*` (code) and `docs/*` (docs) branches, one PR each, drafts by default.

---

## 1. What was achieved this session

- **First light: native `.brink` prose runs byte-identical to the ink twin
  across all 9 first-light fixtures.** A writer can author a scene natively and
  get exactly ink's behavior.
- **The pipeline is wired end-to-end**: a `.brink` file compiles through the
  real `brink-db` salsa pipeline to `StoryData` (not just the test harness).
- **A ruling landed** (docs): native flows **end implicitly** — no `-> DONE`
  ceremony required (see §7).
- The two wins that got first light from 3/9 → 9/9:
  - **Implicit `-> DONE`** for flows that fall off the end (the ruling's
    implementation). Fixed `const-vars`, `simple-glue`.
  - **Significant-whitespace fix** in the native parser (`content.rs` was
    eating the space after `]`/`<>`). Fixed `basic-tunnel`, `complex-flow-v1`,
    `exhibit-fogg-passage`, `gather-basic`.

---

## 2. Why the session paused (the blocker)

Mid-session the **GitHub MCP connector and the scheduling (`send_later`)
connector both disconnected** and could not be re-authorized from a
non-interactive session. Even after the user reconnected GitHub in the UI, a
*running* session does not pick up a reconnected MCP server's tools — so the
`mcp__github__*` tools never re-registered. There is no `gh` CLI in that
environment and no exposed token for `curl`, so the paused session could push
branches but could **not** open/merge PRs or read CI.

**A fresh session initializes its tool list against the now-connected
connector, so it should have the GitHub tools.** If yours still lack them, tell
the user and stop — do not fake it.

---

## 3. The branch stack + landing order

All branches are pushed to `origin`. **Order matters — several are stacked.**
SHAs are the heads as of handoff.

| # | Branch | SHA | Base | What | PR |
|---|--------|-----|------|------|----|
| A | `docs/effect-system-native-unification` | `13d0fbaf` | main | Effect-system unification (docs only) | **#1217** — conflict resolved (merged main in), **auto-merge armed**; lands on green |
| B | `docs/native-implicit-end-ruling` | `531d7040` | main | Implicit-end ruling (docs only) | **#1220 — MERGED 2026-07-22** ✅ |
| C | `auto/first-light-integration` | `885de01b` | main | **First light 9/9** = `auto/first-light-native` + `auto/native-content-whitespace` merged | needs opening |
| D | `auto/b0-10a-native-seam` | `da3731c5` | first-light-native | Native `.brink` compile seam (parse_native_query + dispatch) | needs opening |
| E | `auto/b0-10b-native-discovery` | `b2c58d9b` | b0-10a | Multi-file discovery + filesystem modules — **⚠️ UNVERIFIED WIP** (agent interrupted mid-build; no gates run, may not compile) | needs verify+finish, then PR |

Also present, the two component branches that `first-light-integration` (C)
already combines — land C, **or** land these two separately, not both:
- `auto/first-light-native` (`a4df6f2d`) — harness + implicit-DONE lowering
- `auto/native-content-whitespace` (`842b2416`) — the whitespace fix

**Recommended landing sequence:**
1. **A (#1217)** and **B (#1220)** — docs-only, independent, land anytime CI is green.
2. **C** (`auto/first-light-integration`) → `main`. This is the first-light 9/9
   milestone. Open a PR (base `main`). *Note the one tracked low-sev gap in §4.*
3. **D** (`auto/b0-10a-native-seam`) → after C lands. It was branched off
   `auto/first-light-native`; once C is in `main`, **rebase D onto `main`**
   (`git rebase --onto origin/main <old-first-light-base> auto/b0-10a-native-seam`,
   or simpler: rebase onto `main` and resolve — it only adds `brink-db` files,
   so conflicts should be nil). Then open its PR.
4. **E** (`auto/b0-10b-native-discovery`) → after D lands; rebase onto `main`, open PR.

If you prefer, C can be landed as its two components (`first-light-native` then
`native-content-whitespace`) instead of the integration branch — same result,
more PRs. The integration branch is cleaner.

**Changeset note:** none of these carry a `@brink-lang/web` changeset. The rule
is "wasm-observable behavior needs a `@brink-lang/web` changeset," but the
native frontend is **not yet consumed by the web pipeline** (`parse_query`
stays `brink_syntax::Parse`; native is a parallel `parse_native_query`), so
nothing here is observable through `@brink-lang/web`. This matches the B0.7
native PR (#1215) precedent. Do not add one unless something starts feeding
`@brink-lang/web`.

---

## 4. Reviews done (all adversarial, by subagents)

- **B0.6b doc-comment fix** — already merged as **#1218** last session. Clean.
- **Whitespace fix** (`auto/native-content-whitespace`, `842b2416`) —
  **CLEAN**, no blockers. Complement-predicate `starts_text_run` verified exact;
  both load-bearing `skip_ws` uses preserved; losslessness holds. One **nit,
  pre-existing, tracked as task #56**: an interior `/* */` comment folds into
  the TEXT node (native lowering emits `node.text()` verbatim), so a mid-prose
  comment *may* render as visible prose. Design question — is a mid-prose
  comment invisible trivia or literal text? Not introduced by this fix; do not
  block on it.
- **B0.10a seam** (`da3731c5`) — **CLEAN**, no blockers. Ink path proven
  byte-identical (the seam is additive; `parse_query`/`lower_file` untouched;
  dispatch branches before either parser runs). Two inert nits. It flagged a
  scary-sounding "native lowering emits empty-stub bodies" — this is a **STALE
  DOC COMMENT** at `crates/internal/brink-ir/src/hir/lower_native/mod.rs:131-132`,
  disproven by first-light 9/9 (bodies *are* lowered since B0.7). Fix folded
  into task #57.
- **Implicit-DONE lowering** (`a4df6f2d`) — **SOUND**, one **should-fix (low)**:
  a flow whose top-level body ends in a **choice point with a non-empty gather
  continuation** does not actually get implicit-end — the appended `-> DONE`
  lands as a dead-code sibling (unreachable; the choice-target goto clears the
  container stack) and the non-empty continuation has no terminator, so the VM
  raises `RanOutOfContent`. **Not a regression, no wrong output, not hit by any
  fixture**, but it is a real hole in the ruling's promise. Tracked as **#57**.

---

## 5. Next build waves (after landing)

In priority order. All must keep oracle at 5577 (§6).

1. **B0.10b — native discovery + filesystem modules.** ⚠️ **A WIP snapshot is
   already pushed as `auto/b0-10b-native-discovery` (`b2c58d9b`) — UNVERIFIED**
   (the building agent was interrupted before committing; no gates run, may not
   compile). Pick it up: read the diff, run gates, confirm oracle 5577, add the
   tests below — do NOT rebuild from scratch, and do NOT merge it until verified.
   The intended design: sorted deterministic filesystem walk
   of the source root (no INCLUDE machinery), filesystem-derived `HirFile.module`
   stamped in the discovery/module-map layer (NOT in `lower_native::lower`).
   **Save-stability guardrail:** it touches `DefinitionId` qualification (the
   #719 landmine) — the model is already ruled (charter NF-3 filesystem-derived
   + `DefinitionId = (module, name)` + saves record absolute paths); if any
   decision isn't covered by those rulings, STOP and ask the maintainer.
2. **#57 — harden implicit-end** (see §4). Native-only fix: when a flow body's
   trailing stmt is a `ChoiceSet` whose continuation has `Tail::Unit`, recurse
   the implicit `-> DONE` into that continuation instead of stamping a
   knot-level sibling. Do **not** patch shared `build_continuation_container`
   (oracle-risky). Add a fixture (a choice-ending flow that falls off the end →
   graceful DONE). Also: add a `TODO(return-types)` breadcrumb in
   `apply_implicit_done` (it keys on `is_function`, not return-type — value
   flows must-return once they exist), and fix the stale doc comment at
   `lower_native/mod.rs:131-132`.
3. **B0.10c — CLI + real first light.** `brink compile scene.brink`: branch on
   entry extension in `brink-compiler::prepare_driver` (`crates/brink-compiler/src/driver.rs`,
   ~line 46 — `.ink` keeps the INCLUDE BFS, `.brink` uses native root-walk
   discovery). Wire the LSP native accessor (NS-T). Exit criterion: a `.brink`
   scene compiles and plays through the runtime via the CLI.
4. **#1219 — branch-asymmetry lint** (already filed as GitHub issue #1219). The
   one thing ink's "ran out of content" error was good for: a choice branch
   that dead-ends while siblings divert. Low-priority, opt-in warning, not a
   blocker.
5. **Parked, off the "author a scene" critical path:** the block-effect
   migration S3→S4→S5 (analyzer reads `tail`/effect-row off `Block`; build the
   §6.1 row-polymorphism); the design stubs #1210 (concurrency), #1211 (effect
   core), #1212 (runtime restructuring) — these need maintainer design input,
   don't build blind.

---

## 6. Invariants — do not violate

- **Oracle ratchet: `EPISODES: 5577 pass / 1027 mismatch / 0 missing`,
  `CASES: 350 pass / 14 fail / 390 total`.** Every branch here holds it. Run
  `cargo test -p brink-test-harness --test oracle_snapshots -- --nocapture`
  after any change. If it moves, you leaked native behavior into the ink path —
  the native work is supposed to be *additive*. `RATCHET_EPISODE_COUNT` lives in
  `crates/internal/brink-test-harness/tests/oracle_snapshots.rs`.
- **Run WORKSPACE clippy, not per-crate.** The lesson from BE-S1: adding a field
  to a public struct (or a new query/enum) breaks downstream literals in *other*
  crates' tests that a per-crate check misses. Use:
  `cargo clippy --workspace --exclude bevy-brink --exclude bevy-brink-derive --all-targets -- -D warnings`.
- **Native changes are native-only.** `lower_native` and `parse_native_query`
  must never alter ink output. The oracle is the proof.
- **`fmt`:** `cargo fmt --all -- --check`.
- **Lints denied:** `unwrap_used`, `expect_used`, `panic`, `todo`,
  `print_stdout`, `print_stderr`, `unsafe_code`. Tests exempt via `clippy.toml`.
- **Determinism:** never iterate `HashMap` where order affects output; sort or
  use `BTreeMap`. Critical for the B0.10b discovery walk (sort dir entries;
  assign `FileId`s in sorted path order or `DefinitionId`s scramble).
- **Commit granularity:** one fix per commit. End commit messages with the
  `Co-Authored-By:` and `Claude-Session:` trailers the repo uses.

---

## 7. The ruling this session made (context for the code)

**Native flows end implicitly** (decision-log 2026-07-22; docs on branch B /
PR #1220). A `flow` — or any braced body (choice branch, conditional arm) —
that runs out of content **ends implicitly**, lowering to the **DONE** terminal
(turn/flow complete), NOT END. `-> END` stays the explicit-only "story is
permanently over" act. `-> DONE`/`-> END` remain available but optional. This
retires ink's "ran out of content. Need a `-> DONE`?" error on the native
surface — justified because brace-delimited flow **and** choice bodies make body
extent explicit (ink's error existed to disambiguate implicit-extent knots).
**Value-returning flows are the exception** (must return; checker-enforced) —
but v1 native flows don't declare return types yet, so all native flows are
non-value for now. The residual value of ink's error (catching an asymmetric
choice-branch dead-end) moved to lint #1219.

Implementation: `body::apply_implicit_done` in `lower_native` — a non-value
flow whose `Block` tail is `Tail::Unit` gets a synthesized `-> DONE`
(`DivertPath::Done`, existing opcode, `ptr: None`). Called at flow finalization
in `container.rs` (`lower_knot` only when `!is_function`; `lower_stitch`
always). The `tail` taxonomy is from BE-S1 (`Tail::{Value|Diverge|Unit}`,
`crates/internal/brink-ir/src/hir/types.rs`).

---

## 8. The B0.10 design (for slices b/c)

Full scoping is in the decision history; the essentials:

- **Seam = Option (b), already built in B0.10a.** `parse_query` (ink) stays
  byte-identical; a parallel `parse_native_query` + `lower_native_file` were
  added; `lowered_query` branches on `file_language(path)` (extension test,
  `.brink` → native) *before* calling either parser. So the ink memo path is
  provably untouched. Files: `crates/internal/brink-db/src/queries/mod.rs`
  (`parse_query` ~299, `lowered_query` ~330, `lower_file` ~2130, ingredient
  list ~142), `crates/internal/brink-db/src/db.rs` (`ProjectDb::parse` ~203,
  new `parse_native`).
- **Downstream is frontend-agnostic** — verified: codegen has zero
  `brink_syntax` refs; analyzer's refs are all `#[cfg(test)]`;
  `validate_admission` takes only the `(HirFile, SymbolManifest, diags)` triple.
  So only *parse + lower* are dialect-specific. Native lowering returns the same
  triple (`brink_ir::hir::lower_native::lower`), manifest via `project_manifest`.
- **Dialect selection** is extension-derived (`.brink`), a pure deterministic
  string test — no new schema, no `Dialect::Native` variant (the existing
  `Dialect{StrictInk,Brink}` in `brink-analyzer/src/dialect_gate.rs` is a
  different axis — ink-extension gating, not frontend selection). Native
  compiles pass `Dialect::Brink` in `AnalysisOptions`.

---

## 9. Where things live

- **Worktrees** (previous session left these; a fresh session won't have them —
  recreate as needed with `git worktree add`):
  `.claude/worktrees/first-light`, `b0-10a`, `b0-10b`, `fl-integ`, and per-agent
  worktrees. Not needed to land — you can work from a fresh clone/checkout.
- **Key native files:**
  `crates/internal/brink-syntax-native/` (lexer/parser/CST/AST — the native
  frontend), `crates/internal/brink-ir/src/hir/lower_native/` (native→HIR
  lowering: `mod.rs`, `container.rs`, `body.rs`), `crates/internal/brink-db/`
  (the pipeline seam), `crates/internal/brink-test-harness/` (first_light test +
  oracle snapshots).
- **First-light fixtures:** `tests/tier1-brink-respell/*` (native `.brink` +
  `manifest.toml`). Test: `cargo test -p brink-test-harness --test first_light -- --nocapture`
  (prints `=== first light: N/9 fixtures episode-identical ===`).
- **Governing docs:** `CLAUDE.md` (project rules), `docs/native-surface-charter.md`
  (native surface design), `docs/decision-log.md` (all rulings — read the last
  several entries), `docs/b0-sequencing.md` (Track B plan),
  `docs/block-effect-model.md` + `docs/effects-spec.md` (the parked S3-S5 work).

---

## 10. Open GitHub items

- **#1217** — effect-system unification (docs). **MERGED 2026-07-22** ✅ (conflict
  with the #1220 decision-log tail was resolved by merging main in; now in `main`).
- **#1220** — implicit-end ruling (docs). **MERGED 2026-07-22** ✅ (in `main`).
- **#1218** — B0.6b doc comments. **Already merged.** (Its `//!`-visibility fix
  is in `main`.)
- **#1219** — branch-asymmetry lint. Filed as an issue (design stub), not built.
- To be filed when convenient: the mid-prose comment-visibility question
  (task #56).

---

## 11. Task board snapshot (session-local, won't persist)

Completed this session: first-light implicit-DONE + rebase; whitespace fix;
B0.10 scoping; B0.10a seam; combined 9/9 confirmation.
Pending: land #1220 + #1217 (GitHub-blocked); #56 (mid-prose comment issue);
**#57 (implicit-end ChoiceSet hardening — see §4/§5)**; B0.10b (building);
B0.10c (CLI).

---

*End of handoff. If anything here conflicts with the actual repo state, trust
the repo + `docs/decision-log.md` over this document, and re-verify branch SHAs
with `git fetch origin && git log`.*
