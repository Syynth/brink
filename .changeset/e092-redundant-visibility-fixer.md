---
"@brink-lang/web": patch
---

`E092` (a redundant `#@public`/`#@private` directive restating the module's
own default) now has a `Safe` auto-fix: it deletes the directive line. The
fix is offered through the same `fixes_at`/`fixes_for` surfaces every other
fixer uses, so it shows up in the Problems panel's fix menu wherever `E092`
is diagnosed. No fix is offered for a native `.brink` file's equivalent
redundant `pub` keyword (no tag-line shape to remove) or when two
conflicting visibility directives are stacked on the same declaration.
