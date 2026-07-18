---
"@brink-lang/web": patch
---

Collapsed the editor session's two context-assembly paths into one (#1032).
`compileProject` now assembles its artifact by querying the session's **own**
`ProjectDb` — the same database the background analysis pass reads — via the
new `IdeSession::compile`, instead of standing up a throwaway compiler driver
(and a second, fresh `ProjectDb`) per call. One db means one file set, one
lowering, and one analysis-options input feeding both compile and analysis, so
the two can no longer diverge on host manifest / T1b dialect / TM-3 type policy:
the class of bug that produced #1004 (manifest missing from the compile path) is
now structurally unrepresentable rather than closed by wiring each input into a
second driver.

Observable through `@brink-lang/web`:

- `compileProject` diagnostics are now keyed into the session's own db, so an
  included file's error span resolves against that file's real source (correct
  UTF-16 offsets and tab attribution) instead of a throwaway-driver `FileId`
  that could index a different file in a multi-file project.
- An unknown entry path now returns a clean `{ ok: false, error: "entry file
  not found in session: <path>" }` (previously a driver I/O error string).

`compileProject`'s JS signature is unchanged. Manifest/dialect/policy behavior
for single-file projects is unchanged. The CLI's one-driver-per-invocation
compile path is untouched.
