# CST Structure Review: Issues for AST Ergonomics

**Date:** 2026-02-28

Review of the current CST node structure in `brink-syntax` to identify oddities, inconsistencies, and design issues that would make building a robust and ergonomic AST API difficult.

---

## 1. Text node proliferation

**Issue:** 5 distinct text node kinds (`TEXT_CONTENT`, `CHOICE_TEXT`, `BRANCH_TEXT`, `MULTILINE_BRANCH_TEXT`, plus implicit text inside `TAG`) and 4 escape node kinds (`CONTENT_ESCAPE`, `CHOICE_ESCAPE`, `BRANCH_ESCAPE`, `ML_BRANCH_ESCAPE`) that are all structurally identical — they just have different stop-character sets.

| Node Kind | Used in | Escape companion |
|-----------|---------|-----------------|
| `TEXT_CONTENT` | `MIXED_CONTENT` (content lines, gathers) | `CONTENT_ESCAPE` |
| `CHOICE_TEXT` | `CHOICE_START/BRACKET/INNER_CONTENT` | `CHOICE_ESCAPE` |
| `BRANCH_TEXT` | `BRANCH_CONTENT` (inline branches) | `BRANCH_ESCAPE` |
| `MULTILINE_BRANCH_TEXT` | `MULTILINE_BRANCH_BODY` | `ML_BRANCH_ESCAPE` |

**Decision: Change CST.** Collapse all text node kinds into a single `TEXT` and all escape node kinds into a single `ESCAPE`. The parent node already conveys the context — a `TEXT` inside `BRANCH_CONTENT` vs `MIXED_CONTENT` is unambiguous. The separate kinds are redundant with tree position and add complexity to both the CST and AST without carrying additional information.

## 2. Inconsistent IDENTIFIER wrapping

**Issue:** Three different patterns for "a name" in the CST:

1. **`IDENTIFIER > IDENT`** — declarations wrap the token in a node (knot headers, var/const/temp decls, function calls, external decls, param lists)
2. **`PATH > IDENT`** — expressions and assignments use `divert::path()`, which always wraps in `PATH` even for single-segment names
3. **Bare `IDENT`** — `CHOICE_LABEL` puts `L_PAREN IDENT R_PAREN` with no wrapping node around the name

**Decision: Minimal CST fix.** Wrap the bare `IDENT` in choice/gather labels with `IDENTIFIER`. The `IDENTIFIER` vs `PATH` split is meaningful — `IDENTIFIER` is a defining occurrence (declaration), `PATH` is a use-site that might be dotted. The only real inconsistency is the bare `IDENT` in labels.

## 3. `DIVERT_NODE` wraps everything, creating double nesting

**Issue:** Every divert construct gets wrapped in `DIVERT_NODE`, but simple diverts (`-> target`) have no inner wrapper node — just raw tokens. Other forms (`THREAD_START`, `TUNNEL_ONWARDS_NODE`, `TUNNEL_CALL_NODE`) do have inner wrapper nodes. Dispatching on the divert variant requires different strategies depending on whether an inner node exists.

```
DIVERT_NODE
  THREAD_START        (for <-)        — has inner node
  TUNNEL_ONWARDS_NODE (for ->->)      — has inner node
  TUNNEL_CALL_NODE    (for -> x ->)   — has inner node
  plain tokens        (for -> target) — NO inner node
```

**Decision: Change CST.** Add a `SIMPLE_DIVERT` (or `DIVERT_CHAIN`) wrapper node for plain `-> target` cases. Keep `DIVERT_NODE` as the outer wrapper — it's useful for "is there a divert here?" queries. The fix makes the inner structure uniform: every `DIVERT_NODE` has exactly one meaningful child node.

## 4. `TUNNEL_CALL_NODE` is a post-hoc wrapper

**Issue:** `TUNNEL_CALL_NODE` is created retroactively via `start_node_at(checkpoint)` when the parser discovers a trailing `->` after targets. It wraps tokens that are already children of `DIVERT_NODE`, acting as a grouping layer injected after the fact.

**Decision: Keep as-is.** With the `SIMPLE_DIVERT` fix from #3, the structure is clean. `TUNNEL_CALL_NODE` groups the divert chain meaningfully — it says "this entire sequence is a tunnel call." The checkpoint mechanics are invisible once the tree is built. The AST treats it as another variant under `DIVERT_NODE`.

## 5. Conditional node hierarchy is complex and polymorphic

**Issue:** 6 conditional-related node kinds plus 4 sequence-related node kinds representing different syntactic forms:

```
CONDITIONAL_WITH_EXPR     — expr : body
  INLINE_BRANCHES_COND    — true_content | false_content
  MULTILINE_BRANCHES_COND — - branch\n - branch\n
  BRANCHLESS_COND_BODY    — content without branch markers
    ELSE_BRANCH           — wraps a MULTILINE_BRANCH_COND

MULTILINE_CONDITIONAL     — { NEWLINE - branch\n - branch\n }

SEQUENCE_WITH_ANNOTATION  — annotation + branches
IMPLICIT_SEQUENCE         — {a|b|c}
  INLINE_BRANCHES_SEQ     — a|b|c
  MULTILINE_BRANCHES_SEQ  — - a\n - b\n
```

