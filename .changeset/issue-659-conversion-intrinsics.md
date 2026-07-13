---
"@brink-lang/web": patch
---

New pure conversion intrinsics `int(x)`, `float(x)`, `string(x)` under the
brink dialect (maintainer-ruled domains: permissive numerics + bool;
parse failure is a turn-terminating fault; float→int truncates toward
zero matching `INT()`). New compileable surface reachable through the
wasm compile entry points; out-of-domain arguments are `E078` compile
errors under `types = strict`.
