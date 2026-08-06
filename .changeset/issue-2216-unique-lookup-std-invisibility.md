---
"@brink-lang/web": patch
---

Issue #2216 (follow-up to #2197/#2080): `brink-analyzer`'s
`resolve::lookup_unique_by_name` — the scope-free UFCS-receiver lookup used
by `infer::body`, which has no `ImportScope` to consult — now excludes any
`std…`-mounted candidate the same way `lookup_by_name_direct` already
does for the scoped path, including when it is the function's sole
candidate. Without this, a name whose only candidate was declared in the
mounted `std/` tree would resolve through this path with no `use std::…`
import, disagreeing with `lookup_by_name`'s stdlib-invisibility rule
(#2080's SCOPE FENCE) and loosening `lookup_unique_by_name`'s own documented
"strict subset of `lookup_by_name`" guarantee.

**Reachable today, but no observable diagnostic delta found.** The prior
wording here claimed this was unreachable because the only caller "resolves
struct/UFCS-callable receivers" — false: `infer::body`'s
`infer_ufcs_free_fn_result` looks up the trailing method segment against
`&[SymbolKind::Knot, SymbolKind::External]` and records a call-graph edge
*before* it ever checks whether the receiver is a struct (that check only
gates the call's own *result type*, further down). `std/conventions/
screenplay.brink` ships exactly those kinds (`fn heading`/`transition`/
`cue`/`parenthetical`, `extern scene_entered`), so a project with no
same-named `fn`/`extern` of its own reaches this path on an ordinary
`x.heading(...)`-shaped call today. We tried to pin the resulting delta
(the spurious call-graph edge, or the differing inferred result type) as an
observable diagnostic through the real `brink_environment::compile` path
and could not: for a non-struct receiver, `brink_analyzer::ufcs`'s own
resolver (a separate, properly import-scoped pass this PR does not touch)
already declines the call outright before this function's answer matters;
for a struct receiver whose shape declares a matching field — the one
shape where `ufcs` settles the call via field access without needing this
function's answer at all — the spurious edge did not surface as an
`#@effects` exceedance in the cases we tried either. So the change is real
and reachable at the analyzer's internal-state level (confirmed by the two
new `resolve.rs` unit tests), but we did not find a corpus/oracle- or
diagnostic-observable case to pin end to end; see the PR body's
"Reachability" section for the fixtures tried. Included per this repo's
`@brink-lang/web` changeset convention for any patch touching resolution
behavior reachable through `brink_environment::compile`, regardless.
