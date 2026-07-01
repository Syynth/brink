> **Status: complete (2026-07-01).** All #311 sub-issues merged; the wave tables below are the historical execution record. See #311 and `editor-consumer-guide.md`.

# @brink-lang/editor epic (#311) — execution plan

Derived from the Stage-0 design/RCA dossier fan-out (2026-06-30). Two tracks:

- **Track N (non-visual)** — additive Rust/wasm/TS-library surface (ops, wasm methods, exported extensions/models). *No studio UI consumes these until Track V wires them*, so they are invisible to users and land on a **green-gated auto-merge-train** (serial merges in dependency order, adversarial review, no per-item human gate).
- **Track V (visual)** — the UI that wires the Track-N cores + the genuinely-visual issues. Gets a **dedicated design round** with the user first, then human-gated build + run-the-studio (Claude Preview) verification before merge.

Green gate (`.github/workflows/ci.yml`, now required on protected `main`): Rust = `cargo check/test/clippy --all-targets -D warnings/fmt`, `cargo-deny` (all `--workspace`); Frontend (via `@brink-lang/studio`, which aggregates every package — `ink-editor` has no own scripts) = `wasm-pack build crates/brink-web` → `pnpm install` → `typecheck` → vitest `test` → Playwright `test:e2e`; plus `Book`.

## Headline RCA corrections from the fan-out

- **#318 is not a code bug in current source.** `rename_file` produces the correct shallower-move rewrite (verified by executing it). Likely real cause: a stale published `@brink-lang/web` wasm. → reclassified to *verify + shallower-move regression test + rebuild/republish*. Do **not** edit `compute_relative_path`/`resolve_include_path` (shared, correct).
- **#321 C**: the Ctrl-. menu is never wired into a studio editor today (`getCodeActions` is unset). The Track-N core is only the wasm `resolve_code_action` op + a stable action id.
- **#319 A**: the studio editor already has search via `basicSetup`. Track-N value = a reusable exported `findPanel()`.
- **#322 D**: a working pure search engine already exists in `studio-store`; the Track-N core *relocates* it into `@brink-lang/editor`.
- **#320 B₁**: the data-loss clobber is two-part (`project-session.ts` `updateFile` + `applyExternal`).

## Track N — auto-merge-train

### Wave 1 — STATUS (2026-06-30)
| PR | Core | Issue | Status |
|----|------|-------|--------|
| #325 | shallower-move regression tests | #318 | merged |
| #331 | shared `include_block_span` + `reachable_from` + fold range + INCLUDE insertion | #312/#313 | merged |
| #326 | `findPanel()` opt-in extension export | #319 | merged |
| #327 | B₁ conflict hook + stop dirty-buffer clobber | #320 | merged |
| #328 | wasm `resolve_code_action` op + payload id | #321 | merged |
| #329 | relocate search engine → editor `ProjectSearch` model | #322 | merging (rebased) |
| #330 | `findReferencesAt` / `referencesToSymbol` wasm + TS | #317 | merging (rebased) |
| #333 | cargo-deny ignore RUSTSEC-2026-0192 (ttf-parser) | #332 | merged |

### Wave 2 — `#316` (off a `main` that has #317)
Generalize the introduced-diagnostics gate op-agnostically; add `deleteSymbol`; upgrade `rename_file`/`move_stitch`/`promote`/`demote` to carry `safe`+`introduced_diagnostics`; reorders hardcode `safe:true`; unify into one **breaking `StructuralResult`**.

### Wave 3 — `#314` (← #316) · `#315 H` extract op (← #316)

## Track V design decisions — LOCKED (2026-06-30, mockup round)

- **#319 A Find panel** — opt-in `findPanel()`, docked **top**, replace inline; not auto-enabled in studio.
- **#320 B₂ merge view** — **side-by-side 2-way** (yours vs disk, no baseline). Adds `@codemirror/merge`. Keep-mine / use-disk banner + two-column diff.
- **#322 D search** — **editor-owned editable results buffer** (full Zed-style ask): a CM6 results buffer in `@brink-lang/editor` where edits route back to documents.
- **#321 C code-actions** — **editor API only, not enabled in studio this epic**. Ship resolve/apply wiring for hosts; don't wire the Ctrl-. menu into studio yet.
- **#323/#324 inline rename** — fully inline in the editor (the mockup version):
  - Inline **name-input chip** replaces the symbol at its anchor on F2 / context-menu rename.
  - Live **"⚠ breaks N"** badge to the right, recomputed debounced via `rename_safe` (#324); hidden when N=0 (safe rename commits on Enter with no popover).
  - The badge expands to an **inline breakage report** beneath it — the affected-reference list + **[Cancel]** / **[Rename anyway]** — NOT a modal. (Supersedes the earlier modal-for-report lean.)
  - `Esc` cancels, `Enter` commits (focuses Rename-anyway when unsafe). The modal remains only for **Binder/Story-Graph** renames (no editor anchor).
  - The breakage `SymbolRenameResult` is still exposed via an **optional host callback** so a host (the celeris lens) can override the rendering; the editor's **default is this inline report**.

### Track V open UX questions still to resolve (per dossiers)
- #320 B₂: mount (replace doc / side panel / inline banner), re-baseline on resolve, expose `conflictedPaths()`.
- #322 D: replace UX in the buffer; multi-line/stale-line handling.
- #316 delete UX (after Wave 2 core): entry-point placement, confirm-always vs confirm-on-break, Force-delete.
- #315 H (after Wave 3 core): param handling v1, crossing-header behavior, new-knot placement, editor-only vs also-Binder.
- #312 F: "from <file>" affordance style, multi-definition tiebreak, cycle handling.
- #313 G: fold placeholder copy, N≥2 vs N==1, default-folded Setting.

## Architectural decisions — RESOLVED (2026-06-30)

1. **Release shape** — all 12 items ship as one npm release of `@brink-lang/editor` (+ `@brink-lang/web`); the phases are internal build order, not release boundaries.
2. **Track split** — non-visual cores auto-merge; visual surfaces get the design round first.
3. **#316 result shape → one breaking `StructuralResult`** `{ ok, path?, new_source?, cross_file_edits, safe, introduced_diagnostics }`. **Coordinated breaking change**: the train migrates `@brink-lang/web` + studio; the **celeris Narrative lens (separate repo)** must migrate its `applyMoveResult` consumers in lockstep — out of scope for this repo, flagged for the user.
4. **#316 reorder gating → hardcode `safe:true`** for reorder ops (skip reanalysis).
5. **#318 → verify-only** (stale-wasm assumption; re-open at the binder layer only if a fresh build still reproduces).
6. **Merge mechanism** — `main` is protected with required checks `[Check, Test, Clippy, Format, cargo-deny, Book, Frontend (brink-studio)]` and `allow_auto_merge`; cores land via PR + auto-merge-on-green.
