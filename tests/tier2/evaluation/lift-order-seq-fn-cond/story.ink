VAR n = 0
-> k
=== function bump() ===
~ n = n + 1
~ return n
=== k ===
+ [again]
    {a|b|c}{bump()}{n > 1:big|small}
    -> k
