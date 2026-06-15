# Argument widget spec

**Status:** Design (extends `docs/host-capability-manifest.md` §Tier-3 "live value providers & host
editors", and composes with the host-aware argument picker `docs/host-argument-picker-spec.md`).
The built-in `hex_color` color picker is the first, studio-rendered corner of this system; this spec
generalizes it into a **widget** model where both the in-text affordance *and* the editor can be
studio-built-in or host-provided, popover or modal, single-arg or arg-group.

> Trust note: author-time tooling only. A widget edits the literal text the author would type by
> hand; it never changes the compiled program, runtime, codegen, or the oracle. Advisory, like the
> rest of the host-capability manifest.

## 1. The model

An **argument widget** attaches to a semantic type or an argument group. It has up to two surfaces,
each with a **renderer source** (studio-builtin or host-provided):

| Surface | What it is | Renderer | Built-in example | Host example |
|---|---|---|---|---|
| **inline** | the in-text affordance at the call site | **always studio-rendered** | a color swatch | a chip reading the map name + location |
| **editor** | opened on invoke; returns the new value(s) | studio **or** host | a popover color picker | a modal map editor |

- **Inline is always studio-rendered** — the studio draws the swatch/chip; a host never mounts UI into
  the source text. A host contributes inline only as *data*: a label (`inline(ctx) → { text, className? }`,
  e.g. "Harbor District @ 12,8") plus an optional CSS class on the chip span for styling. No
  host-mounted DOM in-text, no thumbnails. (A future `icon?` SVG-string prop is possible but not in
  scope today.) The heavy host UI lives in the **editor**, not the line.
- **Editor surface** is a **popover** (light, anchored to the call site — the color case) or a
  **modal** (full-screen-ish overlay — the spatial/map case).
- A widget may span an **argument group** (`place_object(x, y)` → one `map_point` widget over
  `[0, 1]`); the editor returns multiple values and the studio writes them back as a multi-slot edit.
- A widget may take **inter-argument context** (`teleport(actor, mapId, x, y)` → the picker over
  `[2, 3]` opens map = arg `1`); the studio resolves the sibling literal and hands it to the editor.

The **studio owns the chrome** (inline decoration placement, popover/modal container, positioning,
focus-trap, esc/click-outside to cancel) — reusing the shell **overlay primitive** (studio-shell-spec
§7.7). A host fills the *body* and signals back through a resolve/cancel host object. This keeps
accessibility + consistency studio-side and lets a host focus on its domain UI.

### 1.1 Three entry points into one editor

The per-parameter widget editor is the **atom**. Authoring should not require typing literals like a
programmer — so the same editor is reachable three ways:

| Entry point | Trigger | Editor anchored on | Write |
|---|---|---|---|
| **Edit** | click the inline affordance on an existing literal | the literal | **replace** the literal |
| **Fill** | click a ghost placeholder at an **empty** arg slot | the slot position | **insert** a literal at the slot |
| **Form** | click the call-level affordance (whole call) | the whole argument list | **write/normalize all args** |

- **Edit** is the inline picker (the `hex_color` swatch). **Fill** handles "you inserted a call and
  haven't typed anything yet" (`set_tint(‹color›)` — a dimmed, clickable placeholder per missing arg).
- **Form** is a studio popover/modal with **one field per parameter**, each field driven by that
  param's widget editor (color picker, value dropdown, number field, …) or a plain text input
  (fallback for params with no widget). Submitting writes the entire call. This is the middle ground
  for complex calls — fill a form instead of hand-writing the arg list.
- A host can register a **whole-call widget** (a single editor spanning all params) that *replaces*
  the auto-composed form — the host's **multi-argument completion** UI (e.g. one dialog that picks an
  event, a position, and a duration together). Same `resolve(values)` contract, just N values.

All three reuse the same registry (§3) and the same multi-slot write-back (§5). The form is just the
per-param editors composed; Fill is the editor at an empty slot; Edit is the editor on a literal.

**Fill and Form are distinct and both kept** — they serve different purposes (quick single-arg fill
vs. compose-the-whole-call) and each behaves the same way every time. We do *not* try to be clever
about merging them. The one place they look merged is an **arg-group widget**: because the widget
already spans several params as one editor (`place_object(x, y)` → one `map_point`), that group is a
single Fill target *and* a single field in the form. That collapse is the widget's doing, not a Fill/
Form special case — a multi-field widget just presents as one interaction wherever it appears.

### 1.2 The Host Functions panel is a Form launch point

