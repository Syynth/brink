# brink-studio specification

brink-studio is a standalone, pro-grade ink story editor with a screenplay-mode editing experience, built on CodeMirror 6 with a Rust/wasm backend (`brink-ide`). It ships as both a standalone web application (like Inky, but better) and a reusable component library that can be embedded in host applications like s92-studio.

See also: [brink-ide-spec](brink-ide-spec.md) (the query layer powering IDE features), [brink-driver-spec](brink-driver-spec.md) (pipeline orchestration), [compiler-spec](compiler-spec.md) (compilation pipeline), [runtime-spec](runtime-spec.md) (story execution).

## Motivation

Ink is a powerful scripting language for interactive narrative, but existing editing tools fall into two categories: general-purpose code editors with syntax highlighting (VS Code + Inky plugin) and the legacy Inky editor. Neither provides a writing-first experience that understands ink's structural constructs — weave nesting, choice/gather flow, divert graphs — the way a screenplay editor understands scene headings, dialogue, and transitions.

brink-studio adapts Scrivener's screenplay-mode paradigm to ink: each line in the editor is a typed "element" with specific visual treatment, keyboard behavior, and succession rules. The editor maintains awareness of the author's position in ink's weave tree and provides structural editing operations that insert syntactically valid constructs. A live preview layer adds selective visual richness — styled typography, interactive choice bracket previews, and expandable divert disclosure widgets — without replacing the underlying syntax.

The target user is a professional narrative designer investing significant time in a large ink project. The editor must scale to multi-file stories with hundreds of knots and provide the structural navigation (binder view), refactoring (rename, move knot/stitch), and comprehension (divert disclosure, choice bracket preview) tools that large projects demand.

## Architecture

### Package structure

brink-studio spans two packages in the brink repository:

```
crates/brink-web/          Rust → wasm (wasm-bindgen)
                           Compiles brink-ide, brink-compiler, brink-runtime to wasm.
                           Exports: compile, semantic_tokens, completions, hover,
                                    goto_definition, structural editing, outline, etc.
                           This is the existing brink-web crate, extended with new
                           wasm-bindgen exports as brink-ide grows.

packages/brink-studio/     TypeScript (Vite library mode + Tauri app)
                           The CM6 editor, screenplay mode, live preview, player,
                           binder panel, and standalone app shell.
                           Consumes the wasm module from brink-web.
                           Exports:
                             - Tauri desktop app (primary standalone distribution)
                             - Web app (browser-based standalone, no install)
                             - Component library (editor, player, binder — embeddable)
                             - React wrappers (for host apps)
```

**Rationale:** The Rust/wasm layer already exists as `brink-web`. Rather than creating a parallel wasm crate, brink-studio extends `brink-web` with additional wasm-bindgen exports as new brink-ide features are implemented. The TypeScript package (`packages/brink-studio/`) is the new deliverable — it contains all CM6 integration, screenplay mode logic, live preview widgets, the story player, and a standalone app shell.

### Standalone app

brink-studio ships as a standalone web application — a modern replacement for Inky. The app provides:

- **Binder panel** — project tree showing file → knot → stitch hierarchy, with drag-and-drop reordering
- **Editor panel** — the CM6 screenplay-mode editor (the core of this spec)
- **Player panel** — debug-oriented story player
- **Project management** — open/save ink files from the local filesystem (via File System Access API or file input/download fallback), manage multi-file projects

The standalone app is the primary development and testing surface. All features are built and proven here first. This is the Inky replacement — an author can open brink-studio in a browser and write, compile, and play ink stories without any other tooling.

### Embeddable components

The same components that make up the standalone app are individually exportable for embedding in host applications. A host like s92-studio can mount just the editor, just the player, or all three panels, and wire them into its own layout, file management, and state systems via props and callbacks.

**brink-studio has no knowledge of any host application's internals.** It does not import from s92-studio, does not know about SpacetimeDB, and makes no assumptions about the host's framework beyond providing React wrappers as a convenience. The integration boundary is a clean props/callbacks/ref API.

### Layer diagram

```
┌─────────────────────────────────────────────────────────────────┐
│  Host application (e.g., s92-studio)                    OPTIONAL │
│  Thin wrapper that mounts brink-studio components               │
│    - Provides file content via props / callbacks                │
│    - Owns layout, persistence, and host-specific concerns       │
│    - brink-studio has no knowledge of the host                  │
├─────────────────────────────────────────────────────────────────┤
│  brink-studio                                                    │
│  TypeScript, Vite (library mode + standalone app)               │
│    - Standalone app shell (binder + editor + player)            │
│    - CM6 editor with screenplay mode                            │
│    - Live preview (decorations, widgets)                        │
│    - Binder panel (project tree, drag-drop, navigation)         │
│    - Story player (debug-oriented)                              │
│    - React wrappers (ref-based, uncontrolled)                   │
│    - Wasm API surface (typed TS bindings)                       │
├─────────────────────────────────────────────────────────────────┤
│  brink-web wasm module                                           │
│    - brink-ide (semantic tokens, completions, hover, goto-def,  │
│      rename, code actions, structural editing, outline)         │
│    - brink-compiler (ink source → bytecode)                     │
│    - brink-runtime (bytecode execution)                         │
├─────────────────────────────────────────────────────────────────┤
│  brink-syntax / brink-ir / brink-analyzer / brink-driver         │
│  (consumed transitively through brink-ide and brink-compiler)   │
└─────────────────────────────────────────────────────────────────┘
```

### Build tooling

| Concern | Choice | Rationale |
|---------|--------|-----------|
| Language | TypeScript | Type safety for CM6's precise API surface; s92-studio is TS and consumes typed exports |
| Bundler | Vite (library mode) | Familiar from s92-studio; Rollup under the hood for production; built-in dev server |
| Output | ES modules | Consumed by s92-studio's Vite build via package import |
| Wasm build | wasm-pack | Same as brink-web today; produces `pkg/` with `.wasm` + JS glue + `.d.ts` |
| Desktop shell | Tauri | Lightweight native wrapper; Rust codebase aligns; proper filesystem access |
| Package manager | pnpm | Consistent with s92-studio monorepo |

### Dependency on brink-web

brink-studio imports the wasm module built from `crates/brink-web/`. The build process:

1. `wasm-pack build crates/brink-web/ --target web` produces `crates/brink-web/www/pkg/`
2. `packages/brink-studio/` references the pkg output (via local path or workspace link)
3. Vite handles wasm loading and initialization at runtime

The wasm module is the single integration point between the TypeScript editor and the Rust backend. All IDE intelligence flows through wasm-bindgen function calls.

### One alias map, owned by this package (#2464)

`packages/brink-studio/alias-map.ts` is the single source of truth for this
package's module resolution. `vite.config.ts`, `vite.config.embed.ts` and
`vitest.config.ts` all compose their `resolve.alias` from its exported
factories; `tsconfig.json`'s and `tsconfig.build.json`'s `paths` are JSON and
cannot import, so they stay copies that `src/__tests__/alias-map.test.ts`
compares against `studioTsconfigPaths()` and `studioBuildTsconfigPaths()`.

The map is deliberately not one flat record — the differences between the
five copies are decisions, and each is a separate export so it stays legible:

- **`STUDIO_PACKAGE_ALIASES`** — the private workspace packages the studio
  bundles, resolved to source. Applied unconditionally by every config.
- **`STUDIO_WASM_ALIASES`** — `brink-web` and `@brink-lang/web`, applied only
  where the wasm is actually loaded: the dev server (`vite.config.ts` under
  `command === "serve"`) and the embed app build. The library build applies
  neither, because it externalizes `@brink-lang/web`
  (`rollupOptions.external`) and an unconditional alias would inline the
  wrapper into the published npm bundle.
- **`studioTestWasmAliases()`** — the unit suite's variant, which repoints
  `brink-web` at `src/__mocks__/brink-web.ts`; vitest runs under jsdom and
  must not load real wasm.
- **`brink-web`'s two targets** — bundlers resolve the ESM glue file, `tsc`
  needs the package directory whose `package.json` names `brink_web.d.ts`.
  The entry carries both rather than leaving the divergence implicit.
- **`DTS_ROLLUP_EXCLUDES`** — the wasm pair, dropped from
  `tsconfig.build.json` (the `tsconfig` tsup runs the published `d.ts` rollup
  against). `@brink-lang/web` is left to resolve through `node_modules` so
  the rollup keeps it external; `brink-web` needs no mapping because
  `src/index.ts` never imports that specifier — only `packages/wasm/src`
  does, and that source is outside the rollup's program as a consequence of
  the first exclusion. Verified empirically in #2465: mapping it back emits a
  byte-identical `dist/index.d.ts`.