**Decision: Keep as-is, normalize in AST.** These are genuinely different syntactic forms. Inline `{x: a | b}` and multiline `{x:\n - a\n - b\n}` use different delimiters. `BRANCHLESS_COND_BODY` is real Ink sugar. An LSP formatter needs to know which form the author wrote. The AST is the right place to unify into `Conditional { condition, branches }`.

## 6. `LOGIC_LINE` vs `BLOCK_LOGIC_LINE`

**Issue:** Identical internal structure (both parse `~ statement`), differing only in whether a trailing NEWLINE is consumed. `BLOCK_LOGIC_LINE` exists because multiline branch bodies manage newlines at the parent level.

**Decision: Change CST.** Merge into a single `LOGIC_LINE` that optionally consumes a trailing newline if present. The trailing newline isn't semantically part of the logic line — it's a line terminator. The parser function `block_logic_line()` can be deleted and `logic_line()` used everywhere. Losslessness is preserved either way.

## 7. Content containers aren't uniform

**Issue:** Content elements appear inside 7 different container node kinds, each with its own parsing function and different rules about what children are allowed:

| Container | Diverts? | Logic lines? | Brackets? |
|-----------|----------|-------------|-----------|
| `MIXED_CONTENT` | No (sibling) | No | No |
| `CHOICE_START_CONTENT` | No | No | Stops at `[` |
| `CHOICE_BRACKET_CONTENT` | No | No | Yes (`[`...`]`) |
| `CHOICE_INNER_CONTENT` | No | No | No |
| `BRANCH_CONTENT` | Yes | No | No |
| `MULTILINE_BRANCH_BODY` | Yes | Yes (`~`) | No |
| `BRANCHLESS_COND_BODY` | Yes | Yes (`~`) | No |

**Decision: Keep as-is, normalize in AST.** Unlike the text nodes (#1), these containers genuinely differ in what children they accept. Collapsing them would mean the CST no longer encodes which elements are valid where. The choice content kinds represent before-bracket / bracket / after-bracket regions, which is meaningful structure.

## 8. `CHOICE_LABEL` name is misleading for gathers

**Issue:** `choice_label()` is used by both `CHOICE` and `GATHER`, but always emits `CHOICE_LABEL`. A gather with a label produces `GATHER > CHOICE_LABEL`, which is inaccurate.

**Decision: Change CST.** Rename `CHOICE_LABEL` to `LABEL`. It's the same syntax (`(ident)`), same structure, same semantics. The name should be accurate regardless of parent context.

## 9. `PATH` is used for multiple distinct concepts

**Issue:** `divert::path()` produces `PATH` nodes for divert targets (`-> knot.stitch`), value references in expressions (`x`, `list.member`), and assignment LHS (`~ x = 5`). All produce identical `PATH > IDENT (DOT IDENT)*` trees.

**Decision: Keep as-is, distinguish in AST.** The syntax is identical in all three cases — a dotted identifier. The CST's job is to represent the syntax, and `PATH` does that accurately. Whether it's an lvalue, rvalue, or divert target is a semantic distinction that belongs in the AST or a later analysis pass.

## 10. No error-boundary node for partial content

**Issue:** `error_recover()` wraps a single token in `ERROR`. There's no broader error-boundary mechanism for partially-parsed constructs.

**Decision: Keep as-is.** This is standard for rowan-based CSTs (rust-analyzer works the same way). The AST needs `Option`-returning accessors anyway for LSP robustness. Better error recovery can be layered in later without changing the AST API surface.

## 11. `EMPTY_LINE` may create noise

**Issue:** Every blank line becomes an `EMPTY_LINE` node. AST traversals must filter these constantly.

**Decision: Keep as-is.** A formatter or LSP needs blank line information. Absorbing them as trivia makes them harder to reason about. The AST skips them, same as it skips trivia and errors.

---

## Summary of CST changes

Changes to make:
1. **Collapse text nodes** — `TEXT_CONTENT`, `CHOICE_TEXT`, `BRANCH_TEXT`, `MULTILINE_BRANCH_TEXT` → single `TEXT`
2. **Collapse escape nodes** — `CONTENT_ESCAPE`, `CHOICE_ESCAPE`, `BRANCH_ESCAPE`, `ML_BRANCH_ESCAPE` → single `ESCAPE`
3. **Wrap label ident** — bare `IDENT` in choice/gather labels → `IDENTIFIER > IDENT`
4. **Add `SIMPLE_DIVERT`** — plain `-> target` gets an inner wrapper node for consistency with other divert forms
5. **Merge logic line kinds** — `BLOCK_LOGIC_LINE` → `LOGIC_LINE` (optionally consume trailing newline)
6. **Rename `CHOICE_LABEL`** → `LABEL`

Things to handle in AST only (CST stays as-is):
- Conditional/sequence hierarchy normalization
- Content container normalization
- `PATH` semantic disambiguation (divert target vs value ref vs lvalue)
- Error node tolerance
- Empty line filtering
- `TUNNEL_CALL_NODE` flattening into divert enum variant
