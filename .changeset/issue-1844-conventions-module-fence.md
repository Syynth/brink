---
"@brink-lang/web": patch
---

Compiler: `E169` — pattern-claiming handlers confined to the `brink.toml`-named
conventions module (issue #1844).

The 2026-07-31 §9.1 ruling's item (4) settled an asymmetry: a `!name`-dispatched
`@[element(…)]` handler stays legal anywhere (it self-announces at the call
site), but a pattern-claiming `@[element(claims = "…")]` handler — which can
silently reinterpret ordinary prose as a call — is confined to ONE file, the
conventions module named by `brink.toml`'s new `[project] elements` key.
`#1838`/`#1847` already enforced the *placement* half (`E112`: a claim must be
a top-level `fn`); this lands the *module* half.

- `brink-project-config` parses `[project] elements` (a built-in preset name
  or a project-relative `.brink` path) into `ProjectConfig`.
- `brink-analyzer`'s `AnalysisOptions` carries it through `apply_project_config`.
- A new per-file `HirFile::claim_handlers` record (independent of
  `element_matches`) captures every declared claiming handler's name and
  annotation range, regardless of whether it ever won a claim in its own file.
- `brink-db`'s `conventions_confinement_diagnostics_query` — the one seam with
  both a file's real module identity and the resolved pointer — compares the
  two and emits `E169` (default `Error`, matching `E112`'s posture) naming the
  file the handler should live in.

Only fires when `elements` names a project-relative path; an unset `elements`
key or a bare preset name (`elements = "screenplay"`) enforces nothing yet —
see `E169`'s own doc for the exact boundary and the tracked follow-up (#1863)
for consuming an *evaluated* `fn conventions()` registry, a separate,
larger piece of work this PR does not attempt.