This invariant is this package's own, and it does not replace the
cross-package one recorded in `docs/desktop-shell-spec.md` § "Alias map
parity with the playground" (#2450): that guard, which lives in
`packages/brink-desktop`, pins the RELATIONSHIP between the desktop shell's
map and these copies, which a studio-side test cannot see. Both run, and
neither is redundant.

`tsconfig.json`'s `include` is `["src"]`, so the package's root-level config
modules are not in that program. `tsconfig.node.json` is the program that
covers them, and `pnpm --filter @brink-lang/studio typecheck` runs three
programs: `tsconfig.json` (`src/`), `tsconfig.node.json` (root-level config
modules), and `tsconfig.e2e.json` (`e2e/*.spec.ts` plus
`playwright.config.ts`, #2607). The `tsconfig.node.json` guard covers
root-level `.ts`, `.mts`, and `.cts` modules alike, comparing its `include`
array against the directory listing name for name — no globs, so a config
module added later, in any of the three extensions, fails the guard until it
is added to `include` explicitly.

The `tsconfig.e2e.json` guard is weaker by design: it does not do a
name-for-name listing comparison the way the node-program guard does.
Instead it asserts that `tsconfig.e2e.json`'s `include` array contains
`"e2e"`, that the `e2e/` directory exists with at least one `*.spec.ts` file,
and that `package.json`'s `typecheck` script actually references
`tsconfig.e2e.json`. Because coverage is granted to the whole `e2e/`
directory via `include`, rather than to files listed by name, a new spec
added under `e2e/` is automatically covered without the guard needing to
learn about it — the difference from the node program, where every new
config module must be added to `include` by hand before the guard stops
failing.

`src/__tests__/alias-map.test.ts`'s "resolves every alias to a target that
exists on disk" case runs `existsSync` over `studioWasmAliases()` and
`studioTsconfigPaths()`, which include `crates/brink-web/www/pkg` — the
gitignored `wasm-pack` output. This package's unit suite therefore now needs
that build present (`wasm-pack build crates/brink-web --target web --out-dir
www/pkg`, see "Cloud / fresh-environment sessions" in `CLAUDE.md`) even
though `brink-web` is mocked under vitest and `@brink-lang/web` resolves to
source — the alias-map guard checks the target paths the map declares, not
just the paths the test run itself resolves through. CI is unaffected: the
frontend job builds wasm before running any package's tests.

### Mock refusal payloads must equal the Rust payloads (#2568)

Because `studioTestWasmAliases()` repoints `brink-web` at
`src/__mocks__/brink-web.ts` for every one of this package's unit tests, that
mock is the only thing the studio suite ever talks to — a payload it
*understates* makes every test blind to bugs living in the field it omits.
This is not hypothetical: #2543 shipped because the mock's structural-refusal
sites answered `{ ok: false, error }` alone, while the real
`error_json`/`dir_error_json` (`crates/brink-web/src/editor_refactor.rs`)
serialize the whole `StructuralResultJs`/`DirMoveResultJs`, so a refusal
still ships `safe: true` with empty `cross_file_edits`/`introduced_diagnostics`
beside its `ok: false`. Under the understated mock a refusal read as
*unsafe*; in production it read as *safe* and was committed.

The fix is a shape-parity guard, split across both languages so neither side
is hand-copied from the other:

- **`crates/brink-web/fixtures/refusal-shapes.json` is GENERATED — never hand-edit
  it.** It's produced by `#[cfg(test)] mod refusal_shape` in
  `crates/brink-web/src/editor_refactor.rs`, which runs the real
  `error_json`/`dir_error_json`, builds a real `AutoImportJs` struct literal,
  and (#2577) also pins `CompileResult` — the compile channel's own `{ ok:
  false, error }`, whose home is `crates/brink-web/src/compile.rs`, outside
  `editor_refactor.rs` — so a field add/rename/`skip_serializing_if` change is
  a compile error or a failing assertion, not silent drift. `CompileResult`
  had no mock counterpart when #2577 pinned it — the mock's `compile_project`
  had no refusal path at all — until #2589 gave it one: `entry` not
  resolving to a loaded file, the one failure mode the mock (no compiler) can
  reproduce, mirroring the real `EditorSession::compile_project`
  (`crates/brink-web/src/editor/mod.rs`) -> `IdeSession::compile` ->
  `CompileEntryError::EntryNotFound` path. Regenerate with:

  ```sh
  BRINK_BLESS_REFUSAL_SHAPES=1 cargo test -p brink-web --lib refusal_shape
  ```

- **Both guard halves must stay green together**, not just the one you
  touched:
  - `refusal_shape` (`crates/brink-web/src/editor_refactor.rs`) — asserts the
    checked-in fixture matches what the real Rust payloads emit right now.
  - `structural-refusal-shape.test.ts`
    (`packages/brink-studio/src/__tests__/structural-refusal-shape.test.ts`)
    — reads that fixture and `toEqual`-compares every mock refusal call site
    against it, plus a source-scanning case that fails if a new call site
    answers an inline `{ ok: false, ... }` literal instead of routing through
    one of the shared refusal helpers in the mock — `structuralRefusal`,
    `autoImportRefusal`, (#2577) `dirMoveRefusal`, and (#2589) `compileRefusal`
    — checked against the `REFUSAL_HELPERS` list rather than names hard-coded
    into the guard.

  A Rust-side change therefore fails `refusal_shape` first; regenerating the
  fixture then fails the TypeScript test until the mock is updated to match.

- **The struct enumeration is discovered, not trusted (#2577).**
  `every_refusal_struct_is_in_the_fixture` (`crates/brink-web/src/editor_refactor.rs`)
  scans every `.rs` file under this crate's `src/` for a `Serialize` struct
  carrying both an `ok: bool` field and an `error: Option<String>` field —
  the signature of a payload that can refuse — and fails if that struct's
  name is missing from `generated()`. This closes the *omission* gap (a
  fourth refusal struct landing with every gate green) but does not make
  construction automatic: Rust has no runtime reflection and this crate has
  no `inventory`-style registry or derive macro to enumerate types with, so
  each shape in `generated()` is still hand-constructed from a real struct
  literal. The scan is textual, not a parse, so it has documented blind
  spots: a hand-implemented `Serialize` (no `#[derive(Serialize)]` to match
  on) slips past, as does a struct built by a macro rather than written out
  field-by-field, and a `#[derive(...)]` line split across multiple lines
  (the scanner expects the derive and the struct signature on adjacent
  lines). A struct that hits one of these blind spots ships silently
  unguarded, the same as before #2577.

### Shape is checked; vocabulary is driven for nearly every site (#2603, #2620)

