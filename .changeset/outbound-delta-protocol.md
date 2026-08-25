---
"@brink-lang/web": patch
"@brink-lang/editor": patch
---

Outbound delta protocol (#3064 option A): per-keystroke wasm→JS payloads for line contexts and semantic tokens drop from whole-document JSON (~1.4 MB combined on a 6k-line file) to a small segment manifest plus the edited knot's slice. New wasm surface: `getSegmentManifestDoc` (per-segment version keys — salsa identity `index:generation`, stable across shift edits, changed exactly when a segment's content changes, ABA-safe by generation) plus `getSegmentLineContextsDoc`/`getSegmentSemanticTokensDoc` slice fetches. `DocHandle.lineContexts()`/`semanticTokens()` assemble transparently from a version-keyed slice cache — same return types, no consumer changes — and fall back to the whole-document queries for fragment views, native files, older wasm builds, and mocks. Delta-reconstructed results are parity-gated against the assembled queries across the full corpus.
