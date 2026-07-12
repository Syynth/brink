VAR total = 0
VAR touches = 0

~ {
    temp i = 0
    while i < 3 {
        temp j = 0
        while j < 3 {
            temp k = 0
            for x in #[1, 2, 3] {
                if (i + j + k + x) mod 2 == 0 {
                    if i == j {
                        if j == k {
                            total = total + 100
                        } else {
                            total = total + 10
                        }
                    } else {
                        total = total + 1
                    }
                }
                touches = touches + 1
            }
            j = j + 1
        }
        i = i + 1
    }
}

Total is {total}. Touches is {touches}.
-> END
