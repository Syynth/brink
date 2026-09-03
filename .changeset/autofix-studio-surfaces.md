---
"@brink-lang/studio": patch
---

Auto-fix reaches the studio (`docs/autofix-spec.md` §7). The Problems panel
gains a per-row **Fix** button labelled with the fix's tier, a header **Fix
all safe (N)** whose `N` is the batch's own count, and fix entries in each
row's context menu beside the existing suppress items. The editor context
menu offers the fixes for the diagnostic under the pointer plus "Fix all safe
in this file", and the command palette gains "Fix: Fix all safe in project"
and "Fix: Fix all safe in this file".

Settings ▸ Editor gains **Fix on save** (`off | safe | project`, default
off) — an app-scope ceiling over the project's own `[fix]` policy, so it can
only ever be more conservative than `brink.toml`, never more aggressive.
