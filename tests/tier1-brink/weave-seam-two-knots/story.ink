VAR counter = 0

~ {
    counter = counter + 1
}
Start counter is {counter}.
-> middle

=== middle ===
~ {
    counter = counter + 10
}
Middle counter is {counter}.
-> ending

=== ending ===
~ {
    counter = counter + 100
}
End counter is {counter}.
-> END
