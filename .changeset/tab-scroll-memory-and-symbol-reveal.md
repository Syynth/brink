---
"@brink-lang/studio": patch
---

Fixed two "navigation loses my place" bugs (#3355, #3356): switching an editor tab away and back now keeps your scroll position in long files instead of resetting to the top (`InkFileDocument`'s CM6 mount effect now snapshots scroll on unmount via `useLayoutEffect`, before React detaches the deactivated tab's container — a plain `useEffect` cleanup ran too late to read it); and clicking a knot/stitch whose file is already open as a tab jumps to it in place, or focuses that tab in another group, instead of always opening a new fragment tab.
