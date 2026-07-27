---
"@brink-lang/web": patch
---

#1407: `brink.toml` gains a `[project] unprune-dirs` key — an explicit
escape hatch for a project that legitimately keeps native `.brink` sources
under a directory name discovery otherwise prunes by default (`target`,
`.git`, `node_modules`; see `brink_source_tree::IGNORED_DIR_NAMES`). Also
adds a diagnostic naming any pruned directory that plausibly held a wanted
source file, and documents (rather than leaves ambiguous) a deliberate
decision **not** to add `.gitignore`-awareness, since discovery is a
deterministic-compilation input (#1306) and `.gitignore` resolution is not
fully determined by tracked repository content alone.

**Reachable through `@brink-lang/web`, traced:** `EditorSession::
apply_project_config` and `EditorSession::discover_project_config` both
call `brink_project_config::parse_str`/`parse_str_at` directly and return
every `ConfigWarning` to the JS caller as a JSON string array. Before this
change, a served `brink.toml` setting `[project] unprune-dirs = [...]`
produced an "unknown key `project.unprune-dirs`" warning in that array; now
the key is recognized (no warning for a real `target`/`.git`/`node_modules`
entry, or a differently-worded "not a pruned directory name, no effect"
warning for anything else — likely a typo). The escape hatch's actual
*functional* effect (widening what a native discovery walk descends into)
is **not** reachable through `@brink-lang/web`: `RealFs`/`Walk` are host-only
and never constructed on a wasm-reachable path (`brink-web`'s `compile`/
`compile_fragment` build `brink_source_tree::InMemory` directly, whose
`list()` is unaffected by `unprune-dirs`). Only the config-parsing/warning
text changes.
