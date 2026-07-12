VAR label = ""

~ {
    temp score = 72
    if score >= 90 {
        label = "A"
    } else if score >= 80 {
        label = "B"
    } else if score >= 70 {
        label = "C"
    } else {
        label = "F"
    }
}

Grade: {label}.
-> END
