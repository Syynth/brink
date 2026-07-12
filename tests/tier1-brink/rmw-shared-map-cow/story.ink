VAR base = 0
VAR shared = 0

~ {
    base = #{"a": 1, "b": 2}
    shared = base
    insert(shared, "c", 3)
    remove(shared, "a")
}

base={base} shared={shared}
-> END
