VAR s = ""

~ {
    s = "café"
}

char_at("hello", 0) = {char_at("hello", 0)}, char_at("hello", 4) = {char_at("hello", 4)}, char_at(s, 0) = {char_at(s, 0)}, char_at(s, 3) = {char_at(s, 3)}
-> END
