INCLUDE characters.ink

// ── External functions (resolved by the engine) ───────────────────────
// These are declared here and bound in main():
//   • play_sound(name) — bind_brink_command::<(), PlaySound>: fires a
//     Bevy PlaySound event (fire-and-forget). Returns nothing to ink.
//   • shout(text) — bind_brink_fn: a pure transform; returns the text
//     uppercased, inlined into story output below.
EXTERNAL play_sound(name)
EXTERNAL shout(text)

// Fire-and-forget: ask the engine to start the ambient loop. The `~`
// statement form evaluates and discards the (null) return.
~ play_sound("waves_loop")

You wake at the edge of a quiet shoreline.
The tide is unhurried; the gulls are not.

{greeting()}

// Choice text comes in two flavors:
//   * Plain text — displayed in the choice list AND emitted as content
//     into the transcript on pick.
//   * [Bracketed] — displayed in the choice list, NOT emitted on pick.
//
// Note the indented divert form: putting `-> knot` on its own line under
// the choice preserves the newline at the end of the emitted content.
// (Putting `-> knot` on the same line as the content glues per ink
// semantics — the choice text would run into the next knot's first line.)
* You walk toward the lighthouse.
  -> lighthouse
* You sit and watch the waves roll in.
  -> watch
* [Leave] -> END

=== lighthouse ===
~ play_sound("door_creak")
You climb the wooden stairs.
Each step creaks like a small announcement.
At the top, a stranger waits.
// `shout` is a pure binding: "you came" comes back as "YOU CAME".
"{shout("you came")}," they say. "I wasn't sure you would."
-> END

=== watch ===
~ play_sound("gentle_surf")
The waves do what waves do — arrive, then leave, then arrive again.
-> END
