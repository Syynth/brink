~ temp a = vec3(1.0, 2.0, 3.0)
~ temp b = vec3(4.0, 5.0, 6.0)
a is {a}.
sum {a + b}.
diff {b - a}.
prod {a * b}.
scaled {a * 2.0} and {2 * a}.
~ temp n = -a
negated {n}.
dot {dot(a, b)}.
cross {cross(vec3(1.0, 0.0, 0.0), vec3(0.0, 1.0, 0.0))}.
lanes {a.x} {a.y} {a.z}.
~ temp v = vec2(3, 4)
v is {v}, dot {dot(v, v)}.
mins {min(a, b)}, maxes {max(vec2(1.0, 4.0), vec2(3.0, 2.0))}.
clamped {clamp(vec2(0.0 - 1.0, 5.0), vec2(0.0, 0.0), vec2(2.0, 2.0))}.
lerped {lerp(vec2(0.0, 0.0), vec2(10.0, 20.0), 0.5)}.
~ temp q = quat(0.0, 0.0, 0.0, 1.0)
q is {q}.
rotated {q * a}.
~ temp m = mat2(vec2(1.0, 2.0), vec2(3.0, 4.0))
m is {m}.
col {m.y_axis}.
transformed {m * vec2(1.0, 0.0)}.
same {a == vec3(1.0, 2.0, 3.0)}, different {a == b}.
{v == vec2(3.0, 4.0): int lanes promoted.}
-> END
