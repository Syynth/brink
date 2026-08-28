---
"@brink-lang/studio": patch
---

The State View shows each call frame's local variables.

Function parameters and `~ temp`s now appear under the frame that owns
them, with their live values — so a function that computes with locals is
no longer opaque exactly while it is the thing running.

Values render structurally rather than as display strings: a list shows its
members, a struct shows its fields, and an empty list is distinguishable
from a null. A frame from a story built without debug info says so, rather
than showing an empty panel that would read as "this function has no
locals".
