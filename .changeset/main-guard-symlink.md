---
"@brink-lang/studio": patch
---

The desktop preflight scripts' main-guard now compares real paths, so it
fires when the script sits under a symlinked directory.

`import.meta.url` is symlink-resolved by Node while `process.argv[1]` is
not. On macOS — where `/var` is a symlink to `/private/var` — a script run
from anywhere under `$TMPDIR` compared unequal, and the guard silently did
not fire. `tauri.conf.json`'s `beforeBundleCommand` runs
`assert-real-sidecar.mjs` directly, and that guard exists to stop a stub
sidecar shipping; an inert guard ships exactly what it was meant to catch.
