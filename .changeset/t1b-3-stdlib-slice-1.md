---
"@brink-lang/web": patch
---

T1b-3 (#571): the brink dialect ships stdlib slice 1
(docs/t1b-surface-spec.md §5) — lowercase free functions, brink-dialect-gated
(`strict-ink` never sees them). Pure: `len(x)`, `keys(m)`, `values(m)`,
`contains(x, v)` (arrays: element containment; maps: key containment).
Mutating: `push(a, v)`, `insert(x, k_or_i, v)`, `remove(x, k_or_i)` — all
three require an lvalue first argument (a variable, temp, or indexed path)
and lower through the same take → `make_mut` → write-back RMW discipline
indexed assignment uses (§4); passing an rvalue (`push(#[1, 2], 3)`) is now
a targeted compile error (`E055`), and using a mutator's result — they
return nothing — is a compile error too (`E056`).

An author-defined function of the same name shadows the builtin, with a
warning (`E035`, reusing the existing "name shadows a built-in function"
code); imported vanilla ink that defines e.g. `len` keeps working under the
brink dialect. Under `strict-ink`, an unresolved call to any of the seven
names is now rejected the same way other brink-extension syntax is (`E051`).

VM-native: the array-generalized `MapInsert`/`MapRemove`/`MapContains`
opcodes (reserved+live since #575, now compiler-emitted) are the mutators'
primitives — despite the `Map*` names, they now also handle `Array`
containers (index-based insert-with-shift/remove-with-shift/element-scan),
since the frozen v4 collection-opcode block has no dedicated array-append
opcode and the RFC's one-bump rule reserved exactly this set. `push(a, v)`
desugars to `insert(a, len(a), v)`. No wire-format change — same opcode
bytes, generalized VM-side semantics.

Also fixes a latent gap this surface exposed: diagnostics produced during
LIR lowering (as opposed to the earlier analysis phase) were always treated
as warnings regardless of their own severity, so an Error-severity one could
never actually block compilation. `E055`/`E056` are the first Error-severity
diagnostics LIR lowering ever produces; the pipeline now partitions
lowering-phase diagnostics by severity like every other diagnostic source,
so they correctly fail the compile instead of silently compiling anyway.

Oracle corpus: unchanged, 5,577 passing episodes — the strict-ink corpus
never reaches any of this new surface by construction.
