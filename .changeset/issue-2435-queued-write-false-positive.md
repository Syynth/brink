---
"@brink-lang/studio": patch
---

`file.save`/`file.saveAll` no longer raise a false "…changed while saving —
still unsaved" warning when a `requestSave` queued behind another in-flight
write legitimately catches up to a later edit and persists it (issue
#2435). The #2426 mid-write guard's pre-save content comparison couldn't
tell a queued write's legitimate catch-up apart from a genuine mid-write
divergence; a path whose content moved on since the pre-save snapshot is
now re-checked against the provider's actual written content
(`ProjectSession.readProviderFile`) before being treated as stale — a
genuine divergence still fails that check and stays dirty, unchanged from
#2426's behaviour.
