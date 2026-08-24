---
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Find References works — and presents through the Search panel (the spec's open question, now ruled): the menu item and ⇧⌥F route results into the search results surface, grouped by file with line previews, cross-file included, click-to-reveal and inline-editable like text-search results. A references-mode chip names the symbol and count; typing a query returns the panel to text search. (The old in-view 3s highlight painted raw cross-file offsets into the current document — broken by design; it remains only as a fallback for hosts that wire no references surface.)
