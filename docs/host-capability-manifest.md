# Host Capability Manifest (design — Track B)

**Status:** design-stage, deferred. This is the head of the "tooling /
analyzer extensibility" track (Track B). It is **not** a prerequisite for the
external-function binding foundation (Track A — runtime/web binding, save/load
persistence, name-based variable access, seeding). The manifest is additive
over a bind-by-name + `Value` boundary, so it can attach later without a
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
    { "name": "has",    "params": [["item","string"]], "returns": "bool", "kind": "query" },
    { "name": "camera", "params": [["target","string"]], "returns": "void", "kind": "presentation" },
    { "name": "grant",  "params": [["item","string"]], "returns": "void", "kind": "effect" }
] }
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

#### Widget classes

- **Studio-builtin widgets** — generic, studio renders them, no host assets:
  color picker (`color`), number fields (`vector3`), searchable dropdown
  (backed by a Tier-3 value provider, for `item_id`).
- **Host-provided editors** — studio *cannot* render these; the host owns the
  UI and assets. Studio's role is **broker + seam**.

#### Arg-group semantic types (one widget, many params)

A widget may span an **argument group**, not just a single param:

```jsonc
{ "name": "place_object",
  "params": [["x","int"], ["y","int"]],
  "widgets": [ { "group": [0,1], "type": "map_point", "editor": "rmmz.map_picker" } ] }
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

The data flow for a host-rendered editor (reuses inlay/code-lens affordances +
text-edit returns that `brink-ide` already has, cf. `convert_element` /
`rename`):

1. Manifest declares an arg-group widget with `editor: "rmmz.map_picker"`.
2. Studio renders an affordance at the call site (code-lens / inlay).
3. On invoke, studio emits an **edit request** to the host:
   `{ editor, call_site, current: {x,y}, context: {mapId} }`.
4. The host opens *its* editor, the user acts, the host returns a **structured
   result** (`{x,y}`) — or cancels.
5. Studio applies a **multi-arg text edit**, writing the slots back to source.

The only net-new infrastructure is steps 3–4: a wasm↔host
**request-host-UI / return-structured-value** callback — the same seam as the
value provider, "host renders an editor" instead of "host answers a query."

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

## Open design forks (remaining)

1. **`SymbolInfo` enrichment in place vs. a parallel `host_meta` side-table** —
   widen `SymbolInfo`/`ParamInfo` with optional types/doc, or keep a separate
   map keyed by external name. (Implementation detail; decide in the plan.)
2. **`analyze()` parameter vs. `ProjectDb` input** — does the manifest ride in
   the db inputs like files, or pass as an explicit `analyze(files, manifest)`
   argument? (Implementation detail.)
3. **Tier-3 completions: push-cache vs. async provider** — host pushes value
   sets into the session on change (sync completions; lean this) vs. the session
   calls out mid-query. Only relevant once Tier 3 is built.
4. **Manifest generation** — generating registered entries from host source
   (e.g. an RMMZ plugin's commands) is a nice-to-have, not required.

## MVP

Inline `///` tags + registered semantic-type vocabulary → **Tier 1 + closed
Tier 2**: type-mismatch diagnostics, enum/pattern/range validation, richer
completion/hover/signature, and the insert-`EXTERNAL` code-action — all on the
existing analysis pipeline. Tier 3 (live providers + host-rendered widgets) is
the later, host-protocol phase.

## Phasing

Tier 1 alone delivers real value (call-site validation) and is a shippable
file. Tier 2 (closed domains) layers on statically. Tier 3 is the host protocol
built when RMMZ studio embedding becomes real. Each tier is independently
useful; you do not need Tier 3 to benefit from Tier 1.
