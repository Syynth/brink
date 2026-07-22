---
"@brink-lang/web": patch
---

F31 partial-b (#1145): three more tower operator rows land on the frozen
arithmetic operators — `mat * mat` (composition, matching sizes),
`mat * scalar` (scale, int operands promote, one direction only), and
`vec / scalar` (scale down, int operands promote, one direction only; IEEE
float division, so a zero divisor yields `inf`/`nan` lanes rather than a
fault, per `docs/tower-mini-spec.md` T4). Every other currently-faulting
glam-native form — `mat ± mat`, `quat * scalar`, `vec / vec`,
`scalar / vec`, `scalar * mat`, … — is unchanged and still faults. Brink-
dialect only; the oracle corpus is unaffected by construction.
