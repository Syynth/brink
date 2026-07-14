---
"@brink-lang/web": patch
---

Close the `FlowInstance`-level host visibility gap left by M-2b (#772/#781):
`begin_function_eval`/`begin_function_value_eval`/`choose_path_string(_with_args)`
now refuse `#@private` definitions on any `FlowInstance` driven directly,
not just through `Story`.

- **`WebSpeculation.goToPath`/`.evalFunction`/`.resumeFunctionEval`** (the
  wasm bindings over `brink_runtime::Speculation`, which drives a
  `FlowInstance` clone directly rather than a `Story`) now correctly refuse
  a `#@private` knot or function with the same `PrivateAccess` error the
  `StoryRunner`-level `go_to_path`/`call_function` surface already enforced
  — previously a speculative fork could read past a private boundary that a
  live `Story`-mediated session already blocked.
- Same documented dev-tooling override: a `FlowInstance`'s own visibility
  enforcement flag mirrors `Story`'s, and `Story` keeps every flow it owns
  (default, named, shared) synced to its own setting, so a `Story`-level
  `setDevVisibilityOverride`/play-from-here session behaves identically
  whether or not it composes a `Speculation`.
