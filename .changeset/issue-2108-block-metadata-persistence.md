---
"@brink-lang/web": patch
---

Format: `SaveState`'s suspended-flow section (`SuspendedFlow`, FS-1) gains
two fields — `next_block_id` and `pending_element` — per the 2026-08-05
ruling on issue #2108 ("block metadata persists, and `next_block_id`
persists with it"). Element-attachment data (`@[convention(..., attach =
X)]`, #2260) is keyed by a flow's block-run numbering; a flow parked
(`await`) inside an open attach run needs both to survive a save/resume
cycle, or the attributed speaker/metadata would silently reset to empty on
resume. Both fields carry `#[serde(default)]` (an older save decodes as
`0`/empty, identical to pre-#2108 behavior) and `pending_element` uses
`skip_serializing_if` to omit the key entirely when no run was open at
park time, so the common case's wire form is unaffected.

Format-only, matching the rest of `SuspendedFlow`: the FS-2/FS-3 compiler
synthesis and runtime spill/restore that would populate or consume a
*live* value are still unbuilt, so `Story::save_state`/`load_state` (and
therefore every current `@brink-lang/web` save/load call) still always
produce/consume `suspended: None` — no observable runtime behavior
changes today. The changeset is filed because the wire shape of a type
`@brink-lang/web` re-exports (`SaveState`) changed.
