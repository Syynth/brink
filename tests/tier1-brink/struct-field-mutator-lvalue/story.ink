STRUCT Bag = #{
    items: Array<int>,
}

VAR a = 0
VAR b = 0

~ {
    a = Bag#{items: #[1, 2]}
    b = a
    push(a.items, 3)
    insert(a.items, 0, 0)
    remove_at(a.items, 2)
}

~ temp c = Bag#{items: #[1]}
~ push(c.items, 2)
~ insert(c.items, 0, 0)
~ remove_at(c.items, 2)

a is {a.items}. b is {b.items}. c is {c.items}.
-> END
