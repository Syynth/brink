# Localization & Saves

Two bevy-brink features build on the program/line-tables split: runtime locale
switching (swap the rendering data) and `.brkt` transcript persistence (save and
re-render the visible history). Both rely on line tables being independent of the
immutable program — see the [Overview](./index.md#the-story-asset-bundle).

## Locale switching

Switching is **global and event-driven**: one resource is the source of truth,
and a single command changes it everywhere.

| Item | Role |
|------|------|
| `BrinkCurrentLocale<M>` | resource holding the active locale (`None` = base/source language) |
| `commands.set_brink_locale::<M>(handle)` | set the locale and fire `BrinkLocaleChanged<M>` |
| `BrinkLocaleChanged<M>` | event; an observer reconciles every flow's `BrinkLocale` |
| `BrinkLocaleOverride<M>` | marker that opts a flow out of global switching |

A `.inkl` overlay loads as a `LocaleAsset`. Switch with the command:

```rust
# extern crate bevy_asset;
# extern crate bevy_brink;
# extern crate bevy_ecs;
# use bevy_asset::{AssetServer, Handle};
# use bevy_ecs::prelude::Commands;
# use bevy_brink::{LocaleAsset, SetBrinkLocale};
# fn demo(commands: &mut Commands, assets: &AssetServer) {
let spanish: Handle<LocaleAsset> = assets.load("dialogue.es.inkl");
commands.set_brink_locale::<()>(Some(spanish));   // switch
commands.set_brink_locale::<()>(None);            // revert to base
# }
```

Every non-override flow's `BrinkLocale` is reconciled to point at the localized
line tables (built by applying the overlay to the base tables, cached/shared per
`(base, locale)` so flows don't each rebuild it). Any `BrinkTranscript<M>`
re-renders automatically via the locale change. New flows read the current
locale at spawn; a catch-up reader handles `.inkl`s that finish loading *after*
a switch — so there's no per-frame polling.

The plugin retains each flow's canonical base tables in `BrinkBaseLocale<M>`, so
overlays always apply to the base (never to an already-localized table) and
reverting restores it exactly.

### Per-flow locale (polyglot NPCs)

Add `BrinkLocaleOverride<M>` to exclude a flow from the global switch, then set
its `BrinkLocale` manually with the `apply_locale_overlay` helper:

```rust
# extern crate bevy_asset;
# extern crate bevy_brink;
# extern crate bevy_ecs;
# extern crate brink_runtime;
# use bevy_asset::Assets;
# use bevy_ecs::prelude::{Commands, Entity};
# use bevy_brink::{
#     apply_locale_overlay, BrinkLocaleOverride, LineTablesAsset, LocaleAsset,
#     LocaleMode, ProgramAsset,
# };
# use brink_runtime::RuntimeError;
# fn demo(
#     commands: &mut Commands,
#     npc_flow: Entity,
#     program: &ProgramAsset,
#     base_tables: &LineTablesAsset,
#     locale_asset: &LocaleAsset,
#     mut line_tables: Assets<LineTablesAsset>,
# ) -> Result<(), RuntimeError> {
commands.entity(npc_flow).insert(BrinkLocaleOverride::<()>::default());

let handle = apply_locale_overlay(
    program, base_tables, locale_asset, LocaleMode::Overlay, &mut line_tables,
)?;
// point that flow's BrinkLocale at `handle`
# let _ = handle;
# Ok(())
# }
```

`LocaleMode::Overlay` falls back to base text for untranslated lines;
`LocaleMode::Strict` requires a full translation.

## `.brkt` transcript persistence

A `.brkt` is the serialized output history of a playthrough — an append-only log
of *structural* parts (line refs, values, glue, tags), not resolved strings.
Because it stores structure, a saved transcript re-renders against any matching
program + locale without re-running the story. Uses: a story-log mechanic, QA
capture, and the visible-history half of a save file.

### Capturing

```rust
# extern crate bevy_brink;
# use bevy_brink::{capture_transcript, BrinkFlow, ProgramAsset};
# fn demo(flow: &BrinkFlow<()>, program: &ProgramAsset) {
let bytes: Vec<u8> = capture_transcript::<()>(flow, program);
// write `bytes` into your save file
# let _ = bytes;
# }
```

The bytes embed the program's `source_checksum`, so a later load can detect a
mismatched story version.

### Re-rendering

Load saved bytes through the `.brkt` asset loader (→ `TranscriptAsset`) or
`brink_runtime::transcript::read_transcript`, then re-render against a program +
locale — checksum-validated, so a wrong-story render errors instead of producing
garbage:

```rust
# extern crate bevy_brink;
# use bevy_brink::{
#     render_transcript_asset, LineTablesAsset, ProgramAsset, TranscriptAsset,
#     TranscriptError,
# };
# fn demo(
#     transcript_asset: TranscriptAsset,
#     program: &ProgramAsset,
#     line_tables: &LineTablesAsset,
# ) -> Result<(), TranscriptError> {
let lines = render_transcript_asset(
    &transcript_asset, program, line_tables, /* plural resolver */ None,
)?;
// each entry is (text, tags) for one resolved line
# let _ = lines;
# Ok(())
# }
```

Pass any locale's line tables and the saved history localizes too — capture in
English, re-render in Spanish. Runnable demos:
`cargo run --example locale_switch` and `cargo run --example transcript_save`.
