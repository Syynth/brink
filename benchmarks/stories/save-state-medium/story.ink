// SaveState wire-size benchmark (issue #821 Workstream C): the "medium"
// story-state shape — a few scalars plus one moderate-size array (500
// ints), representing a modest inventory/log-sized collection. See
// save-state-small/story.ink's header for the corpus's overall intent.
VAR hero_name = "hero"
VAR score = 42
VAR inventory = 0
~ {
    inventory = #[]
    temp i = 0
    while i < 500 {
        push(inventory, i)
        i = i + 1
    }
}
Medium state ready.
-> END
