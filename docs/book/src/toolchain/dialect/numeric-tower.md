# The Numeric Tower

The Last Light inn sits on the edge of a vast forest. The guard plots a course on the map:

```ink
~ temp position = vec3(0.0, 0.0, 0.0)
~ temp waypoint = vec3(1.0, 1.0, 0.0)

The guard's current post is {position}.
The next waypoint is {waypoint}.
Scaled to half-distance: {waypoint * 0.5}.
~ temp rotation = quat(0.0, 0.0, 0.0, 1.0)
After rotation: {rotation * waypoint}.
-> END
```

```text
The guard's current post is vec3 { x: 0, y: 0, z: 0 }.
The next waypoint is vec3 { x: 1, y: 1, z: 0 }.
Scaled to half-distance: vec3 { x: 0.5, y: 0.5, z: 0 }.
After rotation: vec3 { x: 1, y: 1, z: 0 }.
```

Beyond scalars — whole numbers, fractions, true/false, text — brink offers a closed numeric tower: **vectors, quaternions, and matrices**. All f32-backed, all `glam`-implemented, all supporting componentwise operations. They are never orderable (you cannot sort vectors), and they save as explicit lane data, never as their in-memory shape.

## Vectors

A **vector** is an ordered sequence of lanes. The tower has three: `vec2` (two f32 values), `vec3` (three), and `vec4` (four). Construct them with explicit lanes:

```ink
~ temp a = vec2(3.0, 4.0)
~ temp b = vec3(1.0, 0.0, 0.0)
~ temp c = vec4(0.0, 1.0, 0.0, 1.0)
The vector a is {a}.
The vector b is {b}.
The vector c is {c}.
-> END
```

```text
The vector a is vec2 { x: 3, y: 4 }.
The vector b is vec3 { x: 1, y: 0, z: 0 }.
The vector c is vec4 { x: 0, y: 1, z: 0, w: 1 }.
```

Read a single lane by name: `a.x`, `a.y`, `a.z`, `a.w`. Lanes are f32; when you interpolate or compare them, they follow float rules.

```ink
~ temp v = vec2(3.5, 4.5)
X is {v.x}, Y is {v.y}.
-> END
```

```text
X is 3.5, Y is 4.5.
```

### Componentwise operations

When you add, subtract, or multiply two vectors of the same kind, the operation applies **to every lane**:

```ink
~ temp a = vec2(1.0, 2.0)
~ temp b = vec2(3.0, 4.0)
a + b = {a + b}.
a - b = {a - b}.
a * b = {a * b}.
-> END
```

```text
a + b = vec2 { x: 4, y: 6 }.
a - b = vec2 { x: -2, y: -2 }.
a * b = vec2 { x: 3, y: 8 }.
```

Multiply or divide a vector by a scalar and every lane gets the same factor:

```ink
~ temp v = vec2(2.0, 3.0)
v * 2.0 = {v * 2.0}.
2.0 * v = {2.0 * v}.
v / 2.0 = {v / 2.0}.
-> END
```

```text
v * 2.0 = vec2 { x: 4, y: 6 }.
2.0 * v = vec2 { x: 4, y: 6 }.
v / 2.0 = vec2 { x: 1, y: 1.5 }.
```

Negate a vector and every lane negates:

```ink
~ temp v = vec2(1.0, 2.0)
Negated: {-v}.
-> END
```

```text
Negated: vec2 { x: -1, y: -2 }.
```

### Dot and cross products

The `dot` verb computes the dot product of two vectors of the same kind. The result is a single float:

```ink
~ temp a = vec3(1.0, 0.0, 0.0)
~ temp b = vec3(0.0, 1.0, 0.0)
dot(a, b) = {dot(a, b)}.
dot(a, a) = {dot(a, a)}.
-> END
```

```text
dot(a, b) = 0.
dot(a, a) = 1.
```

The `cross` verb computes the cross product of two `vec3` vectors, returning a `vec3`:

```ink
~ temp i = vec3(1.0, 0.0, 0.0)
~ temp j = vec3(0.0, 1.0, 0.0)
i × j = {cross(i, j)}.
j × i = {cross(j, i)}.
-> END
```

```text
i × j = vec3 { x: 0, y: 0, z: 1 }.
j × i = vec3 { x: 0, y: 0, z: -1 }.
```

### The scalar kit on vectors

The scalar math functions — `min`, `max`, `clamp`, `lerp`, `abs`, `sqrt`, and more — all work on vectors **componentwise**. When you apply `min` to two vectors of the same kind, each lane takes the minimum:

```ink
~ temp a = vec2(1.0, 4.0)
~ temp b = vec2(3.0, 2.0)
min(a, b) = {min(a, b)}.
max(a, b) = {max(a, b)}.
-> END
```

```text
min(a, b) = vec2 { x: 1, y: 2 }.
max(a, b) = vec2 { x: 3, y: 4 }.
```

