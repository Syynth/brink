---
"@brink-lang/web": patch
---

M-2c: public cross-module resolution now requires an `IMPORT`
(docs/modules-spec.md §2), completing the M-2 module surface.

- **Import-required resolution (`E025`)** — a reference resolving to a
  *public* definition in another **declared** module which the referring
  file did not `IMPORT` is now `E025` with a did-you-mean-`IMPORT` message.
  Bringing the name in (bare `IMPORT { name } FROM mod`) or importing the
  module qualified (`IMPORT mod`) clears it. The restriction keys off the
  *target's* module being declared, so the permeable legacy world is
  untouched: a plain multi-file `INCLUDE` project with no `#@module` is one
  big default-public module and every cross-file bare reference keeps
  resolving byte-identically (§3). Only genuinely multi-*declared*-module
  projects are constrained; strict-ink and the existing single-module brink
  corpus resolve exactly as before.
- **`E091` qualified ambiguity** — a `IMPORT mod` (qualified) whose module
  name also names a definition visible bare in the same file makes `mod.y`
  ambiguous; flagged at the import (fixed with an alias).
- **`E092` redundant-override warning** — a `#@public`/`#@private` that
  merely restates its module's visibility default is now covered by
  end-to-end reachability tests.
