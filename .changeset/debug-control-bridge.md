---
"@brink-lang/web": patch
"@brink-lang/studio": patch
---

Debugger D8's control half (issue #3186) bridged through wasm to the studio
(#3232) — D9 (#3187) bridged only the read half (program → source position
resolution). `StoryRunnerHandle` and `StorySessionHandle` (`@brink-lang/web`)
gain `debugRun`/`debugStep`/`debugBreakpointAdd`/`debugBreakpointRemove`/
`debugBreakpointSetEnabled`/`debugBreakpoints`, wrapping the runtime's
`Story::debug_run`/`debug_step`/`BreakpointSet` (feature `debug-hooks`, now
built unconditionally into the `brink-web` wasm package rather than a
build-time toggle nothing in the studio's pipeline passes). `@brink/wasm-types`
gains the `Breakpoint`/`DebugRunOutcome`/`DebugStopReason`/`StepMode` wire
shapes.

`@brink/studio-store` gains a `DebugSessionProvider` capability extension on
`SessionProvider` (one extension covering both pause/step/breakpoints and
D9's previously-uncaptured position-resolution capability, per the issue),
implemented by `LocalSessionProvider`, plus a new debug slice
(`debugCapable`/`debugBreakpoints`/`debugLastOutcome`/`debugStatus`) and the
`debug.run`/`debug.stepInto`/`debug.stepOver`/`debug.stepOut`/`debug.
breakpointAdd`/`debug.breakpointRemove`/`debug.breakpointToggle` commands,
registered alongside `story.*` at the app boundary.

**Scope honesty**: this is real, working plumbing (proven over a real
`WebSession` in `crates/brink-web/src/session.rs`'s `debug_control_tests`,
plus a vitest suite over the store slice) — but the studio still cannot
compile a project WITH debug info at all (#3229, a separate, un-made
maintainer ruling on the toggle mechanism), so an end user will not see any
of this working yet. No UI consumes the new slice either — the editor
gutter / current-line highlight is a separate, later ticket. This PR lands
the bridge ahead of #3229 because the plumbing is independent of which
toggle mechanism wins.