The Form has two entry points: the in-editor call-level affordance (§6.5) **and the Host Functions
panel**. Today the panel inserts a bare skeleton (`~ show_picture(name, x, y)` — param names as
placeholder text) at the cursor. With the Form, clicking a panel function instead **opens the Form
pre-targeted at that external**, the author fills it, and the studio inserts the *completed* call
(`~ show_picture("hero", 100, 200)`) at the cursor. This is what makes the panel pull its weight —
it goes from "paste a skeleton I then have to fix up" to "compose a real call by clicking." Same Form
UI, same write path; the only difference is the insert position (cursor) vs. an existing call's spans.
A skeleton quick-insert can stay available (e.g. modifier-click) for power users who prefer to type.

## 2. Manifest declaration

Extends the schema (Rust `brink_ir::host_manifest`, TS `@brink/wasm-types`). A `widget` is **separate
from `values`** (§host-argument-picker-spec): `values` is a *list of pickable values* (a dropdown);
`widget` is a *rendered affordance + editor*. A type may carry either, both, or neither.

```jsonc
// Type-level built-in widget (studio renders it):
{ "name": "hex_color", "base": "string", "widget": { "kind": "color" } }

// Param / arg-group widget on an external (host editor by id):
{ "name": "place_object",
  "params": [["x", "int"], ["y", "int"]],
  "widgets": [
    { "group": [0, 1],            // arg indices the widget spans
      "type": "map_point",        // semantic type / widget id
      "editor": "host.acme.map_picker",   // host editor id, or a built-in kind
      "surface": "modal",         // "popover" (default) | "modal"
      "context": { "map": 1 } }   // inter-arg context: sibling arg → key
  ] }
```

- `widget.kind` (on a `SemanticTypeDef`) names a **built-in** widget (`color`, later `number`,
  `vector3`, …). `editor` (on an external's `widgets`) names a **host** editor id (namespaced
  `host.<vendor>.<name>`) or a built-in kind.
- `surface` is the editor's preferred container; the studio defaults sensibly per built-in.
- `group` + `context` express arg-groups and inter-arg context (both already sketched in the manifest
  spec §Tier-3).
- A `fallback` (e.g. `{ "kind": "number" }`) renders when a host editor is declared but no host is
  attached — so the affordance degrades to plain studio editing, not a dead button.

The analyzer carries the widget declaration on its resolved metadata (alongside `values`), so the IDE
query (§4) can surface it at a call site.

## 3. The studio side — registry + extension surface

A **widget registry** keyed by widget id (built-in kind or host editor id):

- **Built-ins** register at studio startup. `hex_color`'s `color` widget is the first — and is built
  **through this same interface**, so built-in and host widgets share one code path (dogfood).
- **Host widgets** come from a new mount-time extension surface, consistent with host tool windows
  (which are already host-bundled UI):

```ts
/** Context handed to a widget's renderers (one entry per group member). */
interface ArgumentWidgetContext {
  type: TypeRef;                       // the widget's semantic type / id
  external: string;                    // the EXTERNAL being called
  paramNames: string[];                // param name(s) in the group
  values: string[];                    // current literal value(s), parsed (quotes stripped)
  context?: Record<string, string>;    // resolved inter-arg context, e.g. { map: "5" }
}

/** The studio-provided handle a host editor resolves/cancels through. */
interface WidgetEditorHost {
  resolve(values: string[]): void;     // new literal value(s) for the group → multi-slot edit
  cancel(): void;
}

interface ArgumentWidget {
  /** Semantic type / widget id this renders for. Host ids: host.<vendor>.<name>. */
  type: string;
  /** Optional inline label DATA — NOT a renderer. The studio draws the chip;
   *  the host returns the text (e.g. "Harbor District @ 12,8") and may attach a
   *  CSS class to the chip span. No host-mounted DOM in the source text. */
  inline?(ctx: ArgumentWidgetContext): { text: string; className?: string };
  /** The editor — the only host-rendered surface. The studio supplies the
   *  popover/modal chrome; mount the body into `container` and resolve/cancel
   *  through `host`. Return a teardown. */
  editor: {
    surface?: "popover" | "modal";
    render(ctx: ArgumentWidgetContext, host: WidgetEditorHost, container: HTMLElement): () => void;
  };
}

interface StudioExtensions {
  // …toolWindows / commands / statusBarItems / argumentProviders…
  argumentWidgets?: ArgumentWidget[];
}
```

**Inline is data-only; the editor is the mount-callback seam.** The host never mounts DOM into the
source line — `inline(ctx)` returns a label + optional CSS class and the studio renders the chip,
bounding what a host can do in-text to styling (no insane shenanigans). The host's real UI lives in
`editor.render(ctx, host, container)`: it receives a DOM `container` and returns a teardown, so the
host mounts whatever it wants (its own React tree, canvas, etc.) without the studio forcing a shared
React instance across the seam. The studio's *own* built-ins implement the same interface (the `color`
swatch is a studio `inline` + a studio `editor`).

