---
"@brink-lang/web": patch
---

Native multi-file linking (issue #1296, decision-log 2026-07-23 "Native
multi-file linking"): a multi-file native (`.brink`) project now links **every
discovered module** into the one `StoryData`, not just the entry file.

Native modules carry no `INCLUDE` edges, so the ink codegen closure — the
entry file's transitive `INCLUDE` closure — reached only the entry file and
every sibling `.brink` module silently missed codegen. Codegen now selects a
native-aware closure (`compilation_closure_files`) that ranges over the whole
discovered `.brink` module set: the discovery set is the compilation unit. The
entry file still designates the start flow (compilation universe ≠ execution
entry), and a `.brink` file that fails to compile is now an error even if no
other module references it (Rust parity: the whole module tree compiles).

Ink projects are unaffected — a project whose entry is an `.ink` file keeps the
exact `INCLUDE`-transitive-closure behavior. The oracle corpus is unchanged.
