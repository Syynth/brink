---
"@brink-lang/web": patch
---

Issue #2837: `lower_call`'s resolved-target match now refuses a non-callable symbol kind with a new diagnostic, **`E183`**, instead of silently emitting a call against it. This is reachable from real author source (a `temp`/param called before its own declaration — a genuine forward reference) as well as from a defensive-backstop shape (`ListItem`/`Label`/`Stitch`/`Struct` at a call position), and is web-observable: the wasm editor's Problems panel now reports `E183` for the forward-reference shape on both analysis roads. Calling a T1b block-scoped temp (`~ { … }`) after its own block has closed continues to report `E082`, not `E183`, matching `lower_path`'s existing guard for the identical mistake.
