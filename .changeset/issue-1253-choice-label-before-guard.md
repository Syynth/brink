---
"@brink-lang/web": patch
---

#1253: `brink-syntax-native`'s choice-line parser (`* (label) {if cond}
text`) checked for a `{if cond}` guard before a `(label)`, the reverse of
ink's own canonical order. The reference C# parser's `Choice()`
(`InkParser_Choices.cs`) parses `BracketedName` (label) strictly before
`ChoiceCondition` (guard), and `brink-syntax`'s reference grammar agrees —
so a writer copying ink's idiomatic `* (name) {if cond} text` spelling got
a parse error on valid-looking source. The check order is now
label-then-guard, matching ink. There is no reference support for the
reverse order (guard-then-label) either; that shape still parses (the
guard is recognized) but a paren following it is not read as a `LABEL` —
it falls through to ordinary choice text, the same "unrecognized shape is
prose" tradeoff already applied to content-line labels.

`brink-syntax-native` feeds `brink-ir`, `brink-analyzer`, `brink-db`, and
`brink-ide`, all depended on by `brink-web` (`@brink-lang/web`), so any
`.ink` source parsed through the native surface's editor/IDE tooling with
this choice-line shape is affected.
