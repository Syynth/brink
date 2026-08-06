---
"@brink-lang/web": patch
---

Strict mode's Unknown-escape (`E065`) / Conflicted-escape (`E066`)
checking now reaches inside a lambda literal's own body (#1770), the same
way it already does a top-level def's own params/temps. Before this fix,
`strict.rs` never looked inside an `Expr::Lambda` at all — an unannotated
or genuinely conflicting param/temp declared inside a lambda's own body
raised no diagnostic whatsoever, regardless of how nested the lambda was.

```brink
fn f(n: int): int {
  let g = |x: int|: int {
    let t;
    x
  };
  return n;
}
```

used to compile with zero diagnostics under `types = strict`; the
lambda's own unannotated, unused `let t` (genuinely `Unknown`) now
reports `E065`. An ascription on the same temp (`let t: string;`) still
exempts it, exactly like a top-level `~ temp`.

Recorded per-lambda (`InferPass::infer_lambda`, folded into a new
`BodyTypes::lambda_escapes` field), covering both params and body-declared
temps, for every lambda anywhere in a body including one nested inside
another lambda's own body. Deliberately excludes a lambda's own
return-type slot — issue #1994's `LambdaAnnotationMismatch` (`E174`)
already owns a materially different, eager check for a lambda's return
annotation disagreeing with its body. Strict-mode-only; `types = gradual`
is unaffected.

Widening this check over the existing native corpus surfaces new,
expected findings — every one an unconstrained lambda param that
`docs/typed-mode-spec.md` §2 already specifies as an `Unknown` escape
(call-site-driven inference is forbidden), the same category several
top-level params already fall into in `tests/tier1-native/`.
