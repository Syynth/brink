# Pump wave plan — w148 onward

Board snapshot taken 2026-08-13, after w147 (brink-desktop D3 complete).
Companion to `.claude/skills/autonomous-pump/BRINK-CONFIG.md`; the ledger is #967.

## Board state

| Metric | Value |
|---|---|
| Open issues | 394 |
| Created in the last 13 days | **200 (51%)** |
| `pump:scope` | 199 (50.5%) |
| `compiler` | 107 |
| `needs-design` | 49 |
| `testing` | 26 |
| Open PRs | 9 (7 non-dependabot) |
| Ratchet | 5,608 (`oracle_snapshots.rs:52`) |
| Editor acceptance gate | 7/7 |

**The backlog is growing, not shrinking.** This is not sloppiness — scope
reconciliation is working as designed and finding real remainders (w146 landed 5
items, filed 11, one of them a genuine correctness bug). But at a fan-out ratio
above 1:1, more waves of the current shape do not converge. w148 onward changes
wave *composition*, not just wave count.

## The three convergence levers (ruled 2026-08-13)

1. **Cluster by root cause.** Compose waves from issues sharing one defect so a
   single fix closes several. Confirmed clusters below.
2. **Duplicate/staleness cull.** Nothing has swept the 199 recent `pump:scope`
   issues. House rule already presumes sub-#300 stale; the recent half is
   unaudited.
3. **Clear the stale PR queue.** Seven non-dependabot PRs, oldest from 2026-07-11.

Deferred by the same ruling: the 49 `needs-design` issues stay parked. Waves draw
only from items needing no new ruling. Build agents continue to DECLINE per the
#458 precedent.

## Confirmed root-cause clusters

These are *verified duplicates or shared-root defects*, not thematic groupings.

- **Native parse road, one defect in two crates.** #2291 (`brink-web`) and #2360
  (`brink-lsp`) are the same defect — folding/hints/transform still route through
  an always-ink parse for native `.brink` files. One fix, two crates, two issues.
- **`lookup_global` silent-drop.** #2262 states the pattern recurs at **4 more
  sites** and asks for a shared lookup-or-diagnose helper. The helper is the fix;
  the sites are the closure.
- **M-2d collision class.** #2229 (`hir::stamp` per-knot loop, self-described as
  the "4th M-2d collision site") and #2230 (`lir::lower::context::lookup_address_id`,
  no file-scoping, no kind filter). Same missing per-file qualifier.
- **`native_module_path` duplicated** across `brink-analyzer` and `brink-db`
  (#2379) — a #2335/#2274 remainder.

## Wave slate

### w148 — Desktop hardening (CI first)

The desktop lane is running blind: **#2402 reports zero CI** (no `cargo check`,
no clippy, no `tsc`, no `pnpm build`). w147 found the desktop suite was silently
executing **9 of 34 tests** because `vitest.config.ts` lacked the workspace
aliases its sibling `vite.config.ts` carries — a test file that fails to *load*
is not a failing test, it is an invisible one.

`#2402` lands **first and alone**; it changes CI and everything else in the lane
depends on it being real.

Then: #2400 (macOS Dock Quit bypasses guarded quit), #2401 (guarded-quit IPC
unverified; unhandled `destroy()` rejection can leave the window unclosable),
#2403 (`TauriFileProvider.requestSave()` unserialized — autosave and quit-time
`saveAll` can overlap), #2404 (watcher self-write suppression misses null content,
so a studio-initiated delete's own echo drops the pending egress record), #2405
(embedder-api.md silent on orphaned-under-write-through).

Closes epic #2346's follow-on tail. D4 (signing/notarization/updater) stays
deferred per the 2026-08-06 local-build-first ruling.

### w149–w150 — Native editor parity

The project's stated center. Lead with the confirmed duplicate:

