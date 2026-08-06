---
"@brink-lang/web": patch
---

Format: `SaveState`'s suspended-flow section (`SuspendedFlow`, FS-1) gains
two fields — `next_block_id` and `pending_element` — per the 2026-08-05
ruling on issue #2108 ("block metadata persists, and `next_block_id`
persists with it"), for two independent reasons. Element-attachment data
(`@[convention(..., attach = X)]`, #2260) accumulates only in the VM
output buffer's `pending_element`/transcript, which does not survive a
park, so a flow parked (`await`) inside an open attach run needs
`pending_element` to avoid silently resetting the attributed
speaker/metadata to empty on resume. `next_block_id` needs to persist on
its own account regardless of attachment: restarting it at 0 would give
the same uninterrupted run a different id after resume (and could collide
with ids already emitted), breaking `BlockId`'s "same id iff same
uninterrupted run" contract. Both fields carry `#[serde(default)]` (an
older save decodes as `0`/empty, identical to pre-#2108 behavior) and
`pending_element` uses `skip_serializing_if` to omit the key entirely when
no run was open at park time, so the common case's wire form is
unaffected.

Format-only, matching the rest of `SuspendedFlow`: the FS-2/FS-3 compiler
synthesis and runtime spill/restore that would populate or consume a
*live* value are still unbuilt, so `Story::save_state`/`load_state` (and
therefore every current `@brink-lang/web` save/load call) still always
produce/consume `suspended: None` — no observable runtime behavior
changes today. The changeset is filed because the wire shape of a type
`@brink-lang/web` re-exports (`SaveState`) changed.
