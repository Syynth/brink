---
"@brink-lang/web": patch
---

Format VERSION 4 (T1a-4, #526): the single planned Tier-1 format bump. The
`.inkb` binary and the runtime transcript (`.brkt`) now serialize
`Value::Array`/`Value::Map` as tree encodings (a length prefix then
recursively-encoded children, insertion order preserved, map keys restricted to
the scalar `int`/`string`/`bool` domain) instead of folding a collection to
`null`. A binding/external that returns a collection and is inline-emitted
(`{ext()}`) now round-trips through the persisted transcript byte-for-value
identical. The strict reader hard-rejects any version but 4. The reserved
Tier-1 value-tag/section/opcode surface (function values, closures, handles,
projections, records) is frozen at v4 per the §9 one-bump rule so later
milestones add data without another bump. Compiled `.inkb`/transcript bytes read
through `@brink-lang/web` change accordingly; no opcode emits collections yet,
so scalar behavior and the oracle are byte-identical.
