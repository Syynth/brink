---
"@brink-lang/web": patch
"@brink-lang/editor": minor
"@brink-lang/studio": minor
---

`[project] entry` now has a real schema slot and wins over a host's `entryFile` argument (issue
#2331, ruled 2026-08-07 "`[project] entry` beats `mountStudio`'s `entryFile`").

- `brink_project_config::ProjectConfig` gains `entry: Option<String>`, validated the same way as
  `conventions` (any non-empty string; existence/resolution is left to the consuming mount, kept
  dependency-free per #1234).
- `EditorSession` (`@brink-lang/web`) tracks the discovered file's `[project] entry` and exposes it
  via the new `configured_entry()`/`EditorSessionHandle.getConfiguredEntry()` — `null` when no
  `brink.toml` was found, or one was found that doesn't set `entry`.
- `ProjectSession` (`@brink-lang/editor`) now owns entry-file precedence: after
  `discoverProjectConfig` runs, a discovered `entry` that resolves to a real file in the session
  supersedes the constructor's `entryFile` argument (for both `compileProject()` and
  `getEntryFile()`); an `entry` that does NOT resolve to a real file falls back to the current
  `entryFile` and is reported through the existing `onProjectConfigWarnings` channel — no new
  warning channel invented. The `entryFile` constructor option is now only the configless fallback
  (and the seed path `brink.toml` discovery walks up from).
- `mountStudio` (`@brink-lang/studio`) opens the initial tab from `project.getEntryFile()` (read
  after `initialize()`, so any config supersession has already happened) instead of its raw
  `entryFile` option.
- `packages/brink-desktop`'s `resolveEntryFile` regex peek at `[project] entry` is deleted (not
  merely unused) — it shrinks to the plain configless-fallback chain, since `ProjectSession` now
  supersedes its guess whenever `brink.toml` sets a valid `entry`. Not independently versioned
  (`@brink/desktop` is private).

The embedded playground's `?fixture=native` project (`packages/brink-studio/src/main.tsx`'s
`NATIVE_FIXTURE`) already sets `entry = "story.brink"`, agreeing with the `entryFile` argument
`main.tsx` passes for that fixture — this change is a no-op there by construction, not by luck; a
test asserts the agreement holds (`config wins` test in `project-config-application.test.ts`
additionally exercises a real mismatch to prove supersession, not just agreement).