Clamp is the same: each lane is clamped between the low and high bounds:

```ink
~ temp v = vec2(0.5, 2.5)
~ temp lo = vec2(0.0, 1.0)
~ temp hi = vec2(1.0, 2.0)
clamp(v, lo, hi) = {clamp(v, lo, hi)}.
-> END
```

```text
clamp(v, lo, hi) = vec2 { x: 0.5, y: 2 }.
```

Linear interpolation works the same way: `lerp(a, b, t)` interpolates from `a` to `b` using `t` (0 to 1) — componentwise:

```ink
~ temp start = vec2(0.0, 0.0)
~ temp end = vec2(10.0, 20.0)
lerp(start, end, 0.5) = {lerp(start, end, 0.5)}.
-> END
```

```text
lerp(start, end, 0.5) = vec2 { x: 5, y: 10 }.
```

## Quaternions

A **quaternion** represents a 3D rotation. Construct one with four floats in the order `(x, y, z, w)`:

```ink
~ temp q = quat(0.0, 0.0, 0.0, 1.0)
Identity rotation: {q}.
-> END
```

```text
Identity rotation: quat { x: 0, y: 0, z: 0, w: 1 }.
```

### Quaternion composition

Compose two quaternions by multiplying them — the result is a new quaternion representing both rotations:

```ink
~ temp q1 = quat(0.0, 0.0, 0.0, 1.0)
~ temp q2 = quat(0.0, 0.0, 0.0, 1.0)
q1 * q2 = {q1 * q2}.
-> END
```

```text
q1 * q2 = quat { x: 0, y: 0, z: 0, w: 1 }.
```

### Rotating vectors

Multiply a quaternion by a vector to rotate the vector by that rotation:

```ink
~ temp rotation = quat(0.0, 0.0, 0.0, 1.0)
~ temp v = vec3(1.0, 0.0, 0.0)
rotated = {rotation * v}.
-> END
```

```text
rotated = vec3 { x: 1, y: 0, z: 0 }.
```

The scalar kit applies to quaternions too: `lerp` blends two rotations, though this is the spherical interpolation (slerp) performed internally by `glam`.

## Matrices

The tower includes column-major matrices of three sizes: `mat2` (2×2), `mat3` (3×3), and `mat4` (4×4). All are f32.

Construct a matrix by passing column vectors:

```ink
~ temp m = mat2(vec2(1.0, 0.0), vec2(0.0, 1.0))
Identity: {m}.
-> END
```

```text
Identity: mat2 { x_axis: vec2 { x: 1, y: 0 }, y_axis: vec2 { x: 0, y: 1 } }.
```

Read a single column by name: `m.x_axis`, `m.y_axis`, `m.z_axis` (for mat3/mat4), `m.w_axis` (for mat4). Each column is a vector:

```ink
~ temp m = mat2(vec2(1.0, 2.0), vec2(3.0, 4.0))
First column: {m.x_axis}.
Second column: {m.y_axis}.
-> END
```

```text
First column: vec2 { x: 1, y: 2 }.
Second column: vec2 { x: 3, y: 4 }.
```

### Matrix × vector

Multiply a matrix by a vector to transform the vector:

```ink
~ temp m = mat2(vec2(1.0, 0.0), vec2(0.0, 1.0))
~ temp v = vec2(3.0, 4.0)
m * v = {m * v}.
-> END
```

```text
m * v = vec2 { x: 3, y: 4 }.
```

### Matrix × matrix

Multiply two matrices of the same size to compose them:

```ink
~ temp m1 = mat2(vec2(1.0, 0.0), vec2(0.0, 1.0))
~ temp m2 = mat2(vec2(0.0, 1.0), vec2(-1.0, 0.0))
m1 * m2 = {m1 * m2}.
-> END
```

```text
m1 * m2 = mat2 { x_axis: vec2 { x: 0, y: 1 }, y_axis: vec2 { x: -1, y: 0 } }.
```

### Matrix scaling

Scale a matrix by a scalar — every element of every column gets the factor:

```ink
~ temp m = mat2(vec2(1.0, 2.0), vec2(3.0, 4.0))
m * 2.0 = {m * 2.0}.
-> END
```

```text
m * 2.0 = mat2 { x_axis: vec2 { x: 2, y: 4 }, y_axis: vec2 { x: 6, y: 8 } }.
```

## Equality and ordering

Two tower values are equal if all their corresponding lanes are equal, following IEEE rules. A vector with a NaN lane never equals itself, and `-0.0 == +0.0` per lane:

```ink
~ temp a = vec2(1.0, 2.0)
~ temp b = vec2(1.0, 2.0)
a == b? {a == b}.
a == vec2(1.0, 3.0)? {a == vec2(1.0, 3.0)}.
-> END
```

```text
a == b? true.
a == vec2(1.0, 3.0)? false.
```

