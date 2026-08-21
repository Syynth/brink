---
"@brink-lang/web": patch
---

#2333: `EditorSession::apply_project_config`/`discover_project_config` now
dedupe `brink.toml`-driven warning strings against the set returned by the
previous call before returning. Every edit-flush of `brink.toml`
re-analyzed the whole project and, when the file's warnings hadn't actually
changed (a standing typo mid-edit elsewhere in the file, or simply
re-applying an unchanged file), re-returned the identical warning text —
the host (`brink-studio`'s `onProjectConfigWarnings`) appends every
returned string to the 500-entry-capped Output log unconditionally, so a
config-editing session was silently evicting real compile history one
re-application at a time.

Behavior: a warning already surfaced by the immediately preceding call is
omitted; a genuinely new or changed warning still appends; a warning that
clears (the file is fixed) and later reappears (undo, or the same typo
retyped) appends again — the last-emitted set is replaced wholesale on
every call, not accumulated, so "resolved" is representable. Deleting
`brink.toml` entirely also clears the last-emitted set, so a later file
reintroducing the same warning text isn't suppressed by a stale record.
This only changes the JSON string array returned by these two methods; it
does not touch `compile_project`'s diagnostics (the Problems-panel/
db-direct-road surface stays exactly as before) and does not change when
warnings first appear, only whether an unchanged repeat re-appends.
