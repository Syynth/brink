STRUCT Bag = #{
    items: Array<int>,
}

VAR a = 0

~ {
    a = Bag#{items: #[1, 2]}
    push(a.items, 3)
    insert(a.items, 0, 0)
    remove_at(a.items, 2)
}

a is {a.items}.
-> END
