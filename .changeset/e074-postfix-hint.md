---
"@brink-lang/web": patch
---

E074's refusal of a field postfix (`a.count++`) now points at the supported
spelling, `a.count += 1`, instead of telling authors to copy the field into
a temp and write it back by hand (#2905). E074's reference page gains its
explanation, example and fix.
