# T1b surface spec — dialect gate, logic blocks, collection syntax, stdlib slice 1

Status: design round 2026-07-11, rulings by maintainer. Companion to
`docs/tier1-roadmap.md` (T1b) and `docs/value-model-spec.md` (semantics —
ratified; this spec adds surface only). This document is also the
**strict-ink mode design note** the roadmap calls for.

## 1. The dialect gate

**Ruling: superset grammar, analyzer-gated, default strict-ink.**

- `brink-syntax` always parses the full brink grammar (extension
  constructs included). There is one CST/AST/HIR shape regardless of
  dialect — the IDE, formatter, and diagnostics never need a second
  grammar.
- The **dialect** is an analysis input (`AnalysisOptions`, fed from
  project config or CLI flag): `strict-ink` | `brink`.
- Under `strict-ink`, every extension construct lowers to a targeted
  diagnostic at its span: *"`#[…]` is a brink extension — this project
  compiles strict ink (`dialect = brink` to enable)"*. Parse never
  fails on extension syntax; analysis rejects it.
- **Default when nothing is declared: `strict-ink`.** Divergence from
  the oracle-anchored subset is a visible, one-time, per-project
  choice. The conformance harness and oracle corpus pin `strict-ink`
  explicitly, so the ratchet's meaning is enforced mechanically, not
  by convention.
- Per the #368-round precedent, the dialect declaration is an
  authoring-time/tooling artifact: it is not embedded in `.inkb` and
  never delivered to the runtime.

Under the salsa pipeline the dialect is an input to the analysis
queries; strict-mode checking of the oracle corpus shares every parse
and lowering prefix with normal compilation.

## 2. Multi-line logic blocks: `~ { … }`

**Ruling: pure logic only.** Blocks compute; weave flows.

```ink
~ {
    temp total = 0
    for item in #[1, 2, 3] {
        total = total + item
    }
    if total > 3 {
        score = total
    }
}
```

Grammar (superset; every construct below is brink-dialect-gated):

- A `~` logic line whose expression position begins `{` opens a
  **block**. Inside a block, statements are newline-terminated and do
  not repeat the `~` sigil.
- **Statements**: assignment (including indexed lvalues, §4),
  `temp` declaration, `if` / `else if` / `else` (braced),
  `while cond { … }`, `for name in expr { … }`,
  `break` / `continue`, `return` / `return expr`, expression
  statements (function and external calls).
- **Excluded inside blocks** (compile error, not parse error): text
  output of any kind, choices, gathers, diverts (`->`), tunnels,
  threads. `return` is the only flow construct.
- **Locals**: `temp` declared inside a block is **block-scoped** and
  may shadow an outer `temp` (warning diagnostic on shadowing).
  Plain-ink `temp` semantics outside blocks are unchanged.
- `for x in arr` iterates values; `for k in map` iterates keys in
  insertion order (deterministic per the ratified value model). No
  index/pair destructuring in T1b.
- **Bounded growth**: `while`/`for` bodies execute under the VM step
  limit like all bytecode; no new unbounded accumulation surface.

The seam rule generalizing the exclusions: **no weave concept may
appear in `Expr`/`Stmt`** (mirrors the LIR logic/narrative split
hygiene from phase 0). Loosening the fence later is additive;
tightening it would be a break.

## 3. Collection literals: `#[…]` and `#{…}`

**Ruling: sigil literals.**

- Array literal: `#[expr, expr, …]` (trailing comma allowed).
- Map literal: `#{key: expr, key: expr, …}` — key expressions restricted
  to the ratified key domain (int, string, bool) at runtime; the
  analyzer warns on statically-visible non-key types.
- Empty forms: `#[]`, `#{}`.
- **Legal in expression position only**: `~` lines, block statements,
  call arguments, condition expressions. **Not legal in prose
  position in T1b** — `#` opens a tag there, and tags may legally
  contain `{}` interpolation, so `#{…}` mid-prose is ambiguous with
  tag syntax. The pattern is: build in a `temp`, interpolate the temp.
  (Expression position has no such clash: `#` cannot begin an
  expression in ink, so the sigil is collision-free there. This is
  the honest scope of "collision-proof".)
- Nesting is unrestricted: `#[#{a: 1}, #{a: 2}]`.

