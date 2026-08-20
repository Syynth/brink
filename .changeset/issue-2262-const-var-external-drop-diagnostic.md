---
"@brink-lang/web": patch
---

Issue #2262: `brink-ir`'s `CONST`/`VAR`/`EXTERNAL` global-declaration collectors no longer silently drop a declaration when its own definition can't be resolved.

- **New diagnostic `E184`** (non-suppressible backstop, the `E060`/`E073`/`E181` posture): `lir::lower::decls::collect_globals` (`CONST`/`VAR`) and `collect_externals` (`EXTERNAL`) raise it if a declaration's own self-declaration lookup comes back `None` — the same narrow "every surviving same-name candidate is std-declared" condition `E181` already backstops for `STRUCT` (issue #2240), now shared via a `lookup_global_or_diagnose` helper. Before this, the declaration silently vanished from `PreludeDecls` (no `lir::GlobalDef`/`lir::ExternalDef`) with no diagnostic at all.
- **Reachable today for `EXTERNAL`**: an ordinary project (no `#@module`, no `dialect` override needed) that declares its own `EXTERNAL scene_entered(...)` collides with the std-mounted screenplay preset's own `extern scene_entered` — see `brink-environment`'s `external_self_declaration_silently_drops_when_colliding_with_a_std_preset_name` for the reproduction through the real compile pipeline. `CONST`/`VAR` stay reachable only in principle today (`std` declares neither yet), the same status `E181` itself carried before its own reachable case was found.
