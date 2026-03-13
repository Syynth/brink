# Localization Overview

Brink separates executable logic from localizable text. The bytecode is locale-independent -- all user-visible text is referenced via `LineId = (DefinitionId, u16)`, a container-scoped index into the container's line table. Locale-specific content lives in `.inkl` overlay files that replace line content per container.

> **Implementation status:** The localization architecture is defined at the format level. Line templates, plural categories, and select keys are implemented in `brink-format`. Runtime integration (`.inkl` loading, plural resolution during rendering) and CLI tooling (`generate-locale`, `compile-locale`) are planned but not yet implemented.

## Design principles

- **Bytecode is locale-independent.** `EmitLine(2)` always means "line 2 of this container" -- the VM never sees text directly.
- **Text lives in line tables, not in the instruction stream.** This allows line content to be replaced without recompiling bytecode.
- **`.inkl` overlays replace line content per-container** without touching bytecode or control flow.
- **Plural and gender logic lives in the line template**, not the VM. Translators can restructure sentences, reorder interpolation slots, and alter plural forms per locale.
- **Voice acting and text localization share a single `LineId` addressing scheme.**

## The `.inkl` overlay format (planned)

- Header: magic `INKL`, format version, BCP 47 locale tag, base `.inkb` checksum
- Per-container line tables keyed by container `DefinitionId`
- Audio table mapping `LineId` to audio asset references
- Only containers present in the `.inkl` have their lines replaced; others retain the base locale text
