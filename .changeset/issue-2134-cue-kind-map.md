---
"@brink-lang/editor": patch
---

Issue #2134 review finding: add the `cue` completion kind (issue #2134's
new `CompletionContext::CueName` items) to `completionType`'s `KIND_MAP`,
mapping to `"constant"` (matching the LSP side's
`CompletionItemKind::CONSTANT`). Without this entry a cue completion row
silently fell back to `"text"`, mis-rendering the row's icon and disabling
auto-open-on-completion (#229) the same way a missing `value` entry did
before #174 added it.