## 4. The IDE query (brink-ide / brink-web)

Generalize `color::color_hints` → `argument_widgets(root, analysis, range)`. It reports **whole calls**
(so the studio can offer the call-level form, §1.1) with per-slot state — not just literal-bearing args:

```rust
struct CallWidgetSite {
    callee: String,
    name_span: (u32, u32),          // function-name span — anchors the call-level form affordance
    slots: Vec<SlotWidget>,         // one per declared parameter (UTF-16 at the wasm boundary)
}

struct SlotWidget {
    param_name: String,
    widget: Option<String>,         // widget kind / editor id from the manifest (None → text fallback)
    type_name: Option<String>,
    surface: Option<String>,        // "popover" | "modal"
    group: Vec<u32>,                // arg indices this widget spans (single = [i])
    state: SlotState,
    context: Vec<(String, String)>, // resolved inter-arg context literals
}

enum SlotState {
    Filled { spans: Vec<(u32, u32)>, values: Vec<String> }, // a literal → Edit (replace)
    Empty  { insert_at: u32 },                              // no value → Fill (insert)
    Expr,                                                    // a non-literal expression → leave alone
}
```

Built on the existing call-site → semantic-type join point (`find_call_context` + `symbol_meta`),
plus the manifest's arg-group/`context` declarations. The studio's CM extension consumes these: it
renders the inline affordance on `Filled` slots, a ghost placeholder on `Empty` slots, the call-level
form affordance at `name_span`, and on invoke opens the editor / composes the form.

## 5. Invocation protocol (the data flow)

1. **Resolve** — `argument_widgets(range)` yields the sites (spans, widget id, values, context).
2. **Inline** — the studio always draws the in-text affordance (a CM widget decoration): a built-in
   renderer (the `color` swatch from the hex value) or a chip from the widget's `inline(ctx)` label
   data, with the host's optional CSS class applied to the chip span. Decorations are computed for the
   viewport range only (standard CM cost-bounding); no host DOM is mounted in-text.
3. **Invoke** — clicking the affordance opens the editor in the studio's chrome (popover anchored at
   the span, or modal). `editor.render(ctx, host, container)` mounts the body.
4. **Resolve / cancel** — the host (or built-in) calls `host.resolve(values)` or `host.cancel()`.
5. **Apply** — on resolve, the studio applies a **multi-slot text edit** replacing the group's
   `spans` with the new literals (re-quoting per base type), as one undoable transaction.

## 6. Forks — resolved

1. **Host-customizable *inline* rendering — DECIDED: data-only, no host DOM.** Inline stays
   studio-rendered. A host contributes a **label** (`inline(ctx) → { text, className? }`, e.g. the map
   name + location) and may attach a **CSS class** to the chip span; nothing more. No host-mounted
   in-text DOM, no thumbnails. (A future `icon?` SVG-string prop is conceivable but explicitly *not*
   today.) Heavy host visuals belong in the editor (§2 fork). This is the conservative branch — it
   keeps the source line studio-owned and styling-bounded.
2. **Chrome ownership — DECIDED: studio owns the chrome, host fills the body.** Uniform positioning,
   focus management, esc/click-outside, consistent apply/cancel; the host writes only its domain UI.
3. **Seam shape — DECIDED: mount-callback (`(ctx, host, container) => teardown`)** for the editor, so
   the host isn't tied to the studio's React instance. (Inline is data-only per fork 1.)
4. **Surface selection — DECIDED: declared in the manifest** (`surface: "popover" | "modal"`); the
   studio defaults per built-in (color → popover). A host editor may request modal via the manifest.
5. **Call-form affordance placement — DECIDED: prototype both, choose by feel.** Build (a) a
   hover-revealed glyph just after the function name (`set_tint⊞("#FF8800")`) and (b) an
   always-visible inline glyph, behind a toggle, so the user can compare in-editor and pick. Lean is
   hover, but the always-inline variant is cheap to ship alongside for the A/B.
6. **Auto-open on insert — DECIDED: no auto-pop in the editor; the Host Functions panel opens the
   Form.** Typing/completion insertion in the editor shows the Fill placeholders + form glyph, never
   an auto-modal. The *deliberate* Form launch is the Host Functions panel (§1.2) — clicking a
   function opens the Form and inserts the completed call. A **Settings panel** toggle governs whether
   editor insertion auto-opens the Form for authors who want it; default off.

## 7. Built-in widgets

