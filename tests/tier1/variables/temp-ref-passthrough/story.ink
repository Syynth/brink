-> test
=== test
~ temp x = 5
~ double_increment(x)
{x}
-> DONE
=== function double_increment(ref x)
~ increment(x)
~ increment(x)
=== function increment(ref x)
~ x = x + 1
