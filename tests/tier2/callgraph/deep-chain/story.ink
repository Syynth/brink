VAR total = 0
~ total = chain_start()
-> stage_one

=== function chain_start()
~ return chain_two(1)

=== function chain_two(n)
~ return chain_three(n + 1)

=== function chain_three(n)
~ return chain_four(n + 1)

=== function chain_four(n)
~ return chain_five(n + 1)

=== function chain_five(n)
~ return n + 1

=== stage_one ===
Stage one begins, total is {total}.
-> stage_two

=== stage_two ===
Stage two continues.
-> stage_three

=== stage_three ===
Stage three deepens.
-> stage_four

=== stage_four ===
Stage four nears the end.
-> stage_five

=== stage_five ===
The chain concludes.
-> END
