---
"@brink-lang/web": patch
---

Debugger D7 (issue #3185): the runtime can now name a call frame's live
temps/parameters, not just count them. `DebugFrame` (`brink-runtime`) gains
an additive `locals: Option<Vec<DebugLocal>>` — `slot`/`name`/`value` per
declared `~ temp` or parameter currently in scope, resolved from D6's
`DebugInfo` `LocalsTable` (populated here — D6 shipped only the structural
framing) against the call frame's own `temps`. `None` when the linked
program carries no `DebugInfo`; `Some(vec![])` when it does but the frame
genuinely has no locals, so a consumer can tell the two cases apart. The
existing `temps: usize` count is unchanged.

Values are exposed structurally, not as another display string like
`DebugGlobal.value`: the new `DebugValue` enum models every kind the debug
surface can currently distinguish by name (int, float, bool, string, null,
list — member names, divert target — resolved path, struct — shape name
plus named fields, recursively, and handle — kind plus id), falling back to
the existing display-string form only for the long tail of kinds with no
dedicated variant yet.

`DebugState`'s JSON (`debug_snapshot()`/`flow_debug_snapshot()` on
`EditorSession`/`StoryRunner`) carries this same `locals` field on each
call-stack frame, and `@brink/wasm-types` gains the matching `DebugLocal`/
`DebugValue`/`DebugField` mirror types (a `type`-tagged union for
`DebugValue`, so a JS consumer can `switch` on it). This is a wire-
observable addition (new optional key plus new exported types), so it
needs this changeset. Nothing renders it yet: the State View locals panel
(#3140) is separate follow-up UI work that consumes this surface, not part
of this PR.
