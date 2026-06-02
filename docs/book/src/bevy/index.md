# Bevy Integration

`bevy-brink` exposes the brink runtime as a Bevy plugin: compiled stories load
as `Asset`s, each live conversation is an entity with flow `Component`s, and
story-wide save state lives in a `Resource`. Output is delivered through
observer events, and ink `EXTERNAL` functions bind to engine systems.

```sh
cargo add bevy-brink
```

```rust,ignore
use bevy::prelude::*;
use bevy_brink::{BrinkPlugin, BrinkFlowRequest};

fn main() {
    App::new()
        .add_plugins((DefaultPlugins, BrinkPlugin::<()>::default()))
        .add_systems(Startup, start_story)
        .run();
}

fn start_story(mut commands: Commands, assets: Res<AssetServer>) {
    commands.spawn(
        BrinkFlowRequest::<()>::builder()
            .story(assets.load("dialogue.inkb"))
            .build(),
    );
}
```

## The plugin

`BrinkPlugin<M>` registers everything for one story instance: the fulfillment
system that turns requests into live flows, the transcript refresher, the
external-binding resolvers, and the locale machinery. It does **not** add an
auto-advance system — most games drive advancement from input or game state,
not every tick, so you write that step loop yourself (see
[Spawning & Driving Flows](./flows.md)).

Adding `BrinkPlugin<M>` also pulls in `BrinkAssetsPlugin` (once) for the
marker-free asset types and loaders. You can add `BrinkAssetsPlugin` on its own
if you want the asset machinery without any marker plumbing (e.g. a headless
asset-processing binary).

### Markers: multiple concurrent stories

Every type is generic over a `Send + Sync + 'static` marker `M` (default
`()`). The marker monomorphizes the resources and components to distinct Bevy
types with no runtime cost, so independent stories coexist in one app:

```rust,ignore
struct MainStory;
struct DreamSequence;

app.add_plugins((
    BrinkPlugin::<MainStory>::default(),
    BrinkPlugin::<DreamSequence>::default(),
));
```

Each marker gets its own `BrinkGlobals<M>` resource and
`BrinkFlow<M>`/`BrinkContext<M>`/`BrinkLocale<M>` components. Use `()` unless
you actually need this.

## The three-asset bundle

A loaded story is a thin bundle of two labeled sub-assets:

| Asset | Holds | Notes |
|-------|-------|-------|
| `BrinkStoryAsset` | handles to the two below | what you `load()` and hand to a request |
| `ProgramAsset` | the immutable bytecode `Program` + `initial_context` | what the VM executes; `initial_context` is the fresh "new game" state |
| `LineTablesAsset` | the localizable line tables | the swappable rendering data (base language, or a locale overlay) |

The split exists so line tables can be swapped (locale changes, hot-reload)
without touching the immutable program. You rarely touch the sub-assets
directly — spawn a request with a `Handle<BrinkStoryAsset>` and let the
fulfillment system wire everything up.

## Loaders and file types

| Extension | Loader | Produces | Feature |
|-----------|--------|----------|---------|
| `.inkb` | `InkbLoader` | `BrinkStoryAsset` (+ labeled `#program`, `#line_tables`) | always |
| `.ink` | `InkLoader` | `BrinkStoryAsset`, compiled at load time, hot-reloads on source change | `dev` |
| `.inkl` | `InklLoader` | `LocaleAsset` (a locale overlay) | always |
| `.brkt` | `BrktLoader` | `TranscriptAsset` (a saved playthrough) | always |

The `dev` cargo feature (on by default) adds the `.ink` source loader, which
compiles ink at load time and hot-reloads the program when any file in the
`INCLUDE` graph changes. Ship a release build without it to drop the compiler
and load only pre-compiled `.inkb`/`.inkl` assets:

```toml
bevy-brink = { version = "*", default-features = false }
```

## Where to go next

- **[Spawning & Driving Flows](./flows.md)** — the request-component pattern,
  the flow components/resources, the step loop, observer events, transcripts.
- **[External Functions](./bindings.md)** — bind ink `EXTERNAL`s to engine
  code (pure / command / world-query / async / task) and call ink from the
  engine.
- **[Localization & Saves](./localization.md)** — runtime locale switching and
  `.brkt` transcript persistence.