Everything above pins which *keys* a refusal payload ships. It says nothing
about the `error` *string* inside those keys — until #2603, the fixture
carried a placeholder (`REFUSAL`) there, and every string on the TypeScript
side was hand-typed. Two of the doc-handle sites were typed **from the
mock**: `auto_import_include_doc`/`auto_import_apply_include_doc` pinned the
mock's own `"unknown handle"` against production's `"unknown document
handle"`, so the guard asserted only that the mock agreed with itself. That
was the fourth instance of the class (after #2583's invented serde message,
#2599's shadowed stub, #2602's invented `entry file '...' not found`).

- **The fixture now also carries a `messages` map**, alongside `shapes`:
  `{ "<op>:<refusing-input>": "<real error string>" }`. It is produced by
  `driven_messages()` in `#[cfg(test)] mod refusal_shape`
  (`crates/brink-web/src/editor_refactor.rs`), which constructs a real
  `EditorSession`, calls the real production op with an input that refuses,
  and reads `error` back out of the JSON payload — after first asserting
  `ok: false`, so a message can never be harvested from a *successful*
  answer. Nothing in the map is typed by hand. `driven_messages()` is the
  merge of two halves, kept apart because only one of them is measured
  against the handle vocabulary:
  - `driven_doc_handle_messages()` — the document-handle ops (#2603, plus
    #2621's read-only-mount fence).
  - `driven_op_messages()` — every other refusal site (#2620): the
    structural ops, `rename_dir`, `resolve_code_action`, `compile_project`.
- **`structural-refusal-shape.test.ts` reads its expectations from that map**
  via `productionMessage(key)`. As of #2620 that is **every site in the file
  but one** — the mock-only serde abbreviation noted below is the sole
  `error:` string in its call-site arrays that is typed, and it is anchored
  as a checked prefix rather than free-typed. A site that uses
  `productionMessage` cannot drift from production: changing the Rust wording
  restales the fixture, and the regenerated fixture fails the TypeScript test
  until the mock is updated to match. `productionMessage` also records the key
  it resolved into a module-level `consumedMessageKeys` set; a canary in the
  same file asserts that set equals `Object.keys(fixture.messages)`, keyed
  rather than valued so that deleting a case cannot hide behind another case
  sharing the same string.
- **Driving is also the verification mechanism, and it found three lies.**
  #2620's sweep converted ~28 hand-transcribed strings into driven ones, and
  three of them turned out to be strings production never emits — the mock had
  been answering them and the guard had been asserting them:
  `rename_file` on a missing file (`file not loaded` → production's
  `file '{0}' not found`, from `brink_ide::file_rename`), and `delete_symbol`
  for both an unloaded path and a missing KNOT (`file not loaded` /
  `symbol not found` → the single `MoveError::SourceNotFound`,
  `source knot not found` — true only when the knot itself is missing). A
  fourth site pinned the correct wording against an input production
  *accepts*: `resolve_code_action`'s no-change case drove `FormatKnot`, which
  reindents and answers `ok: true`. Driving makes that class self-detecting —
  a driver whose input does not refuse fails `refusal_message`'s `ok: false`
  assertion instead of quietly pinning nothing. A follow-up review of this PR
  (#2627) found the sweep's own fold was itself over-broad: a missing STITCH
  inside a knot that DOES exist is a different `MoveError` variant,
  `StitchNotFound` (`stitch '<name>' not found in knot`), and had no driven
  site at all until then — it is now `delete_symbol:missing-stitch-in-knot`.
- **One wording stays deliberately divergent, and is anchored as a prefix.**
  The mock has no serde, so it cannot reproduce serde_json's unknown-variant
  error (which names every known variant plus a line/column). Its abbreviation
  is registered through `mockAbbreviationOf` rather than typed free: a case
  asserts the abbreviation is a genuine *prefix* of the driven production
  string and strictly shorter than it, so it can never become an invention
  (#2583's failure mode) nor silently become an equality that should have read
  the driven message directly.
- **A third guard closes the omission gap for this one class.**
  `doc_handle_refusal_vocabulary_is_uniform`
  (`crates/brink-web/src/editor_refactor.rs`) scans every `.rs` file under
  this crate's `src/` for a refusal-message literal — the string argument of
  `error_json("…")`/`dir_error_json("…")`, or the literal in an
  `error: Some("…".to_owned())` field — that contains the substring
  `"handle"`, and asserts that set is exactly the two strings production
  uses today (`"unknown document handle"` and `"document handle is
  read-only (mounted stdlib file)"`). A fourth doc-handle op inventing its
  own `"handle"`-containing wording is red at the source, before it can
  reach a mock. **"Both guard halves must stay green together" above is now
  three**: `refusal_shape`/`doc_handle_refusal_vocabulary_is_uniform`
  (Rust), and `structural-refusal-shape.test.ts` (TypeScript).
- **The honest limits, stated once here rather than only in code comments:**
  - Vocabulary checking is **per-site, not automatic**. Nothing in this
    crate can enumerate the `(op, refusing-input)` pairs — Rust has no
    reflection and this crate has no `inventory`-style registry — so every
    driven message is a driver someone wrote in `driven_messages()`. A site
    nobody drives stays unpinned.
  - The literal scan only sees strings that exist **in this crate**. Many
    refusals — `stitch '...' not found in knot`, `source knot not found`,
    `entry file not found in session:` — are `Some(e.to_string())` over
    `brink-ide` error types, so there is no literal for the scan to find.
    #2620 drove those instead, which is why they are pinned despite being
    invisible to the scan; but driving them is a per-site job with matching
    setup, not a scan, so the two mechanisms cover different sets and
    neither subsumes the other.
  - The literal scan's own filter is narrow: it only catches a coinage that
    contains the word `"handle"`. A fourth doc-handle refusal worded
    without that word (`"unknown document id"`, `"no such document"`) has
    no literal the scan recognizes as belonging to this class and is
    invisible to it.
  - **A driven message pins one input's answer, not the op's whole
    vocabulary.** `rename_file` refuses three different ways and only those
    three are driven; a fourth branch added tomorrow is unpinned until
    someone writes it a driver.
  - **An unreachable branch gets no driver and no mock counterpart.**
    `"current file source unavailable"`
    (`crates/brink-web/src/editor/refactor.rs`) is a defensive `let ... else`
    sitting after `ensure_include` has already resolved the same source, so
    no input reaches it —
    `removing_a_file_under_an_open_handle_refuses_before_the_source_guard`
    pins that the documented route refuses one layer earlier, and goes red if
    a refactor ever makes the branch reachable. Mirroring it into the mock
    would model an answer production cannot produce (#2621, applying #2577's
    lesson that a mock method nothing can reach closes nothing).
  - **Discovery still cannot see an undriven SITE.** #2577 guarantees no
    refusal *struct* is omitted from the fixture; nothing guarantees no
    refusal *site* is. A new doc-handle op that spells its error string
    correctly but never gets a fixture entry or a mock counterpart is
    invisible to every guard here, the uniformity scan included — that scan
    only catches wrong vocabulary at sites it already knows about. This is
    the same wall one level up, named here so it is not rediscovered a fourth
    time (#2621 gap 2). #2635 is the one concrete instance found and closed:
    `resolveCodeActionImpl`'s `file not loaded` spelled the production string
    correctly (`crates/brink-web/src/editor/code_actions.rs`) and had neither a
    fixture key nor a call site, so nothing measured it. It is now
    `resolve_code_action:missing-file`. Driving one site does not solve
    site-enumeration in general; the wall stands.

### A MISSING refusal is invisible to all of the above (#2641)

Everything in the two sections above compares a refusal the mock *emits* — its
shape, then its wording. None of it can see an op that does not refuse at all.
That is a distinct blind spot, and it is the harder one: no amount of driving
strings out of production detects a mock that answers `ok: true`.

`delete_symbol` was the instance. It located its target with one
`lines.findIndex` over the WHOLE file, while
`brink_ide::structural_delete::delete_symbol` resolves the knot first
(`MoveError::SourceNotFound`) and only then looks the stitch up **inside that
knot's body** (`MoveError::StitchNotFound`). So the mock succeeded and DELETED
in two cases production refuses — a stitch named under the wrong knot, and a
knot that does not exist at all (the `knotRe` guard #2627 added ran only on the
not-found branch, so a hit anywhere in the file meant the knot was never
checked). Both are now knot-scoped, driven as
`delete_symbol:stitch-under-wrong-knot` /
`delete_symbol:stitch-under-missing-knot`, and additionally asserted
behaviourally — a refusal carries no `new_source`, so a regression reads as
"content was deleted", not as a key diff.

`rename_symbol` had the same shape in a smaller way (#2634): production refuses
`symbol not found` when `brink_ide::rename::declaration_offset` resolves no
declaration, and the mock had no such branch, so renaming a knot that had been
edited away answered `ok: true` — the exact case
`performSymbolRename` (`packages/studio-ui/src/symbolMenuActions.ts`) names as
one it notifies the author about. It is now `rename_symbol:missing-symbol`.

Two decisions that go with it, recorded because "mirror every literal" is the
wrong reflex (#2577's lesson):

- **`rename_symbol`'s `file not loaded` guard is FAITHFUL — do not "fix" it.**
  It is the one op of the three #2620 swept that really does emit
  `error_json("file not loaded")` at the wasm level
  (`crates/brink-web/src/editor/refactor.rs`); `rename_file` and
  `delete_symbol` delegate straight to `brink-ide` with no wasm-level file
  guard, which is why *their* wording was lying. After a wave that found three
  wrong strings, the live risk is correcting the one that is right.
- **`no analysis` and `cannot rename this symbol` get NO mock branch.**
  `no analysis` fires only when `file_id` resolves but `brink-db`'s
  `is_source_file` excludes the path — an extension that is neither `.ink` nor
  `.brink`, which has no outline and therefore no symbol menu to invoke a
  rename from. `cannot rename this symbol` sits below the `symbol not found`
  guard, so it is reached only after a declaration resolved, and every case
  `rename` declines (`External`, a UFCS field call, a prelude intrinsic) names
  a symbol `declaration_offset` cannot reach — it walks `hir.knots` and their
  stitches only. Its wording is not unpinned either: the F2 road
  (`rename_symbol_at`) does reach it and drives it as
  `rename_symbol_at:unrenameable`. Both decisions are pinned by tests rather
  than argued —
  `rename_symbol_says_no_analysis_only_for_a_non_source_extension` and
  `rename_symbol_answers_once_a_declaration_resolves`
  (`crates/brink-web/src/editor_refactor.rs`) go red if either branch becomes
  reachable from the name-based road.

### The missing-refusal class now has a mechanism (#2661)

#2641 and #2634 were both found by *reading* the two implementations side by
side. That does not scale and does not recur reliably, so #2661 audited the
remaining structural ops for the same shape and — more importantly — gave the
class a driven guard instead of another pair of hand-written cases.

**The mechanism.** The generated fixture carries two more machine-produced
maps beside `shapes` and `messages`:

- **`sources`** — the exact source text every driver ran against.
  `structural-refusal-shape.test.ts` asserts its own constants are
  byte-identical to them. Before this, "byte-identical to the TS fixtures" was
  a comment on both sides and nothing checked it, yet every parity claim in the
  file depends on it: a driven answer is evidence about the mock only if the
  mock was asked the same question.
- **`acceptance`** — each driven `(op, input)` pair's own **`ok` flag** plus the
  `error` beside it, produced by `driven_outline_acceptance()` /
  `driven_extract_acceptance()` calling the op on a real `EditorSession`. The
  TypeScript side asks the mock the same question and compares both fields, so
  a mock that succeeds where production refuses, refuses where production
  succeeds, **or** refuses for a different reason is red. Offsets are derived
  from the source text on both sides (`find` / `indexOf`), never typed twice.

A third map, **`headers`**, covers what acceptance still cannot see: two ops
that answer `ok: true` on both sides while rewriting the header differently.
It stores the header line out of production's own `new_source`.

**What the audit found.** Seventeen of the twenty-one driven cases were red
against the pre-#2661 mock (the four greens were deliberate positive controls).
They reduce to three root causes, each fixed by scoping the mock's lookup to
mirror production's resolution rather than by adding a special case:

| op | production refuses / accepts | mock before |
| --- | --- | --- |
| `promote_stitch` | name collision with a top-level **function** knot | succeeded — **missing refusal** |
| `reorder_knots` | a function knot counts toward the permutation | accepted a short order — **missing refusal** |
| `reorder_knot` / `move_stitch` / `demote_knot` | a function knot resolves like any knot | refused `source`/`destination knot not found` |
| `reorder_stitch` | `stitch '…' not found in knot` inside a function knot | refused with the knot wording |
| `reorder_stitches` | `Ok(source)` unchanged when the knot has no stitches | refused `invalid reorder` |
| `extract_to_*` | `invalid extraction name`, `selection crosses a knot or stitch header`, `name collision … variable, const, or list`, a blank-line `empty selection`, `selection cannot be a function body` | succeeded — **five missing refusals** |
| `extract_to_*` | `name collision: '…' already exists as a top-level knot` | refused with `a knot or function named '…' already exists`, a string production never emits |
| `demote_knot` / `promote_stitch` header rewrite | `= function greet()` / `=== deal(n) ===` | header left untouched / `=== deal ===(n)` |

**The root causes, and why the fixes are scoping rather than special cases:**

- **The mock's knot regex could not see a `function` knot.** `parseOutline`
  matched `/^===\s+(\w+)\s*===/`, while production resolves knots through
  `brink_syntax`'s `tree.knots()`, whose `KnotHeader::name()` answers the bare
  `greet` for `=== function greet() ===` exactly as it does for a plain knot
  (`document_symbols` merely tags it `detail: "function"`). One regex
  (`KNOT_HEADER_RE`, now shared) fixed every op that resolves a knot.
  ⚠ **Any knot-matching regex needs `(?:function\s+)?`** — PR #2658's own fix
  introduced this trap and its review caught it. `FUNCTION_KNOT` /
  `KNOT_AND_FUNCTION` exist as fixtures so a new one goes red.
- **The two header rewrites interpolated the declared name.** Production's
  `rewrite_stitch_to_knot_header` / `rewrite_knot_to_stitch_header` are
  **name-agnostic**: they strip the `=` fences and keep whatever is between
  them. The mock now does the same (`rewriteFirstHeader`), which fixes the
  function-knot header *and* the parameterised-stitch header at once, rather
  than adding a `function` case to a name-based regex.
- **`extract_to_*` modelled three of `ExtractError`'s eight variants.**
  `extractImpl` now runs production's own sequence — validate name, empty
  selection, snap to lines (`snapToLines` mirrors `snap_to_lines`, including
  its "`hi` already at a line start" rule), header crossing, knot collision,
  global collision, whitespace-only selection, then the function-body check.
  The order is production's, so an input that trips two checks gets the same
  answer on both sides.

**What this still does not close.** Acceptance is driven **per-site**, exactly
like vocabulary: `driven_acceptance()` is a list someone wrote, so an
`(op, input)` pair nobody drives is still invisible. The #2577 wall stands —
what changed is that the class now *has* a mechanism, and adding a case to it
costs one entry on each side instead of a fresh argument.

## Visual hierarchy

ink's structural elements map to a three-level hierarchy inspired by Scrivener's organizational model:

| ink construct | Scrivener analog | Binder role | Visual weight |
|---------------|------------------|-------------|---------------|
| File (`.ink`) | Binder folder | Top-level container | Not visible in editor; shown in binder |
| Knot (`=== name ===`) | Folder / Part | Chapter-level grouping | Large heading, strong visual break |
| Stitch (`= name`) | Document / Scene | **Primary editing unit** | Scene heading, prominent but smaller than knot |
| Labeled gather/choice | Bookmark | Inline sub-heading | Subtle heading within stitch body |

**Stitches are scenes.** This is the central design insight. The stitch is the primary unit of work — the thing an author opens to write, the thing that appears in the binder as a navigable item, the thing that can be dragged and reordered. Knots are organizational chapters that group stitches. Files are acts or volumes that group knots.

The binder tree structure:

```
act1.ink                      (file)
  chapter1                    (knot — chapter level)
    scene1                    (stitch — scene level, primary editing unit)
    scene2                    (stitch)
  chapter2                    (knot)
    scene1                    (stitch)
    scene2                    (stitch)
act2.ink                      (file)
  ...
```

Within a stitch body, labeled gathers (`- (label_name)`) and labeled choices (`* (label_name) [Choice text]`) appear as inline sub-headings. They are navigable (shown in an outline panel, linkable) but are not binder-level items — they don't participate in drag-and-drop reordering at the binder level.

## Element type catalog

Every line in the editor is classified as one of the following element types. Each type has three properties: **visual treatment** (how it looks), **entry trigger** (how you create it), and **succession** (what happens when you press Enter).

### Structure elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Knot header** | Large font, bold, full-width rule above. Strong visual break. Distinct background band. | Type `===` at start of line, or use binder "new chapter" action. | New stitch header (if knot has stitches) or narrative text |
| **Stitch header** | Medium font, bold, subtle rule above. Scene heading style. | Type `=` at start of line (single equals), or use binder "new scene" action. | Narrative text |

### Flow elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Narrative text** | Body font, normal weight. Standard prose appearance. Full line width. | Default element type in any body context. Typing any non-sigil text. | Narrative text (same depth) |
| **Choice** (`*` non-sticky, `+` sticky) | Single sigil (`*` or `+`) regardless of weave depth. Indentation reflects depth. Bracket content `[...]` gets distinct styling. Sticky choices (`+`) get a subtle visual indicator (e.g., pin icon or different bullet). | Type `*` or `+` at start of content line. Tab on a gather line converts it to a choice. | New sibling choice (same depth and type) |
| **Gather** (`-`) | Single dash, indentation reflects depth. Subtle horizontal rule styling — acts as a convergence marker. | Type `-` at start of content line. Shift+Tab on a choice line converts it to a gather (exits choice block). | Narrative text (at gather's depth) |

### Control flow elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Divert** (`->`) | Right-aligned when standalone at end of line (screenplay transition style). Arrow symbol `->` preserved, target name styled as a link. Disclosure widget (expand to preview target content). | Type `>` at start of content line (inserts `-> ` and triggers target completion). | Narrative text |
| **Divert (inline)** | Stays inline, not right-aligned. Arrow and target styled as a link. Disclosure widget available. | Type `->` within a line. | N/A (inline, not a line element) |
| **Tunnel** (`-> target ->`) | Inline styling, not right-aligned. Visually distinct from plain divert (e.g., bidirectional arrow indicator). | Type full tunnel syntax. | Narrative text |
| **Thread** (`<- target`) | Inline styling with thread indicator. | Type `<-` at start of line. | Narrative text |

### Logic elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Logic line** (`~`) | Monospace font, dimmed/muted color. Visually recessed — this is "backstage" content that doesn't produce player-visible output. | Type `~` at start of content line. | Narrative text |
| **Variable declaration** (`VAR`, `CONST`) | Monospace font, keyword highlighted. Typically appears in file preamble. | Type `VAR` or `CONST` at start of line. | Variable declaration (when in preamble) or narrative text |
| **List declaration** (`LIST`) | Monospace font, keyword highlighted, list items with enum-member styling. | Type `LIST` at start of line. | Narrative text |
| **Temp declaration** (`~ temp`) | Same as logic line. | Type `~ temp` or Tab from a logic line context. | Narrative text |

### Meta elements

| Element | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------------|---------------|---------------------|
| **Comment** (`//`, `/* */`) | Italic, dimmed. Distinctly "not content." | Type `//` at start of line. | Comment (for block comments) or narrative text |
| **Tag** (`#`) | Pill/badge styling after content, or on its own line with decorator color. | Type `#` after content or at line start. | Narrative text |
| **Include** (`INCLUDE`) | Monospace, file path styled as a link (clickable to open file). Typically in file preamble. | Type `INCLUDE` at start of line. | Narrative text |
| **External** (`EXTERNAL`) | Monospace, function signature styling. | Type `EXTERNAL` at start of line. | Narrative text |

### Screenplay elements

Screenplay elements are editor conventions layered on top of valid ink syntax. They are not recognized by `line_contexts()` (which reports them as narrative) — instead, a client-side post-pass in the TS layer pattern-matches their syntax and assigns screenplay element types. This keeps the brink-syntax and brink-ide layers unaware of screenplay conventions.

The underlying ink syntax uses `@Name:<>` for character lines and `(text)<>` for parentheticals. The `:<>` is colon + glue — the runtime sees `@Name:` as a recognizable pattern for downstream game engines, and `<>` (standard ink glue) merges the character/parenthetical line with the following dialogue line into a single output line.

| Element | Ink syntax | Visible in editor | Visual treatment | Entry trigger | Succession (Enter) |
|---------|-----------|-------------------|-----------------|---------------|---------------------|
| **Character** | `@Name:<>` | `NAME` (centered, bold, accent color) | `@`, `:`, `<>` hidden by replace widgets. Name text uppercased in display. Centered on the line. | Tab on a blank line preceded by a blank line inserts `@:<>` template, cursor between `@` and `:` | Dialogue (new line below) |
| **Parenthetical** | `(text)<>` | `(text)` (italic, dimmed, indented) | `<>` hidden by replace widget. Parentheses visible, styled. Indented and italic. | Tab from character line or empty dialogue line | Dialogue (new line below; if empty, converts to dialogue) |
| **Dialogue** | Plain narrative text following character or parenthetical | Normal text (indented from both margins) | Wider indent than narrative, narrower than full width. Screenplay dialogue layout. | Enter from character or parenthetical line. Tab from narrative after double-blank. | See transition table below |

**Character line structure:**
```
@Name:<>
│ │   ││
│ │   │└─ glue (hidden) — merges with next line in runtime output
│ │   └── colon (hidden) — separator for runtime pattern matching
│ └────── name text (visible, centered, bold, uppercased)
└──────── character sigil (hidden)
```

**Parenthetical structure:**
```
(text)<>
│    │││
│    ││└─ glue (hidden) — merges with next line
│    │└── close paren (visible, styled)
│    └─── parenthetical text (visible, italic)
└──────── open paren (visible, styled)
```

**Cursor restrictions:** The cursor cannot enter the `@`, `:`, or `<>` regions. These are atomic replace decorations. If the user backspaces from the line below into a character line, the cursor lands between `@` and `:` (in the name text region). If the user presses Enter in the middle of the name (e.g., `@Hello|friend:<>` where `|` is cursor), the result is:
```
@Hello:<>
friend
```
The second line becomes plain narrative text (the name is split, the sigils stay with the first part).

**Smart backspace:** On a character line with no name text (`@:<>`), Backspace clears the entire line including all sigils, returning it to a blank line. Shift+Tab on any screenplay element strips the sigils and converts to plain narrative text.

**Screenplay element transitions (Tab / Enter):**

| Current element | Tab | Enter | Shift+Tab |
|----------------|-----|-------|-----------|
| **Character** | Parenthetical | Dialogue (new line) | Strip to narrative |
| **Parenthetical** | Dialogue | Dialogue (empty → convert; non-empty → new line) | Strip to narrative |
| **Dialogue (empty)** | Parenthetical | Element picker dropdown | Strip to narrative |
| **Dialogue (with text)** | Parenthetical | Action/narrative | Strip to narrative |
| **Blank line** (after blank) | Character (insert `@:<>`) | Element picker dropdown | — |

Shift+Enter within dialogue inserts a new line that stays within dialogue format.

The **element picker** is an inline dropdown (similar to the existing element type dropdown in the status bar) that appears on Enter from a blank or empty dialogue line, allowing the user to choose the next element type (character, parenthetical, dialogue, narrative, choice, gather, divert, etc.).

**Classification:** Screenplay elements are identified by a TS post-pass in `element-type.ts`, in the same layer that already promotes blank lines after choices to choice bodies. The post-pass runs after `line_contexts()` returns from wasm and pattern-matches:
- Line matching `^@[^:]*:<>$` → Character
- Line matching `^\(.*\)<>$` → Parenthetical
- Narrative text immediately following a Character or Parenthetical line → Dialogue

**Autocomplete for character names** is deferred to a generic pattern-matching autocomplete capability in brink-ide (see deferred items). When implemented, it will collect all `@Name:` occurrences across the project and suggest them when typing in a character line. This capability will also be reusable for tag autocomplete and other pattern-based suggestions.

### Inline elements

Inline elements live within content lines and do not have their own element type in the state machine. They receive rich styling within the line:

| Element | Visual treatment |
|---------|-----------------|
| **Inline conditional** (`{cond: a \| b}`) | Braces styled as delimiters, condition expression highlighted, branches visually separated. Stays as syntax in v1. |
| **Inline sequence** (`{&a\|b\|c}`) | Sequence type sigil (`&`, `~`, `!`, etc.) highlighted, branches visually separated. Stays as syntax in v1. |
| **String interpolation** (`{expression}`) | Expression highlighted within braces. |
| **Glue** (`<>`) | Subtle symbol, dimmed. |

**Multi-line blocks:** When an inline conditional or sequence opens a multi-line block (the branches contain statements on their own lines), standard element-type behavior applies within those blocks. The opening `{condition:` line is treated as a conditional opener, and lines within the branches are classified by their own element types.

## State machine

### Weave cursor

The editor maintains a **weave cursor** — the author's current position in the choice/gather nesting tree. The weave cursor has a depth (0 for top-level content, 1 for content inside a first-level choice, etc.) and a context (whether you're in a choice body, at the choice level, or at a gather).

The weave cursor is not a separate data structure from the CM6 editor state — it is derived from the current cursor position by analyzing the surrounding syntax tree. However, the state machine uses it to determine the behavior of Enter, Tab, and Shift+Tab.

### Key transitions

| Current element | Key | Result | Weave cursor change |
|----------------|-----|--------|---------------------|
| Narrative text (any depth) | Enter | New narrative text line at same depth | None |
| Choice line (`*`/`+`) | Enter | New sibling choice at same depth and type | None |
| Choice line | Shift+Enter | New narrative text line inside choice body | Depth +1 (enters choice body) |
| Choice body content | Enter | New narrative text line at same depth (inside body) | None |
| Any line at depth > 0 | Shift+Tab (at line start) | Depends on context — see below | Depth -1 |
| Any line at depth N | Tab (at line start) | Depends on context — see below | Depth +1 |
| Gather line | Enter | New narrative text line at gather's depth | None |
| Stitch header | Enter | New narrative text line | Depth resets to 0 |
| Knot header | Enter | New stitch header or narrative text | Depth resets to 0 |

### Tab / Shift+Tab behavior

Tab and Shift+Tab at line start navigate the weave depth by converting the current line's element type:

**Tab (increase depth):**
- Narrative text at depth N becomes narrative text at depth N+1 (indented into the previous choice's body)
- Gather at depth N becomes a choice at depth N+1

**Shift+Tab (decrease depth):**
- Choice body content at depth N becomes a new sibling choice at depth N-1 (exits the choice body, becomes a peer choice)
- Choice at depth N becomes a gather at depth N-1 (exits the choice set)
- Gather at depth N becomes narrative text at depth N-1

The visual indentation updates immediately to reflect the new weave position. The underlying ink syntax (number of sigils) is rewritten to match.

### Sigil-based element conversion

At the start of a content line (before any non-whitespace content), typing a sigil converts the line's element type:

| Typed | Conversion |
|-------|-----------|
| `-` | Line becomes a gather. Depth determined by current weave cursor. |
| `*` | Line becomes a non-sticky choice. Depth determined by current weave cursor. |
| `+` | Line becomes a sticky choice. Depth determined by current weave cursor. |
| `~` | Line becomes a logic line. |
| `>` | Inserts `-> ` (divert arrow + space) and triggers completion of valid divert targets. |

Sigil conversion happens only at line start, before any content. Typing `*` in the middle of a narrative line does not convert it to a choice.

## Visual treatment details

### Choice bracket hover

Choice brackets (`[...]`) receive interactive hover behavior that teaches ink's text suppression mechanics:

- **Default state:** Bracket content has distinct styling (e.g., different background, subtle border) to visually separate it from the "before" and "after" parts of the choice text.
- **Hover over bracket content:** The "before" text and bracket text are shown in their "choice presented to player" rendering. The "after" text is dimmed, showing that it won't appear in the choice label.
- **Hover over before/after content:** The bracket text is dimmed, showing the "output after choice is selected" rendering (before text + after text, bracket text suppressed).

This provides an interactive preview of how ink's three-part choice text model works without leaving the editing context.

### Divert disclosure widget

Standalone diverts display a disclosure widget (expand/collapse toggle) that, when expanded, shows the first few lines of the divert target's content inline below the divert line. This provides a "peek" at where the divert goes without navigating away.

Implementation: CM6 line widget decoration. The widget:
1. Resolves the divert target via brink-ide's goto-definition
2. Reads the target's source content from the wasm module
3. Renders a read-only preview block below the divert line
4. Collapses on click or when the cursor moves away

Cross-file diverts show the target file name as a header in the disclosure.

### Divert right-alignment

Standalone end-of-line diverts (`-> target` as the sole content of a line, or appearing after content with nothing following) are right-aligned like screenplay transitions. The `->` symbol and target name are pushed to the right edge of the editor.

Inline diverts (appearing mid-line with content after them), tunnels (`-> target ->`), and threads (`<- target`) are NOT right-aligned. They stay in place to avoid visual confusion when flow control is embedded in content.

### Weave depth indentation

Choices and gathers at weave depth N are indented by N levels (using a configurable indent width, default 2em). The raw ink syntax uses repeated sigils for depth (`* *` for depth 2), but the editor displays a single sigil with indentation:

| Raw ink | Editor display |
|---------|---------------|
| `* Choice at depth 1` | `* Choice at depth 1` (no indent) |
| `* * Choice at depth 2` | `  * Choice at depth 2` (1 level indent) |
| `* * * Choice at depth 3` | `    * Choice at depth 3` (2 levels indent) |
| `- Gather at depth 1` | `- Gather at depth 1` (no indent) |
| `- - Gather at depth 2` | `  - Gather at depth 2` (1 level indent) |

The underlying document still contains the full ink syntax with repeated sigils. The editor's decoration layer hides the extra sigils and applies indentation. Editing operations (Tab, Shift+Tab, typing) update the actual syntax.

### Typography

The editor uses a proportional body font for narrative content and a monospace font for logic/code elements. This visual split reinforces the distinction between "content the player sees" and "logic that runs behind the scenes."

| Element category | Font | Weight | Size |
|-----------------|------|--------|------|
| Knot header | Proportional | Bold | Large (e.g., 1.5em) |
| Stitch header | Proportional | Bold | Medium (e.g., 1.25em) |
| Narrative text | Proportional | Normal | Body (1em) |
| Choice text | Proportional | Normal | Body (1em) |
| Gather text | Proportional | Normal | Body (1em) |
| Divert | Proportional | Normal | Body (1em), right-aligned when standalone |
| Character name | Proportional | Bold | Body (1em), centered, accent color |
| Parenthetical | Proportional | Normal, italic | Body (1em), indented, dimmed |
| Dialogue | Proportional | Normal | Body (1em), indented from both margins |
| Logic / Variable / Temp | Monospace | Normal | Slightly smaller (0.9em) |
| Comment | Proportional | Normal, italic | Body (1em), dimmed |
| Tag | Proportional | Normal | Small (0.85em), pill/badge |
| Include / External | Monospace | Normal | Body (1em) |

## Binder and structure

### Outline data

brink-studio provides an outline API that returns the structural hierarchy of an ink file. This data powers the standalone app's binder panel and is available to host applications for building their own binder UI.

The outline includes:
- Knots with their names, ranges, and function flag
- Stitches within each knot, with names and ranges
- Labeled gathers and labeled choices within each stitch, as sub-heading items

This maps to the existing `document_symbols` function in brink-ide, extended to include labeled gathers and choices as leaf-level children of stitches.

### Binder panel

brink-studio ships its own binder panel component. The binder:

- Displays the file → knot (chapter) → stitch (scene) hierarchy
- Shows labeled gathers and choices as inline sub-headings within stitches
- Supports drag-and-drop reordering of stitches within a knot, and moving stitches between knots
- Supports drag-and-drop reordering of knots within a file
- Clicking a stitch navigates the editor to focused editing mode (that stitch only)
- Clicking a knot or file navigates to scrivenings mode (all content in that scope)
- Context menu: rename, delete, create new knot/stitch

The binder uses brink-studio's own structural editing wasm API for all reorder/move operations and the outline API for building the tree. When embedded, a host application may use this binder or replace it with its own UI consuming the same outline data.

## IDE features

All IDE features are powered by brink-ide compiled to wasm. The CM6 editor calls into the wasm module for intelligence and renders the results using CM6's extension system.

### Features available today (in brink-ide)

| Feature | brink-ide module | CM6 integration |
|---------|-----------------|-----------------|
| Semantic tokens | `semantic_tokens` | `EditorView.decorations` — CSS classes per token type |
| Completions | `completion` (context detection, visibility filtering) | `@codemirror/autocomplete` source |
| Hover | `hover` | `@codemirror/view` tooltip |
| Go-to-definition | `navigation::goto_definition` | Ctrl+Click handler or command |
| Find references | `navigation::find_references` | Command → highlights or panel |
| Rename | `rename::prepare_rename`, `rename::rename` | Command → inline rename widget |
| Code actions | `code_actions` | Lightbulb menu or command palette |
| Inlay hints | `inlay_hints` | `EditorView.decorations` — inline widgets for parameter names |
| Signature help | `signature` | Tooltip on `(` while typing function arguments |
| Folding | `folding` | `@codemirror/language` fold service |
| Document symbols / outline | `document` | Outline panel data source |
| Formatting | `formatting` (format region, sort knots/stitches) | Format command / on-save |

### Features requiring brink-ide extensions

These features require new functionality in brink-ide (new Rust code in `crates/internal/brink-ide/`):

#### Structural editing

Structural editing operations insert, move, or transform syntactically valid ink constructs. Unlike text editing (which operates on characters), structural editing operates on the AST.

```rust
// Proposed brink-ide API additions

/// Insert a new choice after the choice at `offset`. Returns the text edit
/// and the cursor position for the new choice's content.
pub fn insert_sibling_choice(
    source: &str,
    offset: TextSize,
) -> Option<(TextEdit, TextSize)>;

/// Insert a gather line after the current choice set containing `offset`.
/// Returns the text edit and cursor position.
pub fn insert_gather_after_choices(
    source: &str,
    offset: TextSize,
) -> Option<(TextEdit, TextSize)>;

/// Change the weave depth of the element at `offset` by `delta` levels.
/// Positive delta increases depth (Tab), negative decreases (Shift+Tab).
/// Returns the text edit that rewrites the sigils and adjusts indentation.
pub fn change_weave_depth(
    source: &str,
    offset: TextSize,
    delta: i32,
) -> Option<TextEdit>;

/// Extract a knot's source text for moving between files.
/// Returns the full text of the knot (header through end of body).
pub fn extract_knot(source: &str, knot_name: &str) -> Option<String>;

/// Extract a stitch's source text for moving between knots.
pub fn extract_stitch(
    source: &str,
    knot_name: &str,
    stitch_name: &str,
) -> Option<String>;

/// Remove a knot from the source, returning the modified source.
pub fn remove_knot(source: &str, knot_name: &str) -> Option<String>;

/// Remove a stitch from a knot, returning the modified source.
pub fn remove_stitch(
    source: &str,
    knot_name: &str,
    stitch_name: &str,
) -> Option<String>;

/// Insert a knot at a specific position (after another knot, or at the end).
pub fn insert_knot(
    source: &str,
    knot_text: &str,
    after_knot: Option<&str>,
) -> String;

/// Insert a stitch into a knot at a specific position.
pub fn insert_stitch(
    source: &str,
    knot_name: &str,
    stitch_text: &str,
    after_stitch: Option<&str>,
) -> String;

/// Reorder stitches within a knot to match the given name order.
pub fn reorder_stitches(
    source: &str,
    knot_name: &str,
    stitch_order: &[&str],
) -> String;

/// Reorder knots within the source to match the given name order.
pub fn reorder_knots(
    source: &str,
    knot_order: &[&str],
) -> String;
```

**Rationale:** Structural editing must live in brink-ide (Rust/wasm) rather than in TypeScript because it requires AST awareness — knowing where knots, stitches, choices, and gathers begin and end. brink-syntax provides the parse tree; brink-ide provides the editing operations on top of it. The TypeScript layer translates user gestures (Enter, Tab, drag-drop) into calls to these APIs.

#### Enhanced outline

The current `document_symbols` function returns knots and stitches. It needs to be extended to include labeled gathers and labeled choices as children of their containing stitch:

```rust
/// Extended document symbol with sub-heading support.
pub struct DocumentSymbol {
    pub name: String,
    pub kind: SymbolKind,
    pub detail: Option<String>,
    pub range: TextRange,
    /// Full range of the element's body (for scrivenings mode — determines
    /// the extent of the stitch when showing it in isolation).
    pub body_range: Option<TextRange>,
    pub children: Vec<DocumentSymbol>,
}
```

#### Divert target resolution for preview

The divert disclosure widget needs to look up the content at a divert target. This is already possible by combining `goto_definition` (to find the target's location) with reading the source at that location. No new brink-ide API is needed — the TypeScript layer composes these existing operations.

## Story player

brink-studio ships a debug-oriented story player component. Unlike a production game player that shows only what the player would see, this player surfaces runtime internals for authoring and debugging.

### Player features

| Feature | Description |
|---------|-------------|
| Story text | Rendered output of `continue_maximally()` |
| Choices | Clickable choice buttons with full choice text |
| Tags | Display tags for each content line and choice |
| Visit counts | Show current visit count for the active knot/stitch |
| Variable inspector | Expandable panel showing all variable names, types, and current values |
| Turn counter | Display the current turn number |
| Flow indicator | Show current position in the story (knot.stitch path) |
| Step history | Scrollable log of all content produced, with timestamps |
| Restart / Reset | Reset story state to initial |
| Navigate to source | Click on rendered content to jump to the corresponding source line in the editor |

### Player architecture

The player uses the same `StoryRunner` wasm interface as brink-web today, extended with additional query methods:

```rust
// Proposed additions to brink-web's StoryRunner

/// Get the current knot path (e.g., "chapter1.scene2").
pub fn current_path(&self) -> String;

/// Get all variable names and their current values as JSON.
pub fn variables_json(&self) -> String;

/// Get the visit count for a specific knot/stitch path.
pub fn visit_count(&self, path: &str) -> u32;

/// Get the current turn count.
pub fn turn_count(&self) -> u32;
```

### Editor-player interaction

The editor and player are separate components. The host application (s92-studio) wires them together:

1. Editor content changes trigger recompilation (debounced)
2. Successful compilation produces story bytes
3. Story bytes are passed to the player component
4. The player optionally preserves story state across recompilations (continue from current point) or resets

The player emits events that the host can use to coordinate with the editor (e.g., "user is viewing content from knot X" could scroll the editor to that knot).

## Component API surface

### Editor component

The core editor component is framework-agnostic (vanilla CM6). A thin React wrapper provides the integration surface for s92-studio.

#### Vanilla API

```typescript
interface BrinkEditorOptions {
  /** Initial document content. */
  initialContent: string;

  /** Wasm module instance (initialized brink-web). */
  wasm: BrinkWasm;

  /** Called when the document content changes. */
  onChange?: (content: string) => void;

  /** Called when compilation produces a result. */
  onCompile?: (result: CompileResult) => void;

  /** Called when the user navigates to a definition in another file. */
  onNavigateToFile?: (path: string, offset: number) => void;

  /** Called when the outline (document structure) changes. */
  onOutlineChange?: (symbols: DocumentSymbol[]) => void;

  /** Auto-compile on change, with debounce in ms. 0 to disable. */
  compileDebounceMs?: number;

  /** Whether to show the screenplay-mode visual treatment. */
  screenplayMode?: boolean;

  /** Whether to show live preview (divert disclosure, etc.). */
  livePreview?: boolean;
}

interface BrinkEditor {
  /** Replace the editor content. */
  setContent(content: string): void;

  /** Get the current editor content. */
  getContent(): string;

  /** Scroll to and highlight a specific byte offset. */
  revealOffset(offset: number): void;

  /** Scroll to a specific knot/stitch by name. */
  revealSymbol(knot: string, stitch?: string): void;

  /** Focus the editor. */
  focus(): void;

  /** Destroy the editor and clean up. */
  destroy(): void;

  /** Get the current document outline. */
  getOutline(): DocumentSymbol[];

  /** The CM6 EditorView, for advanced integration. */
  readonly view: EditorView;
}

/** Create and mount a brink editor. */
function createBrinkEditor(
  container: HTMLElement,
  options: BrinkEditorOptions,
): BrinkEditor;
```

#### React wrapper

```typescript
interface BrinkEditorProps {
  /** Document content. Changes are reported via onContentChange,
   *  but the component does NOT re-render on every keystroke.
   *  This is the "initial" content — set it to load a file,
   *  not to control every character. */
  content: string;

  /** Wasm module instance. */
  wasm: BrinkWasm;

  /** Called when content changes (debounced). */
  onContentChange?: (content: string) => void;

  /** Called when compilation produces a result. */
  onCompile?: (result: CompileResult) => void;

  /** Called when the user navigates to another file. */
  onNavigateToFile?: (path: string, offset: number) => void;

  /** Called when the outline changes. */
  onOutlineChange?: (symbols: DocumentSymbol[]) => void;

  compileDebounceMs?: number;
  screenplayMode?: boolean;
  livePreview?: boolean;
}

interface BrinkEditorRef {
  setContent(content: string): void;
  getContent(): string;
  revealOffset(offset: number): void;
  revealSymbol(knot: string, stitch?: string): void;
  focus(): void;
  getOutline(): DocumentSymbol[];
  readonly view: EditorView;
}

const BrinkEditor: React.ForwardRefExoticComponent<
  BrinkEditorProps & React.RefAttributes<BrinkEditorRef>
>;
```

**Rationale:** The React wrapper is "uncontrolled" in the sense that CM6 owns the document state internally. The `content` prop is treated as "load this content" rather than "the content must always be this value." This avoids the performance disaster of re-rendering CM6 on every keystroke. The host uses `onContentChange` to learn about edits and `ref.setContent()` to load new files.

### Player component

```typescript
interface BrinkPlayerOptions {
  /** Compiled story bytes. */
  storyBytes: Uint8Array;

  /** Wasm module instance. */
  wasm: BrinkWasm;

  /** Called when the user clicks rendered content (for source navigation). */
  onNavigateToSource?: (offset: number) => void;

  /** Whether to show the debug inspector (variables, visit counts, etc.). */
  showDebugInspector?: boolean;
}

interface BrinkPlayer {
  /** Load a new story. */
  loadStory(bytes: Uint8Array): void;

  /** Reset the current story to its initial state. */
  reset(): void;

  /** Destroy the player and clean up. */
  destroy(): void;
}

function createBrinkPlayer(
  container: HTMLElement,
  options: BrinkPlayerOptions,
): BrinkPlayer;
```

A React wrapper follows the same pattern as the editor (forward ref, uncontrolled).

### Wasm API

The wasm API is the typed TypeScript interface over the wasm-bindgen exports from brink-web. It wraps the raw wasm calls with TypeScript types and handles JSON serialization/deserialization.

```typescript
interface BrinkWasm {
  // Compilation
  compile(source: string): CompileResult;

  // IDE features
  semanticTokens(source: string): SemanticToken[];
  completions(source: string, offset: number): CompletionItem[];
  hover(source: string, offset: number): HoverInfo | null;
  gotoDefinition(source: string, offset: number): LocationResult | null;
  findReferences(source: string, offset: number): LocationResult[];
  rename(source: string, offset: number, newName: string): FileEdit[];
  signatureHelp(source: string, offset: number): SignatureInfo | null;
  inlayHints(source: string, startOffset: number, endOffset: number): InlayHint[];
  codeActions(source: string, offset: number): CodeAction[];
  documentSymbols(source: string): DocumentSymbol[];
  foldingRanges(source: string): FoldRange[];

  // Structural editing
  insertSiblingChoice(source: string, offset: number): EditResult | null;
  insertGather(source: string, offset: number): EditResult | null;
  changeWeaveDepth(source: string, offset: number, delta: number): EditResult | null;
  extractKnot(source: string, knotName: string): string | null;
  extractStitch(source: string, knotName: string, stitchName: string): string | null;
  removeKnot(source: string, knotName: string): string | null;
  removeStitch(source: string, knotName: string, stitchName: string): string | null;
  insertKnot(source: string, knotText: string, afterKnot: string | null): string;
  insertStitch(source: string, knotName: string, stitchText: string, afterStitch: string | null): string;
  reorderStitches(source: string, knotName: string, stitchOrder: string[]): string;
  reorderKnots(source: string, knotOrder: string[]): string;

  // Formatting
  formatDocument(source: string): string;
  formatRegion(source: string, knotName: string, stitchName: string | null): string;

  // Runtime
  createRunner(storyBytes: Uint8Array): StoryRunner;
}
```

## CM6 extension architecture

The screenplay mode and live preview are implemented as CM6 extensions. This section describes the key extensions and how they compose.

### Screenplay mode extensions

| Extension | CM6 mechanism | Purpose |
|-----------|--------------|---------|
| Element type classification | `StateField` | Tracks the element type and weave depth of each line. Updated on document changes by parsing line prefixes against the syntax tree. |
| Element styling | `EditorView.decorations` (line decorations) | Applies CSS classes per element type. Line-level decorations for font, weight, size, indentation. |
| Weave indentation | `EditorView.decorations` (replace decorations) | Hides repeated sigils (`* *` → `*`) and applies indentation via line padding. |
| Screenplay sigil hiding | `EditorView.decorations` (replace decorations) | Hides `@`, `:`, `<>` in character lines and `<>` in parentheticals via atomic replace widgets. Cursor cannot enter these regions. |
| Screenplay post-pass | Part of `elementTypeField` | After `line_contexts()` returns from wasm, pattern-matches `@Name:<>` → Character, `(text)<>` → Parenthetical, and following narrative → Dialogue. Same mechanism as the existing choice body promotion. |
| Sigil conversion | `EditorView.inputHandler` | Intercepts single-character input at line start. If the character is a recognized sigil, converts the line's element type instead of inserting the character literally. |
| State machine keybindings | `keymap` | Enter, Shift+Enter, Tab, Shift+Tab with context-sensitive behavior based on element type and weave depth. Includes screenplay element transitions (character → parenthetical → dialogue cycle). |
| Element picker | `EditorView` tooltip/widget | Inline dropdown on Enter from blank/empty-dialogue lines. Lets user choose next element type without a toolbar. |
| Divert right-alignment | `EditorView.decorations` (line decorations) | Applies right-alignment CSS to standalone divert lines. |

### Live preview extensions

| Extension | CM6 mechanism | Purpose |
|-----------|--------------|---------|
| Choice bracket styling | `EditorView.decorations` (mark decorations) | Applies distinct CSS class to bracket content within choices. |
| Choice bracket hover | `EditorView.domEventHandlers` + `hoverTooltip` | Detects hover over choice text regions and shows before/after preview in tooltip or by toggling CSS classes on the surrounding content. |
| Divert disclosure | `EditorView.decorations` (widget decorations) | Line widget below standalone diverts. Expands on click to show target content. |
| Semantic highlighting | `EditorView.decorations` (mark decorations) | CSS classes from semantic tokens (existing pattern from brink-web). |

### IDE feature extensions

| Extension | CM6 mechanism | Purpose |
|-----------|--------------|---------|
| Autocompletion | `autocompletion()` with custom source | Calls wasm completions API, returns CM6 completion results. |
| Hover tooltips | `hoverTooltip()` | Calls wasm hover API, renders markdown tooltip. |
| Lint/diagnostics | `lintGutter()` + `setDiagnostics` | Compiler warnings and errors from compilation. |
| Inlay hints | `EditorView.decorations` (widget decorations) | Parameter name hints from wasm inlay hints API. |
| Go-to-definition | `EditorView.domEventHandlers` (Ctrl+Click) | Calls wasm goto-definition, navigates within file or emits cross-file navigation event. |

### Extension composition

All extensions are bundled into a single `brinkStudio()` extension that the editor component installs:

```typescript
function brinkStudio(options: {
  wasm: BrinkWasm;
  screenplayMode: boolean;
  livePreview: boolean;
}): Extension;
```

Individual features can be enabled/disabled via CM6 compartments, allowing runtime toggling of screenplay mode and live preview without rebuilding the editor state.

## Standalone app

The standalone app is brink's answer to Inky — a self-contained application for editing ink projects. It composes the editor, binder, and player components into a fixed layout and provides its own project management.

### Desktop app (Tauri)

> **Owned by [desktop-shell-spec.md](desktop-shell-spec.md) since 2026-08-06** (v1 = local
> build; ruling-ledger #28 revived). This section is the original design sketch and stays
> for context; where they disagree, the desktop-shell spec wins.

The primary standalone distribution is a **Tauri desktop app**. Tauri wraps the same CM6+wasm frontend in a lightweight native shell (~5-10MB), providing:

- Native filesystem access — open/save/watch files without browser API limitations
- Native menu bar and keyboard shortcuts
- Window management (resize, minimize, fullscreen)
- OS integration (file associations for `.ink`, recent files)

The Tauri app uses the **wasm backend** (same code path as the browser and embedded versions). This keeps one integration path rather than maintaining a separate native Rust backend. If performance becomes a bottleneck on large projects, a native backend via Tauri commands is a future option.

The Tauri shell itself is minimal — its job is filesystem access and window chrome. All editor logic lives in the shared TypeScript/CM6 layer.

### Web app

The same frontend also runs as a standalone web application (no Tauri required). The web version uses the File System Access API where available, with `<input type="file">` / download fallback. This is useful for quick editing, sharing, and environments where installing a desktop app isn't practical.

### Layout

Three-panel layout (resizable):

```
┌──────────┬──────────────────────────┬──────────────┐
│  Binder  │        Editor            │    Player    │
│          │                          │              │
│  file →  │  screenplay-mode CM6     │  story text  │
│  knot →  │  editor with live        │  choices     │
│  stitch  │  preview                 │  debug info  │
│          │                          │              │
└──────────┴──────────────────────────┴──────────────┘
```

### Project management

| Feature | Tauri (desktop) | Web (browser) |
|---------|----------------|---------------|
| Open file(s) | Native file dialog | File System Access API; `<input type="file">` fallback |
| Save | Direct filesystem write | File System Access API; download fallback |
| Multi-file project | Open a directory; watch for changes | Open a directory (FSAA); manual refresh fallback |
| New file | Create on disk | Create in-memory; prompt to save |
| Recent projects | OS recent files list | localStorage |

The binder panel in standalone mode owns the full file → knot → stitch tree, including drag-and-drop reordering of stitches and knots (using the structural editing wasm API to rewrite the ink source).

### Scrivenings mode

When the user clicks a file in the binder (rather than a specific stitch), the editor shows all knots and stitches in that file concatenated — Scrivener's "scrivenings" mode. Each stitch boundary has a visual separator. The user can edit any part of the file in this view.

When the user clicks a specific stitch, the editor shows only that stitch's content (from its header to the next stitch header or knot end). This is the focused editing mode.

## Embedding in host applications

brink-studio's components are designed for embedding, but **brink-studio itself has no knowledge of any host application.** The integration boundary is a clean props/callbacks/ref API. Host-specific concerns (persistence, layout, file management) are the host's responsibility.

This section describes the integration patterns a host would use. It is guidance for host developers, not a specification for brink-studio.

### Integration pattern

A host application (e.g., s92-studio) would:

1. **Mount components** — use the React wrappers (`BrinkEditor`, `BrinkPlayer`) or the vanilla `createBrinkEditor()` / `createBrinkPlayer()` functions
2. **Provide content** — pass file content to the editor via `ref.setContent()` when the user opens a file
3. **Receive changes** — listen to `onContentChange` callbacks and persist edits to the host's storage
4. **Wire navigation** — listen to `onNavigateToFile` for cross-file go-to-definition and open the target file
5. **Use outline data** — listen to `onOutlineChange` to populate a binder/tree UI with the host's own tree component
6. **Structural editing** — call the wasm API's structural editing functions (`reorderStitches`, `extractKnot`, etc.) in response to drag-and-drop in the host's binder UI

### Binder responsibility split

The standalone app ships its own binder panel. A host application may choose to use it, replace it with its own binder UI (consuming outline data from the editor), or combine both.

| Concern | Standalone app | Host application |
|---------|---------------|-----------------|
| File management | File System Access API / download | Host's file system (e.g., SpacetimeDB, local FS) |
| File tree UI | brink-studio's binder panel | Host's project panel (consuming outline data) |
| Knot/stitch navigation | brink-studio's binder panel | Host's tree UI calling `ref.revealSymbol()` |
| Drag-drop reorder | brink-studio's binder panel | Host's drag-drop calling wasm structural editing API |

### Theming

brink-studio defines `--brink-*` CSS custom properties with sensible defaults (dark theme). A host application can override these properties to match its own theme:

```css
/* Host override example */
.host-container {
  --brink-bg: #1a1a2e;
  --brink-fg: #e0e0e0;
  --brink-accent: #64b5f6;
  /* ... */
}
```

This approach requires no JS coordination and works regardless of the host's UI framework.

## Deferred / out of scope

| Item | Status | Notes |
|------|--------|-------|
| Full inline rendering of conditionals/sequences | Future | V1 shows styled syntax. Future versions could render branch previews. |
| Custom keybind configuration | Future | V1 uses fixed keybindings. Future versions could allow user customization. |
| Spell checking | Future | Could integrate with browser spell check or a dedicated service. |
| Collaborative editing | Out of scope | Would require OT/CRDT integration at the host level. Not on the roadmap. |
| Export to PDF / print | Out of scope | Not part of the editor's responsibility. |
| Localization of editor UI | Future | V1 is English-only. |
| Undo/redo integration with host | Deferred | CM6 has built-in undo/redo. Integration with a host's undo system is a future concern for the embedding layer, not brink-studio itself. |
| Pattern-based autocomplete (brink-ide) | Deferred | Generic capability to collect pattern matches across the project (e.g., all `@Name:` occurrences for character name autocomplete, all `#tag` occurrences for tag autocomplete). Not screenplay-specific — a general brink-ide feature. |
