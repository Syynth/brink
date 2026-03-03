# Loading & Linking

Before running a story, you need to load the compiled data and link it into a `Program`.

## Loading story data

<!-- TODO: explain the three input formats and how to load each:
  - .inkb (binary) — brink_format::read_inkb()
  - .inkt (textual) — brink_format::read_inkt()
  - .ink.json (inklecate output) — parse with brink_json, convert with brink_converter
-->

<!-- TODO: note that brink-format is internal — users should use the brink umbrella crate
     or brink-cli for loading. Show the pattern used in brink-cli's load_story_data(). -->

## Linking

```rust,ignore
let program = brink_runtime::link(&story_data)?;
```

<!-- TODO: explain what the linker does:
  - Resolves all DefinitionId references to fast runtime indices
  - Resolves external functions (host bindings, fallbacks)
  - Initializes global variable defaults
  - Produces an immutable Program
-->

<!-- TODO: error cases — UnresolvedDefinition, NoRootContainer -->
