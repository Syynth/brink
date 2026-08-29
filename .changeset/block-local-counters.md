---
"@brink-lang/web": patch
---

Anonymous-container ids are now edit-local: the conditional/sequence
counter is weave-block-local instead of knot-global, and a `(label)`
anchors its whole subtree (`#lbl:` scopes). Inserting content shifts
anonymous visit-state ids only for later siblings in the same block —
never across the knot — and never inside a labeled choice or block.
One-time renumbering: previously-saved anonymous visit states resolve as
dropped on load (`anonymous_states_dropped`); named state is unaffected.
Also fixes an E060 "internal codegen error" on legal ink when a
block-level alternative held a choice in more than one branch.
