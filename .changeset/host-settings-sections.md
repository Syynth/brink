---
"@brink-lang/studio": minor
---

`mountStudio` takes a `settingsSections` option: host-provided sections
appended to the Settings rail. The row primitives a section needs
(`SettingsRow`, `SettingsGroup`, `SettingsToggle`, `SettingsStepper`,
`SETTINGS_ICONS`) and the `SettingsSection`/`SettingsScope` types are now
exported, along with `SETTINGS_SECTION_IDS` as the reserved-id set.

Appended, never merged over: a host cannot remove or replace a built-in
section, and one whose id collides with a built-in is dropped with a
warning rather than shadowing it — `SettingsModal` resolves a section by
first id match, so a duplicate would put two rows in the rail with one of
them unreachable.

The first consumer is the desktop app's Settings › Updates (update
channel, automatic checks, version history), which is meaningless in the
browser build — there is no installer there and nothing to pin.

`mountStudio` also takes a `proseChecker` option: a decorator receiving the
built-in Harper-backed checker and returning the one to use. A decorator
rather than a replacement, so a host adding a capability does not have to
reimplement — or take ownership of the lifecycle of — the 6.5 MB wasm
module behind it; the studio still creates and disposes its own.
`ProseChecker` and `ProseLint` are re-exported for it.
