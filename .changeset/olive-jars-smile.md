---
"@brink-lang/studio": patch
---

Settings: the Player font-size knob does something now.

Its `min` is 0 — "follow the app type scale" — but its readable floor is 10, so
stepping ±1 from 0 produced 1, the store's own clamp read that as below the
floor and collapsed it straight back to 0. The stepper could never leave "off",
and the Player's prose never changed size. It has been inert since it shipped
(W13/#3306).

Line spacing and measure have the same shape and worked, because each carried a
hop across the dead zone written inline at its call site; the font size never
got one. `SettingsStepper` takes a `floor` now and owns the hop, so stepping up
out of `min` lands on the floor and stepping down off the floor lands back on
`min` — declaring the floor is the whole job, and the two inline copies are
gone. Steppers with a real `min` and no dead zone (editor font size, app font
size, indent) are unchanged.
