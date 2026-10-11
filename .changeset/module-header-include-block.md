---
"@brink-lang/web": patch
---

A file that starts with a `#@module` header now has its INCLUDE block
recognised (#3465). Folding offers the block's fold again, and auto-import
adds a new INCLUDE to the existing block, or below the header, instead of
inserting it above `#@module`.