The studio ships built-ins through the §3 interface:

- **`color`** (for `hex_color`): inline swatch; popover editor = a studio color picker (replacing the
  current OS-native `<input type=color>`, which feels heavy). This subsumes the current
  `hex_color` picker — re-expressed through the registry, and with the UX fixes (§9).
- **`value-list`** (for a type carrying `values`, §host-argument-picker-spec): inline chip = the
  resolved label (e.g. "Potion" for id `3`); editor = a **searchable typeahead popover** that filters
  on the *label*, not the inserted value, so a host with an integer-id set the author thinks of by
  name stays usable at hundreds of entries. Two surfaces, with a deliberate focus rule (#211):
  - **Typing path — never grabs focus.** Stays normal CM autocomplete; completion items *match on the
    label* but *apply the value*, so an author who knows the name types it and one who knows the id
    types that — no cursor theft mid-typing.
  - **Invoke path — focus is intentional.** Opening the picker is an explicit signal, by **mouse or
    keyboard**: click the chip / Fill placeholder, *or* press **↑/↓** while the cursor is in the slot.
    Either opens the popover with a focused search box + scrollable (virtualized) list. Arrow keys are
    a deliberate "let me browse" gesture (no mouse needed) and don't violate the focus rule — the
    author asked for the list; typing alone never opens it.
- Future: `number`/`slider`, `vector3`, `enum-chip`, `bool-toggle` — all studio-rendered, no host.

## 8. Graceful degradation

- No widget registered for a type → no affordance; plain literal editing.
- A host editor declared but **no host attached** → the manifest `fallback` built-in renders (e.g.
  number fields for `map_point`), so the affordance still works, just generically.
- A widget whose literal can't be parsed (a variable instead of a literal) → no inline affordance
  (nothing to anchor a value to); the author edits the expression directly.

## 9. The immediate UX fixes (carried by stage 1)

Independent of the host system, the built-in `color` widget should feel right:

- **Swatch adjacent to the literal**, not at the arg start: `set_tint(color: `▮`"#FF8800")` — the
  param-*name* inlay (`color:`) stays, the **type** (`hex_color`) is dropped (the swatch conveys it),
  and the swatch sits immediately before the value.
- **A light studio popover** color picker on click, not the OS-native dialog.

## 10. Staging (build order)

1. **Built-in widget registry + the `color` widget + the UX fixes (§9).** Studio-only. Establishes
   the registry + inline/editor interfaces + the studio popover/modal chrome (overlay primitive), and
   lands the `hex_color` polish through them. No host API, no manifest change beyond `widget.kind`.
2. **The IDE query generalization (§4)** — `argument_widgets` (whole-call + per-slot state), replacing
   `color_hints`; manifest `widget` on types; analyzer carries it. Unlocks **Fill** (ghost
   placeholders at empty slots) for built-in widgets.
3. **The call form (§1.1–1.2)** — studio-composed form (one field per param, each a built-in widget
   editor or text fallback) reachable from **both** the call-level glyph (prototype the hover and
   always-inline variants behind a toggle, §6.5) **and the Host Functions panel** (click a function →
   Form → insert the completed call at the cursor, §1.2). Plus the Settings toggle for editor
   auto-open (§6.6, default off). Studio-only; no host API yet. The "fill a form instead of typing"
   middle ground for built-in-typed calls, and the payoff that makes the panel pull its weight.
4. **Host widgets (§3 extension) + invocation protocol (§5)** — `argumentWidgets`: the host `inline`
   label hook (+ optional CSS class on the chip) and a popover `editor.render`, multi-slot edits. A
   host can now label an inline chip and provide a popover editor *and* a whole-call form widget
   (multi-arg completion).
5. **Arg-groups + inter-arg context + modal surface** — `widgets`/`group`/`context` manifest, the map
   editor case (host modal). The heaviest; lands last.

## 11. Out of scope

Author-time host vocabulary that isn't a widget (covered by `argumentProviders`, §host-argument-picker
-spec), runtime/inspector concerns (live-inspector-spec), and persisting widget state. Validating a
widget's output against domain rules (e.g. walkable tiles) is the host editor's job, not the studio's.

## 12. Relationship to existing pieces

- **`argumentProviders`** (#175) — data-only value *lists* (a completion dropdown). Orthogonal: a type
  can have a `values`/provider dropdown *and* a `widget`. (Usually one or the other.)
- **`values`** (#174) — the dropdown's source. Different concern from `widget`.
- **Overlay primitive** (studio-shell-spec §7.7) — the popover/modal chrome the editor mounts into.
- **The `hex_color` picker** (current) — becomes the `color` built-in, re-expressed through stage 1.
