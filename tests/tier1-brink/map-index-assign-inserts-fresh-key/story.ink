VAR memo = 0

~ {
    memo = #{}
    memo["a"] = 1
    memo["a"] = 2
    memo["b"] = 3
}

fresh_a={memo["a"]}, fresh_b={memo["b"]}, size={len(memo)}
-> END