**Tower values are not orderable.** You cannot use `<`, `>`, `<=`, or `>=` on them, and they cannot be sorted. If you reach for an ordering operation on a vector or matrix, you get a compile error. This is by design — there is no universal "less than" on vectors, and the language does not invent one.

```ink,error[E156]
~ temp v = vec2(1.0, 2.0)
{v < vec2(2.0, 3.0)}
-> END
```

## Saves and wire format

When a story saves, tower values are serialized as explicit, little-endian f32 lanes in a deterministic order. The in-memory layout of glam types varies with CPU features and versions, so the wire never carries glam's internal repr — only the lanes.

Lane order:
- **Vectors** (vec2, vec3, vec4): `x, y(, z, w)`
- **Quaternion**: `x, y, z, w`
- **Matrices** (column-major): each column in order, each column from first lane to last

This means saves are portable: a vec3 saved on one platform loads identical on another.

## What is the numeric tower?

The tower is the language's closed set of composite numeric types, implemented via the `glam` math library (proven, battle-tested, used in game engines). By using glam, the tower gets correct semantics for quat composition, matrix inverses, and vector math **by construction** — no hand-rolled arithmetic, no subtle bugs, no rewriting the C# oracle to match a botched implementation.

Every type in the tower is f32-backed (not f64) so that bevy marshals are identity — tower types and bevy types are the same types, not versions of the same concept. The scalar kit (min, max, clamp, lerp, etc.) operates uniformly across the tower's width: all functions that work on floats work on vectors and matrices too, componentwise. This uniform depth is why they are called a *tower* — every tool stacks.

Tower values are truthy (they are never empty), they are copyable (no allocation on lane access or component operations), and their equality is structural and IEEE — the same as floats. They simply cannot be ordered, because there is no meaningful total order on a vector.

## Reference

### Constructors

| Written as | Meaning |
|---|---|
| `vec2(x, y)` | A 2-lane f32 vector |
| `vec3(x, y, z)` | A 3-lane f32 vector |
| `vec4(x, y, z, w)` | A 4-lane f32 vector |
| `quat(x, y, z, w)` | A quaternion in (x, y, z, w) order |
| `mat2(col1, col2)` | A 2×2 column-major matrix |
| `mat3(col1, col2, col3)` | A 3×3 column-major matrix |
| `mat4(col1, col2, col3, col4)` | A 4×4 column-major matrix |

### Lane access

| For vectors | Read |
|---|---|
| `vec2` / `vec3` / `vec4` | `.x`, `.y`, `.z`, `.w` |

| For matrices | Read |
|---|---|
| `mat2` / `mat3` / `mat4` | `.x_axis`, `.y_axis`, `.z_axis` (mat3/4 only), `.w_axis` (mat4 only) |

### Operators

| Operation | Types | Result |
|---|---|---|
| `a + b` | same vec/quat | componentwise sum |
| `a - b` | same vec/quat | componentwise difference |
| `a * b` | same vec | componentwise product |
| `a * b` | quat × quat | rotation composition |
| `a * b` | mat × mat (same size) | matrix composition |
| `a * b` | quat × vec3 | vector rotated by quat |
| `a * b` | mat × vec (compatible) | vector transformed by matrix |
| `a * scalar` | vec/quat/mat | scaled by scalar |
| `scalar * a` | vec/quat/mat | scaled by scalar |
| `a / scalar` | vec | each lane divided by scalar |
| `-a` | vec/quat | negated |

### Scalar kit (componentwise on vectors, quaternions, matrices)

| Verb | Parameters | Result | Rows |
|---|---|---|---|
| `min` | `(T, T)` where T ∈ {vec2, vec3, vec4, quat, mat2, mat3, mat4} | T | pure·silent·total |
| `max` | `(T, T)` where T ∈ {vec2, vec3, vec4, quat, mat2, mat3, mat4} | T | pure·silent·total |
| `clamp` | `(T, T, T)` where T ∈ {vec2, vec3, vec4, quat, mat2, mat3, mat4} | T | pure·silent·total |
| `lerp` | `(T, T, float)` where T ∈ {vec2, vec3, vec4, quat, mat2, mat3, mat4} | T | pure·silent·total |

### Tower-specific verbs

| Verb | Parameters | Result | Rows |
|---|---|---|---|
| `dot` | `(vec2, vec2)` / `(vec3, vec3)` / `(vec4, vec4)` / `(quat, quat)` | float | pure·silent·total |
| `cross` | `(vec3, vec3)` | vec3 | pure·silent·total |

## Where this is ruled

- **Mini-spec:** `docs/tower-mini-spec.md` (T1–T5, glam backing, wire format, equality, all matrix sizes)
- **Stdlib:** `docs/stdlib-spec.md` §2b (the numeric tower as a closed, compiler-known domain; the scalar kit as the tower's width-1 floor)
- **Decision log:** 2026-07-19 (tower ruling delegation; F31, F32, F33 follow-up rulings; F4 partial-b matrix ops; the dev/prod NaN-ordering knob delegated to A4)
