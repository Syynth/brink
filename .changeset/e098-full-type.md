---
"@brink-lang/web": patch
---

E098 now names the type it found in full, as an annotation would spell it
(`Map<string, int>`, `Option<int>`, `Weighted<string>`), where it used to
print `?` for several types and a bare `Array` or `Map` for the rest
(#2755).
