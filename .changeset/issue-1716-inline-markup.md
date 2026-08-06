---
"@brink-lang/web": patch
---

Track 1 step 5b of #1351 (issue #1716) — the inline markup layer, native
`.brink` dialect only. `.brink` files compile through the wasm package's
native path (`brink-db`'s `Language::Native => lower_native_file`), and
story playback runs through `brink-runtime`, both of which changed:

- **XML-shaped inline spans** (`docs/prose-dialect-spec.md` §4.1):
  `<name attr="v">content</name>`, self-closing allowed (`<pause/>`,
  `<sfx name="bell"/>` — the point-marker shape, §8b.11). Freeform by
  default (§4.2): an unrecognized tag name is not a parse error.
- **Nesting doctrine** (§4.3), enforced structurally, and the **final
  escape set** (§8d.6): `\<` `\{` `\#` `\\`, a `\` before anything else is
  now a compile error (previously a bare backslash did nothing).
  **Breaking change for authors:** any existing `.brink` prose containing a
  bare backslash — Windows paths like `C:\Users\`, emoticons like `\o/`,
  or any other unescaped backslash — will now fail to compile. Fix by
  doubling the backslash: `C:\\Users\\`, `\\o/`.
- **Behavior change**: a `.brink` line containing `<...>`-shaped markup
  previously rendered as literal text (no grammar recognized it). It now
  parses as a real span; story playback renders the span's text with the
  tag stripped (`brink-runtime`'s `Line::Text` has no structured span
  surface yet — that's a separate, later ruling, §7/§9.1) — so
  `<b>bold</b>` now plays back as `bold`, not `<b>bold</b>`.
- **Wire**: `LinePart::Span` adds the `PART_SPAN` tag to the existing
  `.inkb`/`.inkl` part-tag dispatch. `PART_SPAN` was never part of the v4
  RFC's pre-reserved tag inventory (unlike `VAL_VEC2`/`VAL_WEIGHTED`, which
  needed no bump because materializing them just filled in an
  already-reserved slot), so its introduction is its own one-bump event:
  `.inkb` `VERSION` 5 → 6, `.inkl` version 1 → 2 (`docs/format-spec.md` §
  Versioning). Hash-transparent (§4.4): markup normalizes out of
  `source_hash`, so `Hello <wave>world</wave>` and `Hello world` hash
  identically — a translated line does not re-key when an author bolds a
  word.
