# `fn conventions()` + comptime evaluation — sizing memo and blocking questions

**Status: AGENT-AUTHORED SIZING — not a ruling, and not an implementation.**
Written 2026-07-31 against issue #1840 ("Conventions v1c"), the 2026-07-31
decision-log entry *"Conventions are annotated handlers: the declarative
element surface is subsumed by the annotation surface (§9.1 settled)"*, and
the code at `ca7d3a082` (merge of PR #1845). Every coordinate below was read
at that ref.

Issue #1840 instructs, verbatim: *"Before writing code, answer and record:
1. **Which language subset** is available at comptime? 2. **What happens on
a comptime fault** …? 3. **How do comptime errors map back to source?**"*
— and: *"If those turn out to need a maintainer ruling rather than an
implementation choice, **DECLINE with the analysis naming the exact
question**."*

This memo is that record. **Verdict: decline.** Three of the decisions this
slice needs are language-semantics or format rulings, not implementation
choices, and one of them (Q3) is blocked on an open `needs-design` epic that
has been blocked on a ruling since 2026-07-19. Four further prerequisites
are simply absent from the tree and are not filed anywhere.

---

## 1. What the ruling asks for

```brink
use std::conventions::screenplay

@[effects(pure)]
fn conventions() {
    register(screenplay::scene)
    register(sfx)
    register(screenplay::cue)
    register(screenplay::action)
}
```

Statement order is resolution order. The compiler evaluates this at build
time, freezes the ordered result, and the editor reads the frozen
projection without ever evaluating anything (§3.5's "the data form survives
only as generated interchange").

## 2. What exists today

Genuinely in place, and more than the issue's framing assumes:

| Piece | Status | Coordinate |
|---|---|---|
| `#fn(…)` **value model** | present | `Value::FnRef(DefinitionId)` (`VAL_FN_REF`), `crates/internal/brink-format/src/value.rs:166` |
| `#fn(…)` HIR → LIR → opcode | present | `hir::Expr::FnLiteral` (`crates/internal/brink-ir/src/hir/types.rs:1190`) → `lower_fn_literal` → `PushFnRef`/`MakeClosure` (`crates/internal/brink-ir/src/lir/lower/expr.rs:131`) |
| Effect row on `Ty::Fn` (#1680) | present | `EffectRow`, `crates/internal/brink-analyzer/src/infer/effects.rs:80` |
| `@[effects(pure)]` assertion + exceedance check | present | `crates/internal/brink-analyzer/src/effects_assertions.rs` (`E103` at `:256`) |
| Function evaluation driver | present, **runtime-side** | `FlowInstance::begin_function_eval`, `crates/brink-runtime/src/story/flow_instance.rs:872` |
| Natural-notation claim dispatch (#1838) | present, **file-local** | `crates/internal/brink-ir/src/hir/lower_native/element.rs` |
| Per-line-table-entry source location | present | `brink_format::SourceLocation { file, range_start, range_end }` on `LineEntry::source_location`, `crates/internal/brink-format/src/definition.rs:73,87` |

Absent:

- **No comptime machinery of any kind.** `grep -rn comptime crates/**/*.rs`
  matches exactly one line, and it is a doc comment saying this slice is not
  built: `crates/internal/brink-ir/src/hir/lower_native/element.rs:58`.
- **No `fn conventions()` anywhere.** Same single doc-comment match.

## 3. The three questions

### Q1 — Which language subset is available at comptime?

The tempting answer, *"whatever `begin_function_eval` accepts"*, is not a
subset anyone has specified, and it does not survive contact with the one
thing this slice actually has to move across the boundary.

`begin_function_eval` (`flow_instance.rs:872`) takes a `&Program`, a
`&dyn ExternalFnHandler`, a `container_idx` and `&[Value]`, and returns a
`Value`. Content emission during the call is legal, not refused: its own
doc says "output is captured and discarded" (`flow_instance.rs:845-846`),
and its `# Errors` block is explicit that `RuntimeError::FunctionYielded`
(`crates/brink-runtime/src/error.rs:82`) fires only "if the function
presents choices or ends the story" (`flow_instance.rs:864-865`) — the two
raise sites confirm it: `Stepped::Done | Stepped::Ended`
(`flow_instance.rs:1098-1101`) and a pending choice above the call's
`choice_floor` (`flow_instance.rs:1132-1134`). So the comptime frame is "a
compiled `fn` body whose content emission is captured and discarded;
presenting choices or reaching `-> DONE` / `-> END` is refused."

The unresolved part is **what the registry is made of**. `register` receives
`Value::FnRef(DefinitionId)` — a fn token in the *conventions module's own
compiled program*. But the consumer is HIR lowering of a *different* file,
and what it needs is a `ClaimHandler`: the compiled `claims` pattern, the
parameter-name list, and the annotation's own `TextRange`
(`element.rs:79-93`), all derived from the conventions module's **CST**, not
from its bytecode. A `DefinitionId` is not that, and the mapping back is a
choice with a wire consequence, because the same identity has to key the
frozen editor projection.

> **Q1 (ruling needed): what identity does a registered handler carry across
> the comptime boundary and into the frozen projection — the `DefinitionId`
> fn token, or the module-qualified source name?** The token is stable
> across body edits by construction (`value.rs:166`'s doc: the fn token
> *"hashes from the target's name, so a saved token survives recompiles that
> only edit the body"*). It is not opaque in the sense of unreachable:
> `DefinitionId` is a deterministic 56-bit hash of the qualified name
> (`hash_qualified_name`, `crates/internal/brink-analyzer/src/manifest.rs:512`;
> layout at `crates/internal/brink-format/src/id.rs:52`), and anything that
> can see the name and reach the analyzer — including the wasm editor — can
> compute name → token the same way the compiler does. The real asymmetry:
> the hash is not invertible back to a display name, and it carries none of
> the `ClaimHandler` payload the editor actually needs — the compiled
> `claims` pattern, parameter-name list, or the annotation's `TextRange`
> (`element.rs:79-93`). The name is what the editor can display, and #1838
> already resolves it within a file, but adopting it here reintroduces
> cross-file name resolution that #1838 explicitly deferred. This decision
> fixes the projection's schema, so it cannot be made after the fact.

### Q2 — What happens on a comptime fault?

Mechanically, today: the function-eval loop is `drive_function_eval`
(`flow_instance.rs:1071`). It counts steps at `:1081` and fails at `:1082-84`
with `RuntimeError::StepLimitExceeded(Self::STEP_LIMIT)`, where
`STEP_LIMIT: u64 = 1_000_000` (`flow_instance.rs:194`) is **hardcoded** —
unlike the line-stepping path, which takes a caller-supplied `step_limit`
parameter (`flow_instance.rs:387`). So a comptime evaluation currently
cannot be given its own budget without a signature change, and
`RuntimeError::StepLimitExceeded(u64)` (`crates/brink-runtime/src/error.rs:65`)
carries only the limit — no definition, no offset, no range.

Two sub-decisions here are ordinary implementation choices with obvious
defaults (a distinct comptime step budget; a cap on the registration list,
per CLAUDE.md's "any loop that accumulates data must have a limit"). One is
not:

> **Q2 (ruling needed): is a comptime fault a compile *error* that fails the
> build, or a diagnostic that degrades to an empty/partial convention set?**
> This is not cosmetic. Conventions decide how prose lines classify, so an
> empty registry silently reclassifies every claimed line back to plain
> content — the exact "silent data drop" CLAUDE.md forbids — while a hard
> failure means one bad helper in a preset module bricks the whole project's
> build, including in the editor's live re-evaluation loop that §3.5 owes.

### Q3 — How do comptime errors map back to source? **Blocked.**

There is **no bytecode → source mapping at instruction granularity**.
`Opcode::SourceLocation(u32, u32)` exists in the format
(`crates/internal/brink-format/src/opcode.rs:1659`) but is dormant: the VM
discards it in the same arm as `Nop` (`crates/brink-runtime/src/vm.rs:198`).

The `brink_format::SourceLocation` struct (`definition.rs:73`) is real and
*is* populated by codegen — but it is **per line-table entry**
(`LineEntry::source_location`, `definition.rs:87`; `LineEntry` at `:81`),
not per-definition: it is built per *content* node
(`build_source_location`, `crates/internal/brink-ir/src/lir/lower/recognize.rs:252`),
and `brink-codegen-inkb` populates it at every line-emission site
(`content.rs:11,61,112`) — the `lib.rs:349,371` mentions are call sites of
that same struct, not something unrelated. `ContainerDef`
(`definition.rs:11-52`) — the actual per-*definition* record — carries
**no source range field at all**, only `id`, `scope_id`, a `NameId`, and
`bytecode`.

That makes the available mapping narrower than it first looks, not wider:
a `register(...)` call inside `fn conventions()`'s code-ground body is a
call, not a content line, so it never gets a `LineEntry::source_location`,
and its enclosing `ContainerDef` has no range to fall back to — only its
own `NameId`. A comptime fault today cannot be attributed to "somewhere
inside `fn conventions()`" with any range at all, only to the container's
bare name. Building the real thing is epic **#452**
(`instruction-level source mapping`), which is **OPEN** and labelled
`needs-design` (verified via `gh issue view 452`), and whose pivotal
decision is recorded as blocked on a maintainer ruling —
`docs/sourcemap-epic-evaluation.md` §1 lists workstream 3 (the
`brink-format` carrier) as **"blocked-on-ruling"**, question Q-R1.

> **Q3 (ruling needed, and already open as #452's Q-R1): comptime errors
> cannot point at a statement, or even a range, until the format carries an
> instruction → range map — today's per-line-table `SourceLocation` never
> reaches a code-ground call in the first place.** Either #1840 accepts
> definition-*name*-granularity error reporting (no range, just the
> container's `NameId`) as its v1 contract — a decision, because it is the
> diagnostic quality authors will live with — or #1840 is downstream of an
> epic that has been waiting on a format ruling since 2026-07-19.

## 4. The fence contradicts its own example

Independent of the three questions, and the sharpest finding here.

The ruling says the capability fence is the effect row: `fn conventions()`
is `@[effects(pure)]`, and *"a violation is an ordinary effect diagnostic"*,
explicitly **not** a bespoke checker. `@[effects(pure)]` asserts the **empty
state row** (`docs/effects-spec.md` §10: *"`@[effects(pure)]` asserts the
empty state row (the tooling-trust case)"*), and exceedance is `E103`.

`EffectRow` (`infer/effects.rs:80`) has exactly these state components:
`reads` and `writes` over VAR/CONST global `DefinitionId`s, and `calls` over
**`EXTERNAL` binding names**.

`register(…)` has to *do* something: append to an ordered build-time
registry. Under the machinery the ruling names, there are only two ways to
spell that, and both fail:

1. **`register` is an `EXTERNAL`.** This is the natural fit —
   `begin_function_eval` takes a `&dyn ExternalFnHandler` precisely so the
   host can service calls out of the evaluated program, which is exactly how
   the compiler would collect registrations. But an external call lands in
   `EffectRow.calls`, the row is then non-empty, and **the ruling's own
   canonical example fails its own `@[effects(pure)]` assertion with
   `E103`.**
2. **`register` is a row-exempt compiler intrinsic.** This makes the example
   pass, but it is a bespoke exemption — the thing the ruling rejected one
   sentence earlier — and it cuts directly against the house precedent. The
   RNG cell was the same shape of problem and was decided the *other* way:
   `docs/effects-spec.md` §10 rules that every draw is *"an ordinary
   **write**"* to a named cell, with the explicit consequence that
   *"`@[effects(pure)]` asserts rng-freedom (E103 names `rng`)"*. Applying
   that precedent, the registry is a cell, `register` writes it, and `pure`
   denies it.

> **Q4 (ruling needed): what is `register`'s effect row, such that
> `@[effects(pure)] fn conventions()` — the ruled spelling — passes?**
> Candidate answers: a `comptime` row dimension that `pure` permits; a named
> registry cell plus a redefinition of what `pure` asserts inside a comptime
> frame; or a declared `writes(conventions)` on the ruled example instead of
> `pure`. All three change what `pure` means or what the row is, which is
> `docs/effects-spec.md`'s territory, not this issue's.

## 5. Prerequisites that are absent and unfiled

Even with all four questions answered, none of these exists and none is
tracked:

1. ~~**`#fn(…)` has no native spelling.**~~ **RESOLVED** (issue #1862,
   RULED 2026-08-01 — see `docs/t1c-spec.md` §2a). The ruling went the
   other way from what this item assumed: native gets **no `#fn` at
   all**, precisely because `#` is already the tag sigil in native content
   position (`crates/internal/brink-syntax-native/src/parser/block.rs`).
   Instead, a statically-named function in expression position simply *is*
   a fn value — `register(screenplay::scene)`, no sigil — with a call
   still spelled `screenplay::scene()`. So this was a lowering addition,
   not a grammar addition: `brink-syntax-native` needed no new token and
   no new node. The **binding** form (`#fn(f, a)`, a bound prefix) still
   has no native spelling and stays ink-only; §1's example above is
   already updated to the ruled bare-name spelling.
2. **`std::conventions::screenplay` does not exist.** There are no stdlib
   `.brink` module files in the tree at all — the `.brink` files that do
   exist are either test fixtures under `tests/` (top-level, e.g.
   `tests/tier1-native/`, `tests/tier1-brink-respell/`, and crate-local,
   `crates/internal/brink-syntax-native/tests/corpus/`) or fuzzer seed
   corpus under `crates/internal/brink-syntax-native/fuzz/seeds/brink/` —
   none of them is a stdlib module.
3. **`brink.toml` has no conventions pointer.** `ProjectConfig`
   (`crates/internal/brink-project-config/src/lib.rs:192`) carries
   `dialect`, `types`, `lints`, `deny_warnings`, `unprune_dirs` — no
   `conventions`/`elements` key. §3.5 already listed this as "design pass
   owed".
4. **Element dispatch has no project-level injection point.**
   `element::collect(file_id, root)`
   (`crates/internal/brink-ir/src/hir/lower_native/element.rs:128`) builds
   the handler table by walking *the file's own* CST children. There is no
   parameter through which an externally-evaluated registry could arrive,
   and `element.rs:59-61` records why: confinement to the
   `brink.toml`-named module "needs project identity that single-file
   lowering does not have".

Additionally, `brink-compiler` depends on `brink-runtime` only as a
**dev-dependency** (`crates/brink-compiler/Cargo.toml`). Comptime evaluation
promotes that to a real dependency — the compiler linking and running the
VM to compile. Not cyclic, and not obviously wrong, but it is a stated
architectural direction that deserves to be chosen rather than arrived at.

## 6. Recommendation

Answer Q1–Q4 in one sitting; they are entangled (Q1 fixes the projection
schema, Q4 fixes what the module may contain, Q2 fixes what the editor does
with a bad module, and Q3 fixes the diagnostic floor). Then file the §5
prerequisites as their own slices — items 1 and 4 in particular are
issue-sized on their own, and item 1 is a shared-grammar change that wants
its own review.

Nothing in §2 is wasted: the `#fn` value model, the effect row, and #1838's
claim dispatch are all real, and they are the reason this slice is *close*
rather than speculative. It is blocked on decisions, not on capability.
