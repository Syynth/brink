# Sessions & Replay

A `Story` runs a story; a **`StorySession`** wraps a `Story` and remembers how
it was run. It journals every input that entered the VM — the start, each
choice, each external result, each host mutation — as durable, serializable
data. From that journal you get three things the bare `Story` can't offer: a
save file, deterministic replay, and the ability to detect when a story edit has
invalidated an old save.

The journaling lives at the session boundary, not in the VM. The step loop never
learns it's being recorded — the session observes inputs at the same seam the VM
receives them, so this composes over `Story` rather than threading an
`if recording` branch through the hot path.

## Creating and driving a session

Wrap a `Story`, optionally with a seed for reproducible RNG, and drive it with
the same verbs — the session records as it goes:

```rust
# extern crate brink_runtime;
# use brink_runtime::{Step, RuntimeError, Story, StorySession};
# fn demo(story: Story) -> Result<(), RuntimeError> {
let mut session = StorySession::new(story, Some(42));

loop {
    match session.continue_single()? {
        Step::Choices(choices) => {
            session.choose(choices[0].index)?;   // journaled
        }
        Step::End => break,
        _ => {}                                  // Line / Done — keep going
    }
}
# Ok(())
# }
```

`continue_single`, `continue_to_pause`, `choose`, `advance`, and
`resolve_external` all mirror their `Story` counterparts and journal their
inputs. `story()` / `story_mut()` expose the wrapped `Story` as a deliberate
escape hatch — anything done through them **bypasses the journal**.

Host mutations — `set_var`, `go_to_path`, `load_state` — are **turn-boundary
only**. Called mid-turn (while more content is pending) they return
`SessionError::MutationMidTurn` rather than being silently queued, which keeps
the journal's event order unambiguous. Drain the current turn to a
`Done`/`Choices`/`End` before mutating.

## The journal as a save file

The journal *is* the durable save artifact. It serializes to JSON via serde —
values are tagged, so a `List` or divert survives the round-trip without a lossy
collapse to null.

```rust
# extern crate brink_runtime;
# use brink_runtime::StorySession;
# fn demo(session: &mut StorySession) {
let journal = session.export_journal();   // serde-serializable; write it to disk
# let _ = journal;
# }
```

To load, hand a fresh `Story` and the journal back to `StorySession::restore`.
It fast-paths through an embedded checkpoint when the program is unchanged, and
falls back to full replay otherwise. The journal is capped
(`SESSION_JOURNAL_CAP`) so a pathologically long session degrades honestly —
past the cap, appends drop and restore leans on the checkpoint — rather than
growing without bound.

## Replay and divergence

`StorySession::replay` re-runs a journal against a `Story` from a fresh start,
consuming the recorded inputs event by event. Its `ReplayOutcome` is where the
"did my edit break this save?" answer lives:

- **`Replayed`** — the whole prefix applied cleanly (with soft `warnings` like
  `ChoiceLabelDrift`, when a choice still replays by index but its text has
  changed).
- **`Diverged { at_event, expected, found }`** — a recorded event no longer
  applies to the current program: a choice index that's now out of range, a path
  that no longer resolves. The journal is truncated at that point and the session
  parks at the position it reached.
- **`Failed`** — replay stopped for a non-divergence reason.

`ExternalReplayMode` picks whether externals are served from the journal
(`Recorded`) or called live (`Live`) during replay. Live replay that hits a
deferred external parks, retaining the un-replayed tail; resolve it and resume
with `continue_replay`.

## Snapshots and diffs

For inspecting *state* rather than replaying *inputs*, a session takes a typed
`StateSnapshot` — globals with their real `Value`s (list membership included),
visit counts and turn counts by resolved path, a call-stack summary, and status.
`diff` compares two of them:

```rust
# extern crate brink_runtime;
# use brink_runtime::{diff, RuntimeError, StorySession};
# fn demo(session: &mut StorySession) -> Result<(), RuntimeError> {
let before = session.snapshot();
session.continue_single()?;
let after = session.snapshot();

let delta = diff(&before, &after);   // a is "before", b is "after"
if !delta.is_empty() {
    // delta.changed_globals: name -> (before, after)
    // delta.list_deltas, delta.pushed_frames, delta.popped_frames, …
}
# Ok(())
# }
```

`StateSnapshot` is a **typed serialization path** — its globals keep their
`Value`s, so it round-trips losslessly and a diff can report `(before, after)`
pairs. It has one known projection limit: visit/turn counts for anonymous
counted containers (gathers, choice points with no author path) are omitted from
the path-keyed view; the full id-keyed counts remain in `save_state`.

## Live inspection

For a "state view" UI — a running debugger panel showing where the story is
right now — a `Story` gives you a `DebugSnapshot` directly, no session required:

```rust
# extern crate brink_runtime;
# use brink_runtime::Story;
# fn demo(story: &Story) {
let snap = story.debug_snapshot();
// snap.current_location, snap.position, snap.globals, snap.call_stack,
// snap.visit_counts, snap.pending_choices, snap.rng
# let _ = snap;
# }
```

`DebugSnapshot` is deliberately *not* `StateSnapshot`. It's a read-only,
name-resolved view for display: values are formatted to strings, frames and
visit counts resolve to author-facing knot/stitch paths, and the whole thing is
built on demand off any hot path. Use `DebugSnapshot` to *show* current state to
a developer; use `StateSnapshot` + `diff` to *serialize and compare* state
programmatically. This is the surface the Studio's state view is built on — see
[Studio](../../integrations/studio/index.md).

## Sessions vs. speculation

A session records and reproduces the **real** playthrough — the moves that
actually happened, replayable and diffable. [Speculation](./speculation.md) runs
a **throwaway** branch off the present and discards it. Use a session to save,
replay, and inspect what happened; use a speculation to preview what *would*
happen without it happening.
