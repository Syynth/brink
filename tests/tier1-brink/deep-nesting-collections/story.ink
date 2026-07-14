VAR data = 0
VAR result = 0

~ {
    data = #[#{"a": #[1, 2, 3], "b": #[4, 5, 6]}, #{"a": #[7, 8, 9], "b": #[10, 11, 12]}]
    data[0]["a"][2] = 30
    result = data[0]["a"][2] + data[1]["b"][0] + data[0]["b"][1]
}

Result is {result}.
-> END
