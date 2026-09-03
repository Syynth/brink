---
"@brink-lang/web": patch
---

`EditorSession` exposes the auto-fix batch road (`docs/autofix-spec.md` §5/§7):

- `getFixOffers(select)` — every offered fix of a selection, paired with the
  `(path, start, end, code)` of the diagnostic it discharges and a
  `batchable` flag (whether `fixAll` would take it unattended).
- `countFixes(select)` — how many fixes one batch round would take.
- `fixAll(select)` — the fixpoint loop, answering the `Report` plus
  `files: [{ path, new_source }]`. The session is left exactly as it was
  found; the host applies the sources through its own seam.
- `getFixesInFile(path, offset)` / `applyFixInFile(path, fix)` — the
  cursor-menu pair for a file other than the active one.

A selection may restrict by `codes`, `tiers`, `path`, and an app-scope
`ceiling` (`"off" | "ask" | "auto"`), which only ever narrows what
`brink.toml`'s `[fix]` table allows. That table is now read: a code promoted
to `"auto"` becomes batchable, and one set to `"off"` is withdrawn from
every fix query.
