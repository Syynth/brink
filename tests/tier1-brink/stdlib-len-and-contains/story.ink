VAR arr = 0
VAR m = 0

~ {
    arr = #[10, 20, 30]
    m = #{"a": 1, "b": 2}
}

len(arr) = {len(arr)}, len(m) = {len(m)}, contains(arr, 20) = {contains(arr, 20)}, contains(arr, 99) = {contains(arr, 99)}, contains(m, "a") = {contains(m, "a")}, contains(m, "z") = {contains(m, "z")}, len("cider") = {len("cider")}, len("café") = {len("café")}
-> END
