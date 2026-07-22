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
- **Bugfix:** an error in a file the compiled entry doesn't `INCLUDE` — a WIP
  scratch file, a second unrelated story open in the same editor session — no
  longer blocks that entry's `compileProject`. Sharing one db for compile and
  analysis meant compile's error gate briefly widened from entry-reachable to
  every file loaded in the session (a regression caught in review before this
  shipped); it's now scoped back to the entry's transitive `INCLUDE` closure,
  matching both the prior throwaway-driver behavior and the CLI's
  `discover`-scoped compile path. The unrelated file's error still surfaces
  through the editor's regular per-file diagnostics — it just no longer fails
  a different entry's build.

`compileProject`'s JS signature is unchanged. Manifest/dialect/policy behavior
for single-file projects is unchanged. The CLI's one-driver-per-invocation
compile path is untouched.
