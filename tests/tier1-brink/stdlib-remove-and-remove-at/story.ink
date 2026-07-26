VAR arr = 0
VAR m = 0

~ {
    arr = #[1, 2, 3]
    remove(arr, 1)
    m = #{"a": 1, "b": 2}
    remove(m, "a")
}

Arr is {arr}. Map is {m}.
-> END
