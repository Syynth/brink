---
"@brink-lang/studio": patch
---

Clickable value-list picker. A value-list argument (a semantic type with a
declared `values` list) now renders an interactive chip instead of a passive
label: click it to open a filterable dropdown of the items and rewrite the
literal in place. Hosts get a click-to-pick combobox for free from a declared
value-list — no custom `ArgumentWidget` required (#224).
