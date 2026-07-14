VAR arr = 0
VAR m = 0

~ {
    arr = #[1, 3]
    insert(arr, 1, 2)
    m = #{"a": 1}
    insert(m, "b", 2)
}

Arr is {arr}. Map is {m}.
-> END
