---
"@brink-lang/web": patch
---

#1487/#1488/#1489 (NG-A/NG-B/NG-C, RULED 2026-07-26 — `docs/decision-log.md`
"NG-C ruled: `: type` returns everywhere"): the native `.brink` surface
gains its type-annotation grammar, in **one spelling for every position**.

`brink-syntax-native` grows a `type_expr` production (`TYPE_ANNOTATION` /
`TYPE_EXPR` / `TYPE_NAME` / `TYPE_GENERIC` / `TYPE_FN`, structurally the
brink dialect's own TM-2 shape) and wires `(: type)?` into parameters
(`fn probability(g: Guest)` — shared by `fn`/`flow`/`extern`), bindings
(`let x: int = 1;`, `var hp: int = 10`, `const MAX: int = 100`), the
`fn`/`flow` return clause after the parameter list
(`fn probability(g: Guest): float { … }`), and lambdas
(`|g: Guest|: bool { … }`, grammar-only — lambda lowering is still fenced).

`brink-ir::hir::lower_native` populates the HIR slots that already existed
for the ink dialect: `Param.annotation`, `TempDecl`/`VarDecl`/`ConstDecl`
`.annotation`, and `Knot.return_type`. Because these are the *same*
`hir::TypeExpr` values, `brink-analyzer`'s strict-mode annotation firewall
now reaches native source with no analyzer change: an annotated parameter
or binding is exempt from `E065` Unknown-escape, which was previously
unreachable from a `.brink` file (native is strict-only, #1342). Declaring
a return type is also the ruled coroutine-vs-state toggle, so a
value-returning `flow` no longer picks up the implicit `-> DONE` on
fall-through.

Web-observable: a `.brink` entry compiled through `compile_project` /
`compile_fragment` reaches the native pipeline (`brink_environment::compile`
dispatches on the entry's extension via `brink_driver::is_native`), so a
source carrying any of these annotations now parses and compiles where it
previously failed with a parse error.
