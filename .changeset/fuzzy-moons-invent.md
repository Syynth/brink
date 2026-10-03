---
"@brink-lang/web": patch
---

The localization round trip is available through the wasm bindings.

`export_xliff`, `compile_locale` and `regenerate_xliff` join the exported
surface, taking `.inkb` bytes plus XLIFF text and returning XML or `.inkl`
bytes. They are thin wrappers over `brink-intl`/`xliff2` — the same calls the
`brink-cli` `export-xliff`/`compile-locale`/`regenerate-xliff` subcommands
make, minus the filesystem, which the host now does through its own APIs.

This is Stage 1 of `docs/desktop-ota-spec.md`: the desktop shell shipped a
native `brink-cli` sidecar purely so these three operations had somewhere to
run, and that sidecar is what made an over-the-air web-bundle update unsafe —
`.inkb`'s container version check is exact-match, so a wasm updated ahead of
the sidecar would emit artifacts its reader silently refuses.
