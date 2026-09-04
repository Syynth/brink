---
"@brink-lang/studio": patch
---

Prose checking no longer freezes the editor. The studio's `ProseChecker`
now runs the `brink-prose` wasm module inside a Web Worker, which lazily
imports it there on the first check — so an embedder that never checks
prose still downloads nothing, and a check no longer blocks input for
the length of the document. A check superseded by a newer edit is
dropped before it is posted rather than queued behind the one in flight.

Measured on the 8k-line perf fixture, before and after on one machine:
the long task co-located with a check falls from 6,465 ms to 184 ms
(fast-scroll), and the worst long task of a typing burst from 6,689 ms
to 760 ms. The check itself is not the point — its latency is roughly
unchanged, and a cold first check can be slower (worker spawn plus a
second instantiation of the 6.5 MB module); it simply no longer runs
where keystrokes are handled, and the cached dictionary and rule set pay
the cold cost back from the second check on.

Environments with no `Worker` (jsdom, a bundler that leaves the
`new URL(..., import.meta.url)` shape alone) and a crashed worker fall
back to the previous in-process road, so checking degrades in speed
rather than stopping — from the check after the failure, since a boot
failure surfaces asynchronously and rejects the check already in flight
(#3491).
