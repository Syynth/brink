---
"@brink-lang/web": patch
---

New diagnostic: E188 warns when a declared STRUCT's own name collides with
a reserved builtin/tower type name (issue #1865).

`annotations::resolve` checks builtin leaves (`int`/`float`/`bool`/`string`/
`content`/`divert`) and NS-A8 tower kinds (`vec2`/`vec3`/`vec4`/`quat`/
`mat2`/`mat3`/`mat4`) before it ever consults declared struct names — a
deliberate ordering (the same one that keeps `int`/`float` unshadowable),
unchanged by this fix. But a project declaring `STRUCT content { … }`
previously compiled clean with every `content`-typed annotation silently
resolving to the builtin, never the struct, and nothing said so anywhere.

`E188` now fires at the struct's own declaration, naming both the struct
and the reserved name it collides with. Warning-tier: the declaration
still compiles and constructs normally (`content#{...}` still reaches the
struct) — only a bare type annotation spelling the colliding name is
affected. Does not fire for the generic heads (`List`/`Array`/`Map`/
`Option`/`Weighted`/`Handle`), `void`, or a name shared with a declared
`LIST`/registered `Handle<K>` kind — none of those actually collide, verified
rather than assumed.
