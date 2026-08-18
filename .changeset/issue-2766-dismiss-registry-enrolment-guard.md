---
"@brink-lang/studio": patch
---

Add a mechanical guard (`packages/brink-studio/src/__tests__/dismiss-registry-enrolment.test.ts`)
that a new dismissable surface enrols in the "Escape dismisses every
registered transient surface" safety net (#279, PR #2760), so a future
surface can no longer ship its own `document`-level `keydown`/`pointerdown`
dismiss listener without a `registerDismissible()` call and silently fall
back into the unescapable-menu failure mode #279 was filed for (issue
#2766).

The scan covers both independent, uncoordinated registries
(`packages/studio-shell/src/dismiss-registry.ts` and
`packages/ink-editor/src/dismiss-registry.ts`) from one test file, checking
each package's listeners against its own registry. `packages/studio-shell`'s
three Escape-cancels-a-gesture handlers (tab drag, strip-icon drag,
maximize restore — `tab-drag.ts`, `strip-drag.ts`, `regions.tsx`) are marked
`DISMISS-NET-EXEMPT` with a reason: they manage transient interaction/layout
state, not a floating menu/popover/modal surface, so they are out of this
net's scope by design.
