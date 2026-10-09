---
"@brink-lang/web": patch
---

`brink.toml` accepts `[project] name`, the project's display name

`[project] name = "Harbour Lights"` is now a known key, so the editor session
no longer reports it as an unknown `project.name` key. A blank name is
ignored with a warning, and a non-string value is a config error, like the
other `[project]` keys. The native and desktop studios show the name in place
of the project's folder name. Nothing in the web studio displays it yet.
