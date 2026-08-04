---
"@brink-lang/web": patch
---

Issue #2216 (follow-up to #2197/#2080): `brink-analyzer`'s
`resolve::lookup_unique_by_name` — the scope-free UFCS-receiver lookup used
by `infer::body`, which has no `ImportScope` to consult — now excludes any
`story::std…`-mounted candidate the same way `lookup_by_name_direct` already
does for the scoped path, including when it is the function's sole
candidate. Without this, a name whose only candidate was declared in the
mounted `std/` tree would resolve through this path with no `use std::…`
import, disagreeing with `lookup_by_name`'s stdlib-invisibility rule
(#2080's SCOPE FENCE) and loosening `lookup_unique_by_name`'s own documented
"strict subset of `lookup_by_name`" guarantee.

**Not reachable today**, and this patch does not change any corpus or
`@brink-lang/web` output: this function's only caller resolves
struct/UFCS-callable receivers, and the current `std/` mount ships neither.
Filed proactively (as #2216 asked) so a future std module that does add one
doesn't silently reopen the bare-name visibility gap #2197 closed elsewhere.
Included per this repo's `@brink-lang/web` changeset convention for any
patch touching resolution behavior reachable through
`brink_environment::compile`, even when — as here — no fixture currently
exercises the changed branch.
