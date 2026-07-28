# Host Capability Manifest (design — Track B)

**Status:** Tier 1 + closed Tier 2 **implemented** (see "Implementation status"
below); Tier 3 deferred. This is the head of the "tooling / analyzer
extensibility" track (Track B). It is **not** a prerequisite for the
external-function binding foundation (Track A — runtime/web binding, save/load
persistence, name-based variable access, seeding). The manifest is additive
over a bind-by-name + `Value` boundary, so it attached without a runtime
rewrite. See `docs/decision-log.md` for the two-track split.

## Why this exists

Today an ink author writes `EXTERNAL has(item)` and a host binds `has` by name
at runtime — and *nothing* connects the declaration to the host's actual
vocabulary. That single missing link is behind a cluster of otherwise-scattered
asks from the consuming projects:

- externals tagged **presentation vs. effect** (client/server authority split)
- **unbound verbs degrade, not dead-end** (a cutscene using a newer verb no-ops)
- ink vars **mapped to host data** (RPG Maker game variables / switches)
- **author-time validation, completions, and widgets** for host verbs (the
  "make the analyzer / studio do more sophisticated things" ambition)

All of these are served by describing the host's external vocabulary. It is
**tooling / author-time only** — consumed **two ways**: by the **analyzer/IDE**
(diagnostics, completion, hover, signature) and by **brink-studio** (widgets,
host data). It is **never** consumed by the runtime or by compiler codegen:
every seemingly-runtime use turns out to live elsewhere — arg validation is
author-time (the analyzer); presentation/effect routing is realized in the
host's binding *implementations*, not the VM; and the "use the ink fallback body
if it exists, else null" refinement reads `Program` metadata, not the manifest.
So the runtime keeps Track A's bind-by-name + `Value` boundary untouched. This
is net-new beyond ink (Inkle's `BindExternalFunction` is bind-by-name, no schema).

It is, in effect, a **serializable type-checking + editor-affordance schema for
the host boundary** — scoped to ink external call sites, not a general type
system for ink.

## Authoring: two sources that merge

Metadata comes from **two sources**, merged by the analyzer into one enriched
symbol index. **Inline wins** for a given external on conflict; an inline
reference to an undefined registered type is a diagnostic.

**1. Inline doc-comments** (source-resident, per-external) — JSDoc-style `///`
tags on the `EXTERNAL` declaration, co-located so the `.ink` file is
self-contained:

```ink
/// Whether the player holds an item.
/// @param item {item_id}
/// @returns {bool}
/// @kind query
EXTERNAL has(item)
```

`///` doc-comments are recognized from the existing `LINE_COMMENT` trivia (no
grammar change); HIR lowering walks the `EXTERNAL_DECL`'s leading trivia and
parses the `@param`/`@returns`/`@kind`/`@widget` tags + free-text doc. Like Rust
doc-comments, codegen ignores them — only tooling consumes them.

**2. Registered manifest** (host-owned, project-wide, dynamic) — supplied via
`EditorSession::set_host_manifest(json)` for what doesn't belong in one file:
**semantic type *definitions*** (what `item_id` / `color` / an enum *are* — the
vocabulary that inline `@param … {item_id}` references), **Tier-3 value
providers / host editors**, and **bulk/generated** per-external entries (e.g. an
RMMZ plugin emitting 50 verbs). Per-external signatures *may* also be registered
(for generated verbs) rather than annotated inline.

| Metadata | Home |
|---|---|
| Per-external param types, return, `@kind`, doc, widget ref | inline (or registered, if generated) |
| Semantic type *definitions* (item_id, color, enums) | registered (project-wide vocab) |
| Value providers, host-rendered editors | registered (host-owned) |

The author always still writes `EXTERNAL foo(x)` in ink (existence + arity, for
the compiler) — the manifest/inline annotations only *enrich* it. So nothing
downstream (compiler, runtime, another host) ever depends on the manifest; a
manifest/ink arity disagreement is a diagnostic.

## Concrete integration (where it bolts into the pipeline)

It rides the **same `ProjectDb → analyze → SymbolIndex → IDE queries` pipeline
that source files already use** — no LSP crate; `brink-ide` is the query layer.

| Concern | Owner |
|---|---|
| `HostManifest` schema (Rust, serde) + inline `ExternalDoc` | `brink-ir` (next to `SymbolInfo`/`SymbolKind`) |
| TS mirror of the schema | `@brink/wasm-types` |
| Parse inline `///` tags → `ExternalDoc` | `brink-ir` HIR lowering (from `EXTERNAL_DECL` trivia) |
| Register the manifest | host → `EditorSession::set_host_manifest` → `IdeSession`/`ProjectDb` |
| Merge both sources, enrich `SymbolInfo`, new diagnostics | `brink_analyzer::analyze` |
| Surface it (completion/hover/signature/diagnostics + code-action) | `brink-ide` (mostly already reads `index.symbols`) |
| Widgets / broker host data | brink-studio + a host-callback path — **Tier 3, separate, later** |

The analyzer already models externals as `SymbolInfo` and **arity-checks call
sites** — it just has no types to check against (ink `EXTERNAL` carries only
names). The manifest fills exactly that hole: enrich `SymbolInfo.params` with
types (in place or via a parallel `host_meta` table — TBD), add a type-mismatch
diagnostic, add vocabulary symbols for registered-but-undeclared verbs, and a
code-action to insert the `EXTERNAL` declaration from a manifest entry. The
existing completion/hover/signature queries get richer for free.

## Architecture: a scoped slice of LSP

The division of labor mirrors LSP, extended to three parties:

- **analyzer / manifest** = *semantics* — what each external means, what its
  args are, what their values may be.
- **brink-studio** = *broker / client* — places affordances, applies edits,
  renders builtin widgets, brokers host-owned editors.
- **host** (RMMZ plugin, folklore app) = *the domain UI + live data* — answers
  value queries, renders domain editors, owns the data that changes as the game
  is built.

This reuses infrastructure that already exists: `brink-ide` (completions,
hover, signature help, inlay hints, code actions, text-edit returns) and the
`EditorSession` host-callback precedent (`FileProvider`, see decision-log
#482). The manifest + provider is "teach the existing IDE layer about
host-defined symbols and their value domains," not greenfield.

## The three tiers

The manifest is three separable concerns. Knowing which tier a feature lives in
tells you exactly what it needs and what it can/can't do.

### Tier 1 — Signature & base types (pure static data)

Per external: arity, base type per param, return type, doc string, and a
presentation/effect kind tag. Fully self-contained — no host involvement, no
live data.

Enables: call-site validation ("takes a string, you passed an int"),
unknown-verb / wrong-arity diagnostics, verb-name completions, signature help,
and the fallback policy (a verb that's *in the manifest* but unbound →
no-op + log; a verb in *neither* the manifest nor bound → existing ink fallback
body, else error).

Limit: constrains *call sites* only. Ink variables stay dynamically typed at
runtime. Catches signature/type mismatches, not logic errors.

```jsonc
{ "externals": [
    { "name": "has",    "params": [{"name": "item", "ty": "string"}], "returns": "bool", "kind": "query" },
    { "name": "camera", "params": [{"name": "target", "ty": "string"}], "returns": "void", "kind": "presentation" },
    { "name": "grant",  "params": [{"name": "item", "ty": "string"}], "returns": "void", "kind": "effect" }
] }
```

**Panel categorization (`path`).** An external may carry an optional `path:
string[]` — a category → sub-category breadcrumb the Host Functions panel uses to
group and filter a large vocabulary (a real host has hundreds of verbs). Pure
static presentation metadata, like `doc`/`kind`; un-`path`'d externals fall into a
default bucket. Nested (`["Map","Movement"]`), not a flat string, so hosts can
express a real taxonomy. Designed in #210 (the panel renders collapsible sections
+ search over it).

```jsonc
{ "name": "set_move_route", "params": [{"name": "actor", "ty": "int"}], "returns": "void",
  "kind": "effect", "path": ["Map", "Movement"] }
```

### Tier 2 — Semantic / refined types (static declaration)

A param is not just `string`, it's a **named semantic type** with an optional
constraint. Two sub-cases:

- **Closed domains** — enum set, regex pattern, numeric range. Statically
  checkable from the manifest alone. Examples: `tint_screen(color)` where
  `color` is a `#RRGGBB`-pattern string; folklore's "string that's actually an
  enum."
- **Open domains** — `item_id`, `map_id`, `actor_id`. The *type* is declared
  statically, but the *valid set* lives in the host and changes as the game is
  built. Declarable here, but not populatable — see Tier 3.

**Scope guardrail:** keep semantic types **flat and nominal** — a base type
plus one optional constraint (enum / regex / range / "host-resolved domain").
Records, unions, and generics are out of scope; that way lies a type system.
Color, item_id, enum, and pattern all fit comfortably in flat-nominal.

**Handle kinds (T1d-2, docs/t1d-spec.md §3).** `base: "handle"` is a
distinct fifth base alongside `string`/`int`/`float`/`bool`/`void` — but
unlike those, it doesn't specialize a primitive. The semantic type's own
`name` field *is* the declared handle-kind name (e.g. `AudioInstance`,
`Timer`) — the vocabulary the typed dialect's `Handle<K>` annotation form
resolves `K` against (`docs/typed-mode-spec.md` §3's first amendment).
`Value::Handle { kind, id }` tokens (T1d-1) carry the kind as a `NameId`
at runtime; the manifest is where that vocabulary is declared for the
analyzer, not the format.

```jsonc
{ "name": "AudioInstance", "base": "handle" }
```

### Tier 3 — Live value providers & editor widgets (host protocol)

The part that **cannot** come from a shipped file. Two flavors of the same
architectural move (host owns the dynamic/rich part, manifest declares the
hook, studio brokers):

**(a) Value providers** — for open domains. The manifest says "param is type
`item_id`"; the host answers "here are the current item_ids" (RMMZ reads
`$dataItems`). Enables project-aware completions and "does this item exist in
*this* project" validation.

**(b) Host-rendered editors** — for rich, domain-specific value entry the studio
can't render itself (an RMMZ map editor that needs tilesets + map data).

> **The full widget UX is designed in [argument-widget-spec.md](argument-widget-spec.md).**
> This section is the *schema + protocol* source of truth; the spec covers the
> three authoring entry points (Edit a literal / Fill an empty slot / Form the
> whole call), the Host Functions panel as a Form launcher, the studio widget
> registry, the `argumentWidgets` embedder surface, and graceful degradation.

#### Widget classes

- **Studio-builtin widgets** — generic, studio renders them, no host assets, all
  through one registry: `color` (swatch + popover picker, for `hex_color`),
  `value-list` (a **label-searchable typeahead** for a type carrying `values` —
  filters on the item name, inserts the id; #211), later `number`/`vector3`/
  `bool-toggle`.
- **Host-provided editors** — studio *cannot* render these; the host owns the UI
  and assets. Studio's role is **broker + seam**.

**Inline is always studio-rendered (data-only host input).** The in-text
affordance — a swatch, or a chip reading a label — is drawn by the studio. A host
contributes inline only as *data*: a label string + an optional CSS class on the
chip span (`inline(ctx) → { text, className? }`). No host-mounted DOM in the
source line, no thumbnails. The host's rich UI lives entirely in the **editor**
(a popover or modal), which is the only host-rendered surface.

#### Type-level built-in widget

A semantic type names a studio-builtin widget via `widget` (the `@widget` inline
JSDoc tag is the `///` counterpart, already reserved):

```jsonc
{ "name": "hex_color", "base": "string", "widget": { "kind": "color" } }
```

#### Arg-group semantic types (one widget, many params)

A widget may span an **argument group**, not just a single param. `surface`
(`"popover"` default | `"modal"`) declares the editor container; a heavy host
editor (a map) requests `modal`:

```jsonc
{ "name": "place_object",
  "params": [{"name": "x", "ty": "int"}, {"name": "y", "ty": "int"}],
  "widgets": [ { "group": [0,1], "type": "map_point",
                 "editor": "rmmz.map_picker", "surface": "modal" } ] }
```

#### Inter-arg context

A widget may depend on another arg of the same call for context:

```jsonc
// teleport(actor, mapId, x, y) → picker opens map = arg 1, writes x,y = args 2,3
{ "group": [2,3], "type": "map_point", "editor": "rmmz.map_picker", "context": { "map": 1 } }
```

Both remain flat, declarative, serializable. This is the fiddly edge of the
manifest — but still short of a type system.

#### Host-editor invocation protocol

The host editor is a **studio-side mounted component** (registered through the
`argumentWidgets` embedder surface, like a tool window) — *not* a wasm↔host
callback. The studio owns the popover/modal chrome; the host fills the body and
resolves/cancels. Data flow (detail in argument-widget-spec §5):

1. Manifest declares an arg-group widget with `editor: "host.<vendor>.map_picker"`.
2. The `brink-ide` `argument_widgets` query reports the call's slots — spans,
   current values, resolved inter-arg `context` — and the studio renders the
   affordance (inline chip / Fill placeholder / Form glyph).
3. On invoke, the studio opens its chrome and calls the host's
   `editor.render(ctx, host, container)` — `ctx` carries `{ values, context }`,
   `container` is a DOM node the host mounts its UI into.
4. The host acts; calls `host.resolve(values)` (the structured result, e.g.
   `["12","8"]`) or `host.cancel()`.
5. The studio applies a **multi-slot text edit**, writing the group's spans back
   to source as one undoable transaction.

The net-new infrastructure is just the `argument_widgets` IDE query (spans + slot
state + context) and the `argumentWidgets` embedder surface — the host editor runs
in the studio's JS context, so no new wasm↔host UI callback is required (the value
provider's push-cache already covers the data side).

#### Graceful degradation

- Studio can't validate (x,y) against walkable tiles unless the host offers it —
  rarely needed; the widget constrains by construction.
- The host editor is **opaque** to studio (broker only; no introspection).
- Where no host editor is registered (a plain web playground), the manifest
  declares a **fallback widget** (e.g. number fields) so the affordance degrades
  to plain editing rather than a dead button.

## Can / can't, at a glance

| Capability | Tier | Manifest alone? |
|---|---|---|
| "string but you passed int" diagnostic | 1 | ✅ |
| Unknown-verb / wrong-arity diagnostic | 1 | ✅ |
| Presentation/effect tagging, fallback policy | 1 | ✅ |
| Enum / pattern / range param validation | 2 (closed) | ✅ |
| Color-picker on a `color` param | 2 + studio widget | ✅ (widget studio-side) |
| Auto-complete live game-DB items | 3 (value provider) | ❌ needs host |
| "this item_id exists in my project" | 3 (value provider) | ❌ needs host |
| Visual map-point / path picker writing (x,y) | 3 (host editor) | ❌ needs host |

## Runtime relationship (Track A)

**The runtime never sees the manifest** (see "Why this exists"). Track A's
bind-by-name + `Value` boundary stays untouched. The presentation/effect tag is
informational tooling metadata (studio can group/label "effect" verbs); routing
those to an authoritative reducer is the *host's* binding implementation, not a
VM concern. The nuanced fallback ("use the ink body if it exists, else null") is
a runtime-intrinsic refinement on `Program` metadata, independent of the
manifest — track it separately if/when wanted.

## Resolved (this design pass)

- **Scope: tooling/author-time only** — analyzer + studio; never runtime or
  compiler codegen.
- **Manifest ↔ ink `EXTERNAL`:** additive enrichment; the author always
  declares `EXTERNAL` in ink, the manifest/inline tags add types/semantics/docs.
- **Two sources** (inline `///` JSDoc + registered), merged; **inline wins**.
- **Syntax:** JSDoc-style `///` + `@param`/`@returns`/`@kind`/`@widget` tags.
- **Integration:** `HostManifest` in `brink-ir`; registered via
  `EditorSession::set_host_manifest`; merged in `brink_analyzer::analyze`;
  surfaced by existing `brink-ide` queries.

## Implementation status (MVP landed)

The **Tier 1 + closed Tier 2 MVP** is implemented. What shipped:

- **Schema** (`brink-ir`, `src/host_manifest.rs`): `HostManifest`,
  `ManifestExternal`, `ManifestParam`, `SemanticTypeDef`, `Constraint`
  (`enum`/`regex`/`range`), `TypeRef`, `BaseType`, `ExternalKind` — serde types.
  Plus inline `ExternalDoc` (the `///` counterpart).
- **Inline `///` parsing** (`brink-ir` HIR lowering, `doc_comment.rs`): walks
  the `EXTERNAL_DECL` leading trivia, parses `@param`/`@returns`/`@kind`
  (`@widget` reserved, ignored); stored in a parallel `external_docs` map on the
  per-file `SymbolManifest`. Malformed tags → E038 (warning).
- **Merge + diagnostics** (`brink-analyzer`, `external_check.rs`): inline +
  registered merged into `AnalysisResult.external_meta` (keyed by
  `DefinitionId`; **inline wins**). Diagnostics: E039 manifest↔ink arity
  disagreement, E040 unknown semantic type, E041 call-site literal type
  mismatch, E042 closed-domain (enum/range) violation. Literals-only — no false
  positives on dynamic values.
- **Severity flag** (`ExternalCheckSeverity { Error (default), Off }`) plumbed
  through `analyze_with_options`, `IdeSession::set_external_check`,
  `EditorSession::set_external_check`, and `brink-compiler::compile_with_options`
  (the "compiler flag"). `Off` suppresses diagnostics but still builds
  enrichment.
- **Surfacing**: hover + signature help (`brink-ide`) and completion detail
  (`brink-web`) show typed params / return / kind / doc. `compile_project`
  carries the registered manifest so diagnostics appear in compile output.
- **Registration**: `EditorSession::set_host_manifest(json)` /
  `clear_host_manifest` / `set_external_check`; TS schema mirror in
  `@brink/wasm-types`; handle methods in `@brink-lang/web`.
- **Tier 3 — static slice landed (#174):** `SemanticTypeDef.values:
  ValueSource` (`{source:"static", items:[{value,label,detail?}]}` |
  `{source:"host"}`) — a *separate* field from `constraint`, **advisory** (never
  a diagnostic). The analyzer carries it on `ResolvedType.values`; `brink-ide`
  inlay hints render a **value label** after a literal whose param has a static
  value set (`set_switch(5 ⟨HarborGate⟩, …)`, `InlayHintKind::Value`). The
  completion dropdown offers the values too (`argument_value_completions` +
  `CompletionItem.insert`). See [host-argument-picker-spec.md](host-argument-picker-spec.md).
- **Tier 3 — dynamic host transport landed (#174):** the `host` value source is
  served from a **push-cache** — `EditorSession::set_host_values(json)` /
  `clear_host_values` take a per-type snapshot (`{ "<type>": [{value,label,detail?}] }`)
  the attached host pushes; it lives on `IdeSession` (query-time, no re-analyze)
  and the picker + value-label inlay hints resolve `host`-source types from it
  (empty ⇒ plain literal entry). Studio handle: `EditorSessionHandle.setHostValues`.
- **Tier 3 — studio `argumentProviders` surface landed (#175):**
  `StudioExtensions.argumentProviders?: { type, enumerate() }[]` — a data-only
  embedder API keyed by semantic type. At mount the studio enumerates them and
  pushes the snapshot into the session's value cache (`pushArgumentProviderValues`
  → `setHostValues`), so a host registers value sources declaratively rather than
  poking the session. **This completes the Phase-9 host-aware argument picker**
  (static + dynamic). Optional follow-up: live refresh (re-enumerate when host
  data changes).
- **Tier 3 — widgets & editors: designed, not yet landed.** The full UX is specced
  in [argument-widget-spec.md](argument-widget-spec.md), forks resolved (inline is
  studio-rendered data-only; the host editor is a studio-mounted component via the
  `argumentWidgets` surface; studio owns the popover/modal chrome; `surface` declared
  in-manifest). Schema delta to land: `SemanticTypeDef.widget?: { kind }`,
  `ManifestExternal.widgets?: [{ group, type, editor, surface?, context?, fallback? }]`,
  `ManifestExternal.path?: string[]` (panel categorization, #210). Built in 5 stages
  (registry + `color` first; `argument_widgets` query; the Form; host widgets;
  arg-groups + modal). Related: #210 (panel categories/search), #211 (`value-list`
  label-searchable typeahead).

**Resolved forks:** (1) metadata lives in side-tables (`SymbolManifest.
external_docs` per-file; `AnalysisResult.external_meta` merged) — `SymbolInfo`
stays lean; (2) the manifest is an explicit `analyze_with_options(files, opts)`
argument (non-breaking: `analyze(files)` delegates with defaults), not a
`ProjectDb` input.

**Deferred to follow-ups:**

- **insert-`EXTERNAL` code-action** — the `brink-ide` code-action query is
  currently source-only; making it manifest-aware is its own pass.
- **Regex constraint enforcement** — `Constraint::Regex` is stored and surfaced
  but not checked (no regex dependency at the MVP); enum + range are enforced.
- **`Warning` severity level** — the flag is `Error`/`Off` at the MVP (the
  shared `Diagnostic` type has no per-instance severity; a `Warning` middle
  ground would need one).
- **Reserved-keyword external names** — an external named with an ink operator
  keyword (e.g. `EXTERNAL has(...)` — `has` is the list operator) currently
  mis-parses; use non-keyword names. Tracked separately.

## Remaining design forks (Tier 3+)

1. **Tier-3 completions: push-cache vs. async provider** — *resolved:* push-cache
   (host pushes value sets on change; sync completions). Landed (#174/#175).
2. **Widget UX forks** — *resolved* in [argument-widget-spec.md](argument-widget-spec.md)
   §6 (inline data-only, studio chrome, mount-callback seam, manifest `surface`,
   Form glyph placement prototype-both, panel-as-launcher).
3. **Manifest generation** — generating registered entries from host source
   (e.g. an RMMZ plugin's commands) is a nice-to-have, not required.
4. **Categories taxonomy depth** — `path: string[]` is nested (decided); whether
   the panel renders arbitrary depth or caps it is a panel-impl detail (#210).

## Phasing

Tier 1 alone delivers real value (call-site validation) and is a shippable
file. Tier 2 (closed domains) layers on statically. Tier 3 is the host protocol
built when RMMZ studio embedding becomes real. Each tier is independently
useful; you do not need Tier 3 to benefit from Tier 1.
