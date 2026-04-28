INCLUDE characters.ink

You wake at the edge of a quiet shoreline.
The tide is unhurried; the gulls are not.

{greeting()}

// Two styles of choice text:
//   * Plain text — displayed in the choice list AND emitted as content
//     into the transcript on pick.
//   * [Bracketed] — displayed in the choice list, NOT emitted on pick.
* You walk toward the lighthouse. -> lighthouse
* You sit and watch the waves roll in. -> watch
* [Leave] -> END

=== lighthouse ===
You climb the wooden stairs.
Each step creaks like a small announcement.
At the top, a stranger waits.
"You came," they say. "I wasn't sure you would."
-> END

=== watch ===
The waves do what waves do — arrive, then leave, then arrive again.
-> END
