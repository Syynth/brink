VAR log = ""

~ {
    temp x = 1
    log = log + "outer=" + x
    if true {
        temp x = 2
        log = log + " mid=" + x
        if true {
            temp x = 3
            log = log + " inner=" + x
        }
        log = log + " mid_after=" + x
    }
    log = log + " outer_after=" + x
}

{log}
-> END
