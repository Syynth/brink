---
"@brink-lang/web": patch
---

`E095` (`#@was(name)` naming the definition's own current name — nothing
to migrate) now offers a `Safe` auto-fix: delete the stale `#@was(...)`
tag line. Reaches the Problems panel and the auto-fix batch road
(`docs/autofix-spec.md` §3/§5) via `brink-ide`'s `FIXERS` registry.
