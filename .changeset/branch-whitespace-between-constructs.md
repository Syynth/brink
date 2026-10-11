---
"@brink-lang/web": patch
---

Whitespace around an interpolation or other inline construct inside a sequence or conditional branch is now kept, as in ink: `{ {a} {b} | c }` prints `A B`, not `AB`, and `{ {a} | c}.` prints `A .` (#2982). Leading whitespace at the start of a branch is unchanged.
