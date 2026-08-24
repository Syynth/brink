# Editor context-menu spec (DRAFT for ruling)

## Problem

Right-clicking in the editor today almost always yields the native WKWebView
menu (Look Up / Cut / Copy / Paste / Select All) — the studio's own menu
exists only on knot/stitch headers (the shared symbol menu, #304) and on
Binder/Story-Graph rows. Most things the editor understands are not
context-menu-able at all, and the inline-rename UI with its breakage report
(#305/#306) is reachable from far fewer places than it should be.

Ruled direction (2026-08-24): enumerate every token/context the editor
produces, define the appropriate actions for each, and hold the whole
surface to one **context → action → response** matrix.

## Architecture (proposed)

One CM6 `contextmenu` handler on the editor:

1. `preventDefault()` always — the native menu never appears inside the
   editor. Cut/Copy/Paste/Select All become OUR items so nothing is lost.
2. Resolve the click position into a **context**: innermost HIR span at the
   offset (identity first: `VarRef`, `Divert`, `Call`, decl names…), else
   the semantic token, else the line's element kind, else plain text /
   selection.
3. Dispatch through a **menu-provider registry** — the same ordered-provider
   pattern as the hover sections: each provider contributes items for the
   contexts it knows; the matrix below is the contract for what must appear.

Responses are one of: **navigate** (editor.reveal), **inline UI** (the
rename input with breakage report), **panel** (references list, TODOs),
**session** (play from here), **edit** (structural op, with toast + undo),
**clipboard**.

## The matrix

Legend: ✅ exists today (where noted, only on some surfaces) · ⬜ proposed ·
— not applicable. "Rename" always means the inline-rename UI surfacing the
breakage report; unsafe renames offer "Force rename".

### Identity-bearing tokens

| Context (right-click on…) | Go to Def | Find Refs | Rename (breakage) | Play from here | Structural ops | Notes |
|---|---|---|---|---|---|---|
| Knot name — header | — | ⬜ | ✅ header menu | ✅ header menu | ✅ Move Up/Down · Move to · (function: —) | menu exists (#304); add Find Refs |
| Knot name — divert ref | ⬜ (cmd-click ✅) | ⬜ | ⬜ | ⬜ | — | refs get NO menu today |
| Stitch name — header | — | ⬜ | ✅ | ✅ | ✅ incl. Promote to Knot | |
| Stitch name — divert ref | ⬜ | ⬜ | ⬜ | ⬜ | — | |
| Function name (decl/call) | ⬜ | ⬜ | ⬜ | — | — | calls resolve (`Call` span) |
| Label (gather/choice) | ⬜ | ⬜ | ⬜ | — | — | |
| VAR / CONST / temp name | ⬜ | ⬜ | ⬜ | — | — | decl and every ref |
| Parameter name | ⬜ | ⬜ | ⬜ | — | — | scope-local |
| LIST name | ⬜ | ⬜ | ⬜ | — | — | hover shows members now |
| List item | ⬜ | ⬜ | ⬜ | — | — | |
| EXTERNAL name (decl/call) | ⬜ | ⬜ | ⬜ | — | — | |
| STRUCT name / field (native) | ⬜ | ⬜ | ⬜ | — | — | |

### Structural / statement contexts

| Context | Actions (all ⬜ unless noted) | Response |
|---|---|---|
| Choice line | Fold branch · Extract to knot/stitch (code action) · Move Up/Down within weave | edit + toast |
| Gather line | Fold continuation · same weave ops where legal | edit |
| Conditional / sequence | Fold · (future: invert condition?) | edit |
| Divert statement (`->`) | Go to target · Find refs of target | navigate |
| INCLUDE path | Open file · Reveal in Binder | navigate |
| Tag (`#tag`) | (future: Find same tag — #474's namespace) | panel |
| TODO line | Reveal in TODOs panel · Delete note | panel / edit |
| Interpolation `{expr}` | Go to Def / Rename on the inner ref (resolve innermost) | as identity row |
| Comment | text actions only | clipboard |
| Narrative text / selection | Cut/Copy/Paste/Select All · Extract selection (where code action applies) | clipboard / edit |
| Dialect lines (cue/dialogue) | text actions + (future: dialect-declared actions) | |

### Everywhere (bottom section of every menu)

Cut · Copy · Paste · Select All — ours, replacing the native menu's, with
the standard shortcuts shown.

## Consistency rules

1. **Same identity ⇒ same menu.** A knot name shows the same items whether
   clicked in a header, a divert, the Binder, or the Story Graph — surfaces
   may ADD surface-specific items (Binder file ops) but never diverge on
   the shared core (the #304 principle, extended to references).
2. **Order is fixed**: Navigate (Go to Def, Find Refs) · Identity (Rename)
   · Run (Play from here) · Structure (Move/Promote/…) · Context-specific ·
   Text (Cut/Copy/Paste). Dividers between groups only — never doubled
   (the binder's stitch-menu divider bug class).
3. **Destructive/unsafe actions never act silently** — rename goes through
   the breakage report; structural ops toast with Undo.
4. **No dead items**: a provider either contributes an enabled item or
   nothing. Grayed-out placeholders only where discoverability matters
   (ruling needed — see open questions).

## Open questions (for ruling)

1. Find References response: dedicated references panel, or reuse Search
   results surface?
2. Grayed-out vs hidden for inapplicable items?
3. Does "Extract selection" belong in v1, or wait for the ops audit?
4. Tag actions: wait for #474's tag namespace, or ship "Find same tag" on
   the plain-text search path now?
