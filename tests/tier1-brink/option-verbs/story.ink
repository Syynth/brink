~ temp s = "hello world"
~ temp f1 = find(s, "world")
~ temp f2 = find(s, "xyz")
f1 is {f1}.
f2 is {f2}.
{f1 == some(6): f1 matched at six.}
{f2 == none: f2 is absent.}
~ temp a = #[3, 1, 2]
index of 1 is {index_of(a, 1)}, of 9 is {index_of(a, 9)}.
min {min(a)}, max {max(a)}.
first {first(a)}, last {last(a)}.
~ temp p = pop(a)
popped {p}, leaving {len(a)}.
~ temp m = #{"hp": 10, "mp": 5}
hp {get(m, "hp")}, armor {get(m, "armor")}.
has ten: {contains_value(m, 10)}, has seven: {contains_value(m, 7)}.
~ clear(m)
cleared to {len(m)} entries.
~ f2 = some(0)
{f2 == some(0): f2 holds zero now.}
-> END
