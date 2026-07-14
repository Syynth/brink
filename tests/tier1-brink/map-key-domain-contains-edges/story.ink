VAR m = 0

~ {
    m = #{1: "one", "two": 2, true: "yes"}
}

int_key={contains(m, 1)}, string_key={contains(m, "two")}, bool_key={contains(m, true)}, missing_int={contains(m, 99)}, float_needle={contains(m, 3.5)}, array_needle={contains(m, #[1])}, map_needle={contains(m, #{})}
-> END
