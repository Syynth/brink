// SaveState wire-size benchmark (issue #821 Workstream C): the "large"
// story-state shape — a few scalars plus one large array (10,000 ints),
// representing a long-running session's accumulated log/history
// collection. See save-state-small/story.ink's header for the corpus's
// overall intent.
VAR hero_name = "hero"
VAR score = 42
VAR log = 0
~ {
    log = #[]
    temp i = 0
    while i < 10000 {
        push(log, i)
        i = i + 1
    }
}
Large state ready.
-> END
