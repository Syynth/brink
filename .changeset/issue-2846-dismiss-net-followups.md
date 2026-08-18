---
"@brink-lang/studio": patch
---

Dismiss-net enrolment guard follow-ups (issue #2846, following PR #2838 / #2766).
`dismiss-registry-enrolment.test.ts`'s scan widened from `document`-only to
`document`/`window`/`ownerDocument` targets and `keydown`/`keyup`/`pointerdown`
events — `dismiss-registry.ts` itself attaches its net listener on `window`, so
"attach the way the registry does" was the single most plausible unguarded
next-surface shape and previously evaded the scan entirely. Widening surfaced
that call site in both `studio-shell`'s and `ink-editor`'s `dismiss-registry.ts`,
which now each carry a `DISMISS-NET-EXEMPT` marker (that call site *is* the net,
not a surface enrolling into it) backed by a new `dismiss-registry-net-listener.test.ts`
in each package.

Every `DISMISS-NET-EXEMPT` marker in the workspace — the pre-existing three
(`tab-drag.ts`, `strip-drag.ts`, `regions.tsx`) plus `ElementDropdown.tsx` plus
the two new net-listener ones — now carries a dedicated behavioural test
proving its specific claim against the real production module, matching the
`SAVE-PATH` precedent's "proven, not just present" bar
(`docs/studio-shell-spec.md` §7.7.1).

Also fixed: `scanListenerSites` used to skip `//`-prefixed lines only, so a
block comment (e.g. JSDoc) quoting the listener shape in prose counted as a
real, unmarkable call site; block-comment spans are now blanked (line numbers
preserved) before the scan runs.

No runtime behavior changes — this is test-infrastructure and doc-comment only.
