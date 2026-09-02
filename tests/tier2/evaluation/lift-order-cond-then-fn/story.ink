VAR n = 0
-> k
=== function bump() ===
~ n = n + 1
~ return n
=== k ===
{n == 1:yes|no}{bump()}
-> END
