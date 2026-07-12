VAR sum = 0

~ {
    temp i = 0
    while true {
        i = i + 1
        if i > 10 {
            break
        }
        if i mod 2 == 0 {
            continue
        }
        sum = sum + i
    }
}

Sum is {sum}.
-> END
