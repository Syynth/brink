---
"@brink-lang/web": patch
---

Analyzer: static key-domain warning for `contains(m, needle)` (`E152`,
issue #582, companion to #580).

Under `types = strict`, a `contains(m, needle)` call where `m` is
statically visible as a map and `needle` is statically visible as
outside the int/string/bool key domain (a float, array, map, struct,
function, LIST, divert-target, `Option`, range, `Weighted`, tower, or
handle value) now emits a `Warning`-severity `E152` diagnostic: the call
can never do anything but return `false` at runtime (#580's ruling), so
the always-false result is now flagged at compile time instead of
discovered as a silent, empty membership test. Reaches a literal needle,
a global `VAR`/`CONST`-valued needle or receiver, and a call- or
index-valued needle or receiver — anywhere the project's whole-program
type inference can classify the expression. Deliberately does **not**
flag a needle whose type is in the key domain but disagrees with the
map's own declared key type (e.g. a `string` needle against a
`map<int, _>` receiver), nor a `contains` call on an array receiver
(no key-domain restriction there), nor anything under `types = gradual`
(the runtime's own total `false` return stays the sole signal there).
Re-levelable and suppressible through the project's `[lints]` table /
`//brink-disable` like every other `Warning`-base-severity diagnostic
code.
