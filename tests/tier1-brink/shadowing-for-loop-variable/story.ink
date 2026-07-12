VAR log = ""

~ {
    temp item = 999
    log = log + "before=" + item
    for item in #[1, 2, 3] {
        log = log + " loop=" + item
    }
    log = log + " after=" + item
}

{log}
-> END
