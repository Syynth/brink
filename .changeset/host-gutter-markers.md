---
"@brink-lang/editor": minor
---

Host gutter-marker contribution API (#343): `BrinkStudioOptions.getGutterMarkers(source, fromLine, toLine) => HostGutterMarker[]` and `onGutterMarkerClick` render host-supplied per-line markers (breakpoints, annotations, run/flag icons) in a dedicated gutter slotted after the built-in play-from-here gutter. Purely additive — absent callback changes nothing. Deterministic ordering (by line, host array order within a line), per-marker + shared click dispatch, recompute on doc changes, and an exported `refreshGutterMarkers(view)` / `refreshGutterMarkersEffect` for external marker-set changes. Also exported standalone as `hostGutterExtension`.
