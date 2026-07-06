---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Chromium 88 (RMMZ/NW.js) compatibility: remove every `color-mix()` from the editor and studio themes — Chromium 88 has no `color-mix()` (Chrome 111+), so those declarations were dropped wholesale, most visibly leaving text selection with no fill.

- Behind-text highlight layers (`.cm-selectionBackground`) now use a solid `var(--bs-accent)` fill plus layer `opacity`, which composites identically and works on any host that defines the base tokens.
- The active line uses a new optional theme token `--bs-active-line-bg`, falling back to the opaque `var(--bs-surface-bg)` for hosts that define only base tokens.
- All other alpha-tinted highlights (search/selection matches, bracket matching, binder/search/graph chrome) are written as `rgb(var(--bs-X-rgb) / N%)` over new per-theme sRGB triplet tokens (`--bs-accent-rgb`, `--bs-error-rgb`, …) defined by the built-in Mocha/Latte themes.
- Opaque two-color mixes (story-graph node borders/fills, conflict banner) are precomputed per theme as `--bs-graph-*` / `--bs-conflict-banner-bg` tokens.

Visual output on modern Chromium is unchanged; hosts embedding `@brink-lang/editor` with a custom token set get correct selection/active-line out of the box and can define the new tokens for the tinted variants.
