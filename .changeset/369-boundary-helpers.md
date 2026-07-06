---
"@brink-lang/editor": minor
---

Publish the tier-1 boundary helpers (#369): re-export `CompileResult`/`Diagnostic` from `@brink-lang/web` for module identity, and export the canonical `sortDiagnostics` (positional: file → offset → errors-first; presentation order is a host choice layered on top) and `lineColAt` (offset → 1-based line:col).
