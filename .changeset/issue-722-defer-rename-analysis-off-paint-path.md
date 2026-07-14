---
"@brink-lang/editor": patch
---

Rename-collision analysis (#722) no longer runs synchronously on the paint
path. Root-caused from PR #714's RCA of the #696 e2e flake: the inline
rename widget's breakage/collision query (`renameSymbolAt`, a wasm call)
used to run inline in the debounce/Enter handler and could block paint for
several seconds under load.

The debounce settle (and an Enter with no cached result) now flips the "⚠
breaks N" badge into a disabled, `aria-busy` **pending** state (`⋯`,
`.brink-inline-rename-badge--pending`) synchronously — so a paint can land
before the heavy call runs — then defers the actual query to the next idle
slot (`requestIdleCallback`, falling back to a macrotask where unavailable)
via a small `scheduleIdleWork`/`cancelIdleWork` helper. Apply/force stays
disabled until the deferred query resolves; Enter never forces a synchronous
call. `query`'s signature is unchanged (still plain and synchronous) — only
its *scheduling* moved, so existing synchronous callers/tests keep working
unmodified. Same shared `InlineNameInput` also covers extract-to-knot/
function (#315 H), so both prompts get the same off-paint-path behavior.

No web-worker architecture (out of scope for #722) — a query that is itself
slow still blocks once it starts; this mitigates by not starting it inside
the same frame as the triggering keystroke.
