---
"@brink-lang/web": patch
---

Choice-guard `as` un-deferred (#1508, decision log 2026-07-26): the
native-only `* {if EXPR as name} [text]` binding now compiles and runs for
real, capturing the unwrapped `Option<T>` payload at **presentation
time** (ordinary COW value semantics — the same rule closure capture
uses). The picked choice's own body sees the value the player saw, even
if the same-name source is mutated between the choice appearing and
being picked. `E146` ("not yet supported") is retired — a story that
previously failed to compile on this construct now compiles and runs.

No wire-format change: the guard's binding reuses the same `OptionBind`
opcode and frame-slot machinery `if EXPR as name { … }` already uses
(issue #1475), and the captured value rides the pending choice through
selection via the existing thread-fork snapshot that already restores
tunnel/function temps across a pick — verified end to end, including
across a `StorySnapshot` detach/reattach round trip. `Story`/`Choice`'s
public shape is unchanged. Oracle corpus unaffected (native-only
construct, no ink counterpart).
