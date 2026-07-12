VAR log = ""

~ {
    log = log + "A"
}
-> knot

=== knot ===
~ {
    log = log + "B"
}
Entering stitch.
-> inner

= inner
~ {
    log = log + "C"
}
Log is {log}.
-> END
