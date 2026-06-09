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

All of these are served by one declarative artifact: a **manifest describing
the host's external vocabulary**, consumed three ways — by the **analyzer/IDE**
(author-time), by the **runtime binding registry** (run-time, optional), and by
**brink-studio** (tooling). This is net-new beyond ink (Inkle's
`BindExternalFunction` is bind-by-name with no schema).

It is, in effect, a **serializable type-checking + editor-affordance schema for
the host boundary** — scoped to ink external call sites, not a general type
system for ink.

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

Track A needs to preserve **nothing special** for the manifest: bind-by-name +
`Value` as the boundary type (both already true) is all the manifest assumes.
The manifest's runtime-relevant bits — arg validation, fallback policy,
presentation/effect routing — are all layerable later without changing
`bind_external`. So the binding plumbing and the manifest proceed in parallel.

The one genuine runtime seam the manifest may eventually feed: the
presentation/effect tag and var↔host mappings, if the runtime registry consumes
them (which effects to execute server-side; how to sync RMMZ vars). Still
additive: ship binding without tags, add tag-awareness later.

## Open design forks (for when Track B is picked up)

1. **Push-cache vs. async provider.** `EditorSession.completions()` is sync
   today. Live game-DB completions either (a) have the host **push** value sets
   into the session on change (keeps completions sync; fine for a not-huge game
   DB) or (b) make completions **async** so the session calls out mid-query
   (more general, more plumbing). Lean (a).
2. **Manifest authoring vs. generation.** The static tiers are hand-authored by
   whoever writes the host↔ink plugin. Generating the manifest from host source
   (e.g. an RMMZ plugin's declared commands) is a nice-to-have, not required.
3. **Where the manifest type lives.** Likely a small shared crate/schema so the
   analyzer (Rust) and the host adapters (JS) agree on the format; the static
   tiers are plain serializable data.

## Phasing

Tier 1 alone delivers real value (call-site validation) and is a shippable
file. Tier 2 (closed domains) layers on statically. Tier 3 is the host protocol
built when RMMZ studio embedding becomes real. Each tier is independently
useful; you do not need Tier 3 to benefit from Tier 1.
