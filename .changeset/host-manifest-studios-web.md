---
"@brink-lang/web": patch
---

The session loads the host manifest `brink.toml`'s `[host] manifest` names
from its own documents when the config is applied (#3671), into a slot
separate from a registered one: a manifest set with `setHostManifest` still
wins, and clearing the config never drops it. Three new state queries:
`getConfiguredHostManifestError()` (why the named manifest could not be
loaded, or `null`), `getConfiguredConfigReads()` (every file the applied
config reads — `[dialogue]`'s file and the manifest — for a host to load and
watch), and `getConfiguredWarnings()` (the config's whole current warning
set, unlike the delta `discoverProjectConfig` returns).
