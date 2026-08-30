---
"@brink-lang/web": patch
---

`debugRun`/`debugStep` outcomes gain a new stop reason,
`{ type: "awaitingExternal" }` (#3224): execution reached a bound
external function whose handler deferred. The `External` frame is left
intact — resolve it out-of-band (`resolveExternal`), then resume with
any debug verb. Synchronously resolved externals and in-story fallbacks
now step through cleanly instead of erroring `UnresolvedExternalCall`
mid-session, and step-into on an external call behaves like step-over
(spec §4 — there is no ink bytecode inside an external frame).
