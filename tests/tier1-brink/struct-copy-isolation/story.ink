STRUCT Point = #{
    x: float,
    y: float,
}

VAR a = 0
VAR b = 0

~ {
    a = Point#{x: 1.0, y: 2.0}
    b = a
    a.x = 99.0
}

a is {a.x} {a.y}. b is {b.x} {b.y}.
-> END
