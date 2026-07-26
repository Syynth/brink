---
"@brink-lang/web": patch
---

Fix (#1507): editor hover and go-to-definition now report the UFCS
resolution verdict for `recv.verb()` call sites on native `.brink` files,
instead of falling through to the receiver's own binding. The D2 ruling
(#1482) justified the `ufcs_resolution_query` side table partly on this IDE
payoff, but the editor never queried it — hovering or jumping from `verb` in
`recv.verb(args)` showed/jumped to `recv`'s declaration instead.

Hovering the method segment now shows whether the call dispatches through a
struct field (`FieldCall`), a resolved free function (`FreeFnDesugar` /
`FreeFnAutoRef` for D5 by-reference dispatch), or a stdlib/builtin prelude
verb (`PreludeDesugar`); go-to-definition jumps to the free function's
declaration when there is one, and does nothing (rather than jumping to the
receiver) when the verdict has no `DefinitionId` to jump to. The override is
scoped to exactly the method segment's own range — hovering/jumping from the
receiver itself is unaffected.

`EditorSession::goto_definition` (`brink-web`) now needs a `&ProjectDb`
to read the memoized verdict, so its call site now passes one — a
source-compatible addition, no other public signature changes.
