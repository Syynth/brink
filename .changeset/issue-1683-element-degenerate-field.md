---
"@brink-lang/web": patch
---

#1683 (partial — the "element kind + per-line element data" payload):
`brink_runtime::Element` is new — `{ kind: String, data: BTreeMap<String,
String> }`, added as `OutputLine.element` alongside the existing
`text`/`tags`/`block_id` fields. Every line reports the degenerate
`Element::narrative()` case (`kind: "narrative"`, empty `data`) — this PR
wires the type and field through the runtime and the `@brink-lang/web`
marshal layer (`LineJs`/`ElementJs`, both the legacy `Line` union and the
`StorySession` `SessionLine` shape) so the schema exists and is stable, but
does **not** yet populate it from an `@[element]` handler's classification
(kind = handler name, data = its named captures). That population needs
either new `.inkb` line-table storage (for a single-line, return-based
handler like `heading`/`transition`) or a VM-level scoping mechanism (for a
`block`-capturing handler like `cue`/`parenthetical`, whose call emits more
than one line dynamically) — neither is built here; see the tracked
follow-up linked from #1683. `@brink-lang/web` consumers reading
`Line.element`/`SessionLine.element` today always see the narrative
default regardless of source markup.