Lowering: literals lower to LIR collection-construction ops and codegen
to the VERSION 4 literal pool + the reserved collection opcode block
(0xBE–0xC9, #557) — the first emission of T1a's inert surface.

## 4. Indexing and mutation

- Postfix indexing in expression position: `a[0]`, `m["k"]`,
  chained `grid[y][x]`. New grammar; no clash — postfix `[` follows a
  primary expression, which choice-label brackets never do.
- Indexed assignment: `a[0] = v`, `m["k"] = v`, chained
  `grid[y][x] = v` — statement position ( `~` line or block).
- Lowering follows the ratified RMW discipline exactly:
  take → `make_mut` → write-back on the root cell; chains lower to
  nested RMW, never to interior references (no projections in T1b —
  those are T1e).
- Out-of-bounds / missing-key reads and writes follow value-model-spec
  §11c error semantics (turn-terminating runtime fault; no silent
  growth on write-past-end).

## 5. Stdlib slice 1

**Ruling: lowercase free functions, author-shadowing with warning.**

Pure: `len(x)`, `keys(m)`, `values(m)`, `contains(x, v)` (arrays:
element; maps: key), `char_at(s, i)` (issue #857 — see below). Mutating:
`push(a, v)`, `insert(x, k_or_i, v)`, `remove(m, k)`.

**`remove` is map-only as of issue #1484** (decision log "Quick-docket
closures" 2026-07-26): the array-index leg this signature originally
documented moved to its own verb, `remove_at(a, i)`, joining the `_at`
faulting-index family with `char_at` — `remove` now uniformly names
identity-based, idempotent-total removal (map keys; flags values once
flags land), never OOB-faulting. See `docs/stdlib-spec.md` §4/§10.

- Names live in the brink dialect only; strict-ink projects never see
  them (no collision surface for vanilla ink).
- An author-defined function with the same name **shadows** the
  builtin, with a warning diagnostic. (Imported vanilla ink that
  defines `len` keeps working under the brink dialect.)
- **Mutators require an lvalue** first argument (a variable, temp, or
  indexed path); passing an rvalue is a compile error ("`push` mutates
  its first argument — bind it to a variable first"). They lower
  through the same RMW discipline as §4 and return nothing.
- Pure functions accept any expression.
- Implementation is VM-native (opcode or native-call table), not ink
  functions — they must work on the value core directly.

**`char_at(s, i)`** (issue #857, corpus finding — string indexing was
missing, blocking string-algorithm ports like levenshtein/tokenizers):
`i` indexes Unicode scalar values ("chars"), not UTF-8 bytes — a
byte-indexed read would panic or split a multi-byte sequence for any
non-ASCII text, which is exactly the silent-garbage outcome §11c
forbids. Returns the char at `i` as a single-character `String` (ink
has no separate char type). `i` outside `[0, char_count)`, a non-`Int`
`i`, or a non-`String` `s` is a turn-terminating runtime fault (§11c's
"no silent garbage" default — not a clamp, not a silently-empty
result), matching indexing's own out-of-bounds fault posture (§4).
Typing rule (declared at introduction, per the facility doctrine):
fixed `Ty::String` return, independent of the argument types — the
domain check is a runtime/gradual-mode concern at the `CharAt` op,
same posture as `int`/`float`/`string` (§4's "everything else
explicit" completion, `docs/typed-mode-spec.md` §4).

## 6. Testing (no oracle exists for this surface)

Per the roadmap's divergence discipline:

- **Strict-mode gate**: the entire oracle corpus compiles under
  `strict-ink` in CI; ratchet unchanged at `RATCHET_EPISODE_COUNT`
  byte-identical. Extension syntax appearing anywhere in the corpus is
  a hard failure.
- **Tier-1 corpus wing**: `tests/tier1-brink/` — brink-dialect cases
  with hand-written expected transcripts (spec-derived, not
  oracle-derived), exercised by the existing episode harness.
- **Property tests**: COW/aliasing invariants under randomized
  block programs (sharing-unobservable law); map iteration-order
  determinism; RMW chain equivalence (`grid[y][x] = v` ≡ manual
  take/mutate/write-back).
- Grammar fuzzing: the superset parser must never panic on any input
  in either dialect (extends the existing brink-syntax fuzz targets).

## 7. Explicitly out of T1b

Prose-position literals; method-call syntax (`a.push(v)` — collides
with dotted paths); destructuring; slices/ranges; closures (T1c);
handles (T1d); projections (T1e); structs (typed dialect); any type
syntax (rides `#@` annotations, post-T1b); effects (T2, own round).

## 8. Build sequencing (spine — single reviewed agents, oracle-gated)

1. **T1b-1 grammar + HIR**: superset parse of blocks/literals/indexing
   + dialect gate diagnostics (no LIR/codegen — everything still
   rejects at lowering). Oracle + strict-corpus gates.
2. **T1b-2 LIR + codegen + VM**: collection construction/indexing
   ops, block/loop lowering, V4 literal-pool emission; first live
   opcodes. Tier-1 corpus wing lands here.
3. **T1b-3 stdlib slice 1** + RMW mutator lowering.
4. **T1b-4 mechanical tail** (pump wave): corpus growth, docs/book
   chapter, IDE polish (completion for stdlib, block folding).
