---
"@brink-lang/web": patch
"@brink-lang/editor": patch
"@brink-lang/studio": patch
---

Keystroke micro-work toward the 8 ms frame budget (#3064): a config epoch invalidates delta-slice caches on dialect/host-manifest swaps (fixing a stale-classification bug under unchanged segment keys); one manifest fetch per document version; element-type derives per-line infos per segment under the delta protocol's version keys; the keystroke path serves the edited knot's semantic tokens from a classifier-only slice (no analysis pull — the symbol index and resolution passes leave the synchronous path entirely) with resolution-refined colors landing on the deferred refresh; occurrence highlights defer during large-document typing bursts (selection moves stay instant). Per-keystroke instrumented work on a 6k-line document drops to ~6–7 ms, with most keystrokes completing below the Event Timing API's 16 ms reporting floor.
