---
"@brink-lang/studio": minor
---

Live inspector and host-aware authoring.

- The story session is driven by a `SessionProvider`, so the transcript, State
  View, and Story Graph render against a provider rather than the wasm runner
  directly — the groundwork for inspecting a VM running in a host.
- Capability-gated session commands, program-identity degraded mode, and
  multi-session support (independent runners + shared-context flows) with a
  session/flow picker.
- A host-aware argument picker: a value dropdown and inline value labels for
  `EXTERNAL` arguments whose semantic type declares a value source (static, or
  pushed live by a host), plus a `StudioExtensions.argumentProviders` surface
  for embedders to supply those values.
