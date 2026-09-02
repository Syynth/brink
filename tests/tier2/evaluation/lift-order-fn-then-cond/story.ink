VAR n = 0
-> k
=== function bump() ===
~ n = n + 1
~ return n
=== k ===
{bump()}{n == 1:yes|no}
-> END
