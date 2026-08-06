---
"@brink-lang/web": patch
---

Issue #2113 (NS-T seam 3/6): the explain-match query, discharging the
"no invisible expansion" compensation for conventions-claimed prose
lines — for any line, whether it's matched, by what handler (fn name +
declaration location), what it bound (captures as byte spans), the
patterns attempted on a miss (registration order), and any other
handler shadowed on a hit.

- **`brink_ir::explain_match`/`ExplainMatchCache`** (new, `brink-ir`):
  a pure composition over #2112's `classify_line` output and #2111's
  `ConventionsProjection::entries` — no second walk. `ExplainMatchCache`
  memoizes on `(line text, projection)` and additionally caches the
  *compiled* pattern set per projection (the w133 perf finding on PR
  #2257: `classify_line` compiled a fresh `Regex` per call, per entry).
- **`EditorSession::explain_match`/`explain_match_doc`** (new,
  `@brink-lang/web`): the wasm binding, wrapping a per-session
  `ExplainMatchCache`. Returns JSON with **raw byte ranges** throughout
  (not this crate's usual UTF-16) — a matched handler's declaration
  range lives in the project's conventions module, a file this session
  may never have opened, so there is no single file to convert against;
  see `editor/explain_match.rs`'s own doc.
- **`ElementKind` ("matched kind") composition is deliberately deferred**
  — `crate::ExplainMatchCache`'s own module doc explains why: the one
  function that derives it reads a parsed CST node with surrounding-line
  context (a parenthetical is chain-gated on the preceding line being a
  live cue), which this query's bare-text entry points cannot supply.
  Left as a follow-up for a caller holding a real parsed document.

⚠ **Reachability caveat, discovered while writing this PR's own
end-to-end test, pre-existing and not introduced here:**
`brink_ide::session::IdeSession::analysis_options` hardcodes
`conventions: None` on every call — `EditorSession::apply_project_config`
validates `[project] conventions` far enough to warn on an unrecognized
value, but never wires it into the live `ProjectDb`'s real
`AnalysisOptions`. So `conventions_projection()` (and therefore this
query, and the pre-existing `E169` confinement diagnostic) is always
empty through the `EditorSession`/wasm editor path today, for every
project configured the only way an embedder can. The query itself is
proven correct against real project data one layer down
(`brink-db`/`brink-ir`); see the PR description for the follow-up issue
tracking the `IdeSession` wiring gap.
