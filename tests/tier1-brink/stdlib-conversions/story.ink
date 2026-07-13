VAR arr = 0

~ {
    arr = #[1, 2, 3]
}

int(2.9) = {int(2.9)}, int(-2.9) = {int(-2.9)}, int("42") = {int("42")}, int(true) = {int(true)}, int(false) = {int(false)}, int(7) = {int(7)}
float(3) = {float(3)}, float("2.5") = {float("2.5")}, float(true) = {float(true)}, float(false) = {float(false)}, float(1.25) = {float(1.25)}
string(42) = {string(42)}, string(3.14) = {string(3.14)}, string(true) = {string(true)}, string("hi") = {string("hi")}, string(arr) = {string(arr)}
-> END
