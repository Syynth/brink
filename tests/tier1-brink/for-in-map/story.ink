VAR keys = ""

~ {
    temp m = #{"z": 1, "a": 2, "m": 3}
    for k in m {
        keys = keys + k
    }
}

Keys is {keys}.
-> END
