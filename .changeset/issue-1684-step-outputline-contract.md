---
"@brink-lang/web": patch
---

Track 1 step 4 (#1684): the runtime output contract migrates from `Line`
to `Step`/`OutputLine`, per `docs/prose-dialect-spec.md` §7 (RULED).

- **Terminals carry no payload.** `Step::Choices`/`Step::Done`/`Step::End`/
  `Step::Suspended` no longer bundle trailing text — any content that used
  to fuse onto a terminal event now arrives first as its own ordinary
  `Step::Line(OutputLine)`, and the bare terminal follows on the very next
  `continue_single`/`advance` call.
- **`block_id` is new.** Every `OutputLine` carries a `BlockId` — an opaque
  id grouping the uninterrupted run of adjacent content it belongs to
  (`docs/prose-dialect-spec.md` §3.7/§8d.2). In today's schema-less-ink
  degenerate case this simply counts runs between turn boundaries (a choice
  selection, a `Done` resume, or a host-directed jump); the richer
  attachment-derived assignment rides the element/markup layer (#1683).
- **`@brink-lang/web` wire shape**: the exported `Line` JSON union keeps its
  existing `type` discriminants (`"text"`/`"choices"`/`"done"`/`"end"`/
  `"suspended"`/`"awaiting_external"`), but terminal variants now always
  serialize `text: ""`/`tags: []` instead of fused content, and the
  `"text"` variant gains an additional `block_id` (number) field. Any
  trailing content a host displayed by reading a terminal's `text` field
  must instead be read off the preceding `"text"` message.
- **Ratchet unaffected by construction**: `termination.rs::push_terminal`
  (the test harness's terminal-classification fold, reserved for this
  exact migration since PR #1513) now stamps a terminal's classification
  onto the harness's last open step (or synthesizes an empty one if none
  precedes it in the turn) instead of being a pass-through — this keeps
  oracle episode comparison behavior-identical across the split.
