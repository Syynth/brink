VAR a = "A"
VAR b = "B"
-> k
=== k
Alt: { {a} {b} | c }.
Cond: {true: {a} {b}|c}.
Cycle: {&x {a} {b} y|c}.
Comment: {{a} /* c */ {b}|c}.
Escape: {\# {a}|c}.
Trailing: { {a} | c}.
-> END
