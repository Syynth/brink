---
"@brink-lang/web": patch
---

Hover content delivered to consumers that cannot resolve link targets — the
language server and `brink ide hover` — has its `[text](#N)` references
flattened back to plain labels.

`#N` indexes `HoverInfo.links`, which only a renderer holding that list can
resolve. Without this the LSP would hand an editor a live markdown link
pointing at a fragment that does not exist.
