# Localization Overview

Brink separates executable logic from localizable text. The bytecode is locale-independent — all user-visible text is referenced via `LineId = (DefinitionId, u16)`, a container-scoped index into the container's line sub-table. Locale-specific content lives in `.inkl` overlay files that replace line content per container.

## Design principles

<!-- TODO: explain the core localization architecture:
  - Bytecode is locale-independent: EmitLine(2) always means "line 2 of this container"
  - Text lives in line tables, not in the instruction stream
  - .inkl overlays replace line content per-container without touching bytecode
  - Plural/gender logic lives in the line template, not the VM
  - Translators can restructure sentences, reorder slots, alter plural forms per locale
  - Voice acting and text localization share a single LineId addressing scheme
-->

## The `.inkl` overlay format

<!-- TODO: explain .inkl structure:
  - Header: magic `b"INKL"`, format version, BCP 47 locale tag, base .inkb checksum
  - Per-container line tables (keyed by container DefinitionId)
  - Audio table (LineId → audio asset reference)
  - Only containers present in the .inkl have their lines replaced; others retain base locale
-->

## Loading overlays at runtime

<!-- TODO: explain how to load a locale overlay:
  - Program carries the base line tables
  - Loading an .inkl replaces per-container line content
  - Checksum validation against the base .inkb
  - Hot-swapping locales without restarting the story
-->