- **#2291 + #2360 as ONE item** — the shared native-parse-road fix.
- #2378 — `brink-ide` native hint/widget passes miss SPLICE and module-qualified
  callees (#2359 remainder).
- #2321 — `brink-lsp` surfaces no whole-project diagnostics; E169 and everything
  else from `whole_project_diagnostics_query` is invisible in the LSP problems list.
- #2365 — ink `tag()` text still classified variable/operator in semantic tokens.
- #2364 — native `flags` declarations report `SymbolKind::List`, dialect-inaccurate
  in outline/LSP/QuickOpen.
- #2363 — `brink-cli ide`: `KindFilter` has no `Struct` variant.
- #2362 — studio-ui Binder: surface struct/variable/list/external symbol rows.
- #1952 — NS-T.0: register `.brink` in a client + gate semantic tokens on
  `db.is_native` (must ship together).

Both-roads discipline applies throughout: db-direct (`ProjectDb`) *and* off-db
(`IdeSnapshot::analyze`). A green `brink compile` is not evidence the editor agrees.

### w151 — Name-resolution root-cause wave

Spine first, sites second:

- #2262 — the shared lookup-or-diagnose helper (the root fix).
- #2229, #2230 — the M-2d collision sites.
- #2263 — `manifest::insert_symbol` lacks the #2197/#2213 std carve-out; a
  project's own declaration can be evicted by a std-mounted homonym.
- #2255 — `lookup_unique_by_name` declines when a now-visible std sibling collides.
- #1967 — unimported private const/var in another native module hijacks a bare
  VALUE reference over a same-file binding (E087).
- #2379 — the duplicated `native_module_path`.
- #2246 — LIR lowering resolves bare names against a flat global map, no module
  scope, though the analyzer already has `Candidacy`.

Build-heavy (analyzer/IR). Per the disk rule: **≤4 items, at most one opus build.**

### w152 — Tracker truth + cull

- #2410 — orphaned `auto/b0-10b-native-discovery` branch (524-line WIP, no PR,
  since 2026-07-22); supersession check before any deletion.
- #2086 — stray merge-conflict marker in `decision-log.md` ~line 2405.
- #2126 — stale `ruling-ledger.md` entries from #1683/#1508 delivery.
- #2071 — `native-feature-status.md` stale rows (the `!name` sigil dispatch row
  still says "reserved, unimplemented").
- #2184 — `compiler-spec.md` diagnostic-codes table has no gate against
  `DiagnosticCode::ALL` (E173–E175 rows missing).
- #2220 — repo-wide sweep for stale `@[element(claims = ...)]` prose.
- Plus the `pump:scope` duplicate sweep (lever 2).

## Stale PR queue — decisions needed

| PR | Age | State | Recommendation |
|---|---|---|---|
| **#1050** version npm packages | since 07-18 | `blocked`, 247 files, +6,044 | **Release.** Four weeks of diligently-authored changesets have never shipped. The house rule says this merges LAST but *do not starve it* (the 0.9.1 lesson). It is starved. |
| **#503** release v0.0.12 | since 07-11 | release-plz bot | Decide alongside #1050; #1359 (yank brink-environment 0.0.11) is gated on v0.0.12 shipping. |
| **#1980** #1253 choice label/guard | since 08-01 | `unknown` | **Review and land.** Complete orphan-recovered fix for still-open #1253; explicitly UNREVIEWED. Treat as a fresh build-agent PR. |
| **#1297** book ch.19 numeric tower | since 07-23 | — | #1184 still open. Review or close. |
| #2013 decision-log correction | since 08-02 | — | Small docs correction; land or close. |
| #2037 triage stale-count note | since 08-02 | — | Small docs note; land or close. |
| #1006 BH-4 wake baselines | since 07-17 | — | Per the quiet-window rule, baselines are canonical only from a solo run. Re-measure in an inter-wave gap or close. |
| #1889 / #2338 dependabot | — | — | Ordinary dependabot; ride a wave's merge train. |

## Config correction applied

`BRINK-CONFIG.md` seeded every build agent with "5,598 episodes must not move."
The real ratchet is **5,608** (`oracle_snapshots.rs:52`, confirmed unmoved by the
w147 ledger). Corrected in this change — a stale sacred number in the file that
seeds agent RULES is exactly the "never state a number you did not just read"
failure the house rules warn about.
