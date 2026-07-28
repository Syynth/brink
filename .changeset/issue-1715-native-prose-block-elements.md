---
"@brink-lang/web": patch
---

Track 1 step 5a (ruled 2026-07-25, `docs/prose-dialect-spec.md` §8b/§8d):
the native `.brink` prose ground gains the screenplay preset's **block
elements** — scene headings with a trailing `[slug]` then tags
(`INT. MARKET SQUARE - NIGHT [market] #tense #act1`, in that ruled line
order), **header-scoped stitch bodies** (a scene runs to the next heading
or the enclosing close; scenes are flat siblings, and deeper nesting keeps
the general `flow x { }` spelling), block cues `@VENDOR` with extensions on
the tag channel (`@VENDOR #(v.o.)`), the compact cue `@KID: Says who?` as a
second declared pattern beside the block cue, chain-gated parentheticals
`(hushed)`, and trailing `#tag`s on a `flow` header line as container-level
per-flow tags. The lyrics element stays dropped.

This slice is the **grammar** only: attachment, the conventions `lower:`
column and the per-flow tag API are separate issues, so every one of these
shapes is reported as not-yet-lowered (`E129`) instead of being read as
ordinary prose or silently dropped. Observable through `@brink-lang/web`:
a `.brink` source compiled through the wasm package now classifies these
lines structurally and diagnoses them, where the same lines previously
compiled into player-facing narration. Part of #1715.
