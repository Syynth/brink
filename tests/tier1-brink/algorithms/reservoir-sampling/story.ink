// ALGORITHMS CORPUS — randomness lane (issue #822)
// Reservoir sampling (Algorithm R): pick k items uniformly at random from
// a stream of items seen ONE AT A TIME, without knowing the stream's
// total length in advance and without buffering it — e.g. picking a
// random NPC bark from a pool that's generated/streamed rather than
// pre-loaded into one array. This file simulates the "streamed" part with
// a fixed source array walked one element at a time, since brink has no
// actual unbounded-stream input primitive to draw from — the reservoir
// itself never sees more than `k` items at once regardless.
//
// SEEDED RNG NOTE: vanilla ink's `RANDOM`, not this lane's shared
// `pcg.ink` utility — same reasoning as `shuffle-bag`/story.ink's header:
// a single forward pass with no persisted-across-turns RNG state to
// speak of, so the built-in generator is the simpler, sufficient tool.
//
// TYPES POLICY: gradual (default). `array<string>` (the stream/
// reservoir), `int` (loop indices); nothing here needs annotation.
//
// ERGONOMICS-FINDINGS:
// - The single cleanest "house rule made concrete" case in this whole
//   lane: CLAUDE.md's "guard against unbounded growth" rule and
//   reservoir sampling's OWN reason to exist are the same thing — the
//   entire algorithm IS the bounded-memory-over-unbounded-input pattern,
//   not just an example that happens to obey it. `reservoir` never grows
//   past `k` elements no matter how long the simulated stream runs; the
//   VM's step limit is a safety net this file never gets close to
//   needing, because the algorithm's own invariant already bounds memory
//   independently of it.
// - `RANDOM(0, i)` (inclusive, per `fisher-yates-shuffle`'s already-
//   documented convention) is EXACTLY the textbook Algorithm R formula's
//   `random(0, i)` with no off-by-one translation — the second lane
//   entry (after `fisher-yates-shuffle`) where ink's inclusive-range
//   convention matches a textbook algorithm's own convention verbatim.
// - No stdlib deque/stream type exists (unsurprising — ink has no I/O
//   primitive to stream from in the first place), so "streamed" here is
//   entirely a simulation: `source` is a plain fully-materialized array,
//   walked index-by-index as if arriving one at a time. This is a
//   faithful demonstration of the ALGORITHM's shape (bounded reservoir,
//   one pass, replace-with-decreasing-probability) even though the
//   "unbounded stream" framing is aspirational for a language with no
//   stream I/O to begin with — worth flagging as a scope boundary for
//   whatever this epic's book-appendix writeup eventually says about
//   this entry, so it doesn't overclaim.

CONST RESERVOIR_SIZE = 3

VAR source = 0
VAR reservoir = #[]

~ {
    SEED_RANDOM(5501)
    source = #["intro_bark_a", "intro_bark_b", "combat_taunt_a", "combat_taunt_b", "combat_taunt_c", "victory_line_a", "victory_line_b", "flee_line_a", "flee_line_b", "flee_line_c", "flee_line_d", "idle_mutter_a"]

    temp i = 0
    while i < len(source) {
        if i < RESERVOIR_SIZE {
            push(reservoir, source[i])
        } else {
            temp j = RANDOM(0, i)
            if j < RESERVOIR_SIZE {
                reservoir[j] = source[i]
            }
        }
        i = i + 1
    }
}

Stream length: {len(source)}. Reservoir size: {RESERVOIR_SIZE}.
Reservoir (uniform sample of the whole stream): {reservoir}.
-> END
