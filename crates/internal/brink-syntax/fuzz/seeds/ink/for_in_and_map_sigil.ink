VAR keys = ""
VAR total: int = 0

~ {
    temp m = #{"z": 1, "a": 2, "m": 3}
    for k in m {
        keys = keys + k
        total = total + m[k]
    }
}

Keys is {keys}. Total is {total}.
-> END
