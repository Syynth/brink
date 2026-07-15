// SaveState wire-size benchmark (issue #821 Workstream C): the "small"
// story-state shape — a handful of scalar globals, no collections at
// all. The floor point on the wire-size curve: what a save costs when
// there's almost nothing to save. Compare against save-state-medium and
// save-state-large (same directory) to see how per-element collection
// growth dominates over the small, fixed per-save overhead (SaveState's
// own envelope fields: version, turn_index, rng_seed, previous_random,
// empty visits/turns).
VAR hero_name = "hero"
VAR score = 42
VAR health = 100.0
VAR alive = true
Small state ready.
-> END
