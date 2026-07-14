VAR arr = 0
VAR result = 0

~ {
    arr = #[10, 20, 30]
    arr[0] = arr[1] + arr[0]
    arr[2] = arr[2] + arr[2]
    result = arr[0] + arr[1] + arr[2]
}

Result is {result}. Arr is {arr}.
-> END
