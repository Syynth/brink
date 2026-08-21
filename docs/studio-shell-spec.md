# Studio Shell Spec — brink-studio as an ink IDE

Status: **draft for review** (2026-06-10). No implementation has started; this spec is the
design artifact required before any shell code lands (see decision log: "Studio shell
redesign direction").

## 1. Purpose

brink-studio's current UI is three hardcoded panes (binder | editor | player) with an
activity bar that swaps the left pane between Binder / State View / Program Explorer
([App.tsx](../packages/studio-ui/src/App.tsx)). Every new surface gets shoehorned into
that arrangement because there is no layout contract and no command system. This spec
defines the **shell**: the named regions of the window, the rules for what lives where,
the command infrastructure everything routes through, and the migration path from the
current code.

### References (who does the hard thinking)

| Concern | Reference | What we take |
|---|---|---|
| Structure | **VS Code** ([UX guidelines](https://code.visualstudio.com/api/ux-guidelines/overview)) | The region model: a small fixed set of named regions; *views* are the movable unit; commands are the universal verb. Which content belongs in which region. |
| Docking affordance | **JetBrains IDEs** (tool windows) | Icon strips on the window edges, each strip with two sections; click to toggle, drag an icon to another strip section to re-dock. Tool windows live **only** in edge docks; the editor is always center. |
| Visual language | **Zed** | Quiet, low-chrome: minimal borders, restrained color, generous text rendering. |
| Domain workflow | **Inky** | The editor⇄player two-up with live recompile is the core ink loop and is the **default layout**. |

Explicitly rejected: free-form docking (dockview / golden-layout style drag-any-tab-anywhere).
It has no concept of "edge" or "editor center", so the discipline this spec exists to create
would have to be rebuilt on top of it, and layouts degrade.

## 2. Vocabulary

- **Region** — a fixed, named area of the shell. Regions never move or nest. The full set:
  left dock, right dock, bottom dock, editor area, status bar, dock strips, command palette
  (overlay).
- **Dock** — an edge region that hosts tool windows. Each dock has two **sections**
  (`start`/`end` — visually top/bottom for the side docks, left/right for the bottom dock).
  Each section shows at most one tool window at a time; a section with multiple assigned
  tool windows tabs between them via its strip icons.
- **Strip** — the narrow icon bar along a dock's edge. Shows one icon per tool window
  assigned to that dock; click toggles, drag re-docks. Strips are always visible (even when
  their dock is collapsed) unless the dock has no tool windows at all.
- **Tool window** — a registered, dockable view (Binder, State View, Problems, …). Tool windows
  can live in any dock section, can be toggled, and remember size/placement. They can never
  enter the editor area.
- **Editor area** — the center region. Hosts **editor groups** (tabbed documents,
  splittable). Documents are files or read-only document views (e.g. the `.inkt` dump).
  Editors can never enter a dock.
- **Command** — a named action (`story.restart`, `view.toggle.problems`). Keybindings,
  palette entries, strip clicks, menu items, and buttons all dispatch commands; nothing
  binds a key directly to a function.
- **Story session** — one live story/VM instance: compiled program + runner handle +
  transcript + debug state + status. Some surfaces are **session-bound** (§7.6): they
  render against the current session and show a placeholder when none exists.

## 3. Region layout

```
┌─────────────────────────────────────────────────────────────┐
│ ┌─┐ ┌─────────────────────────────────────────────────┐ ┌─┐ │
│ │s│ │ left dock │   editor area (groups)  │ right dock│ │s│ │
│ │t│ │  start    │  ┌─tabs────┐┌─tabs────┐ │   start   │ │t│ │
│ │r│ │           │  │         ││         │ │  (State)  │ │r│ │
│ │i│ ├───────────┤  │  ink    ││  player │ ├───────────┤ │i│ │
│ │p│ │ left dock │  │  editor ││  doc    │ │ right dock│ │p│ │
│ │ │ │  end      │  └─────────┘└─────────┘ │   end     │ │ │ │
│ └─┘ └─────────────────────────────────────────────────┘ └─┘ │
│      ┌────────────────────────────────────────────────┐     │
│      │ bottom dock (start | end)   [strip along edge] │     │
│      └────────────────────────────────────────────────┘     │
│ status bar                                                  │
└─────────────────────────────────────────────────────────────┘
```

Rules:

1. **The editor area always exists and is always center.** It cannot be collapsed
   (maximize modes are temporary presentation over an unchanged layout — see §5.4).
2. **Docks collapse to their strip.** A dock with no open tool window takes zero width
   beyond its strip.
3. **Sizes are per-dock, persisted.** Resizing a dock with the splitter is remembered;
   reopening a tool window restores its dock's last size.
4. **Tool windows are exclusive per section.** Opening one in a section closes/backgrounds
   the previous occupant (it stays in the strip).
5. **Drag-to-re-dock** moves a tool window's strip icon between any dock section. Layout
   (assignments + sizes + which windows are open) persists to `localStorage`, versioned.

## 4. Tool window & document inventory

Default placement, mapped from existing components:

| Surface | Kind | Default home | Today | Notes |
|---|---|---|---|---|
| **Binder** | tool window | left dock, start | [Binder.tsx](../packages/studio-ui/src/Binder.tsx) | Open by default. Component gets decomposed (tree / selection / DnD / context menu) during migration, not before. Below the project's own file tree, a collapsed-by-default "Library" section (issue #2306, ruled 2026-08-06 "Mounted stdlib presents as a read-only library node") lists mounted `std/` files — browsable (folder tree, click/double-click opens read-only) but with no drag/rename/delete/new-file affordances, excluded from save-all and search/replace. `list_files`/`project_outline`/`story_graph` list mounted files flagged `mounted: true` rather than excluding them (#2231's original "hide" default). |
| **Player** | editor document (session-bound) | editor area, right split by default | the `player` document type ([PlayerPane.tsx](../packages/studio-ui/src/PlayerPane.tsx)) | Landed (#120): a singleton session document (§7.6 — placeholder + Start when no session), opened via `story.openPlayer` and at bootstrap in a right split (the Inky two-up: editor left, player right, focus on the editor). Reopening focuses the existing tab; only an explicit split duplicates the view (two store subscribers over one session). **Reopening a closed player restores the split (#280):** if the player is not open *and* the editor area has collapsed to exactly one group that still holds other content, `story.openPlayer` calls the same bootstrap helper (`openPlayerSplit`) instead of dropping the tab into the focused group — restoring the Inky two-up and handing focus back to the editor. An empty single group (nothing opened yet) or a layout already split beyond one group falls through unchanged to the reveal/open-in-focused-group policy. The component owns its Run/Restart/Maximize header (§7.8); maximize is `editor.maximizeGroup` (§5.4). The old tool window is gone; multi-session maps to player tabs. |
| **State View** (debugger) | tool window | right dock, start | [StateView.tsx](../packages/studio-ui/src/StateView.tsx) | Closed by default; opens when a story is running and the user toggles it. Took the right strip's start slot when the Player left the dock (#120). |
| **Problems** | tool window | bottom dock, start | *new* (data exists in `CompileSlice` diagnostics) | Clickable diagnostics list → `editor.reveal` (§6.1). Status-bar error/warning segment opens it. |
| **Output / compile log** | tool window | bottom dock, end | *new* | Compile timings, wasm/runtime errors that aren't source diagnostics. Replaces nothing; today this information is dropped. |
| **Search** | tool window | left dock, start | the `search` tool window ([SearchView.tsx](../packages/studio-ui/src/SearchView.tsx)) | Landed (#94): project-wide find/replace over the wasm session's live file sources (pure TS string search, UTF-16 offsets — `search-engine.ts` in `studio-store`). Shares left/start with the Binder (the strip tabs between them); closed by default; registered last so Binder keeps `Mod-1` (Search gets the generated `Mod-6`). `search.focus` (palette "Search: Find in Files", `Mod-Shift-F`) opens without ever closing (`ensureToolWindowOpen`) and focuses the query input. Live search debounced 200ms; case / whole-word / regex toggles (invalid regex shows an inline error, like the Settings JSON validation); results capped at 1,000 matches with a notice (unbounded-growth guard). Rows are grouped by file (collapsible headers with counts) and dispatch `editor.reveal` with source Locations (§6.1); Up/Down + Enter walk the rows. Replace is toggle-disclosed (VS Code style): per-match buttons plus a Replace All gated by an inline confirmation showing match/file counts; edits reuse the binder structural-op path (`updateFile` + `invalidateFile` + `triggerCompile`, all-or-nothing on stale results) so open views refresh and the project recompiles. Search state is transient — query/options/results reset per session; only the window placement persists like any other. |
| **Program Explorer** | tool window | bottom dock or right dock (user-movable) | [ProgramView.tsx](../packages/studio-ui/src/ProgramView.tsx) | Resolved (§10.1): the structured tables (globals/lists/externals/knot tree) stay a tool window; the raw `.inkt` dump toggle left it in #91 (a toolbar button opens Compiled Output instead). |
| **Compiled Output** (`.inkt` dump) | editor document (read-only) | editor area | the `compiled-output` document type ([CompiledOutputDocument.tsx](../packages/studio-ui/src/CompiledOutputDocument.tsx)) | Landed (#91): a read-only singleton document tab with a minimal CM6 `.inkt` mode (search, folding, selection), opened via `program.openCompiledOutput`. Compile-bound (§7.8): renders `programInkt` directly — no wasm document handle. Disassembly-view precedent. |
| **Story transcript** | tool window | bottom dock | *future* | Append-only transcript view; listed to validate the model, not scheduled. |
| **Story Graph** | editor document (custom-rendered) | editor area | the `story-graph` document type ([StoryGraphDocument.tsx](../packages/studio-ui/src/StoryGraphDocument.tsx)) | Landed (#97): visual story-structure explorer — a read-only pan/zoom react-flow canvas over the wasm story-graph query, opened as a singleton tab via `story.openGraph`. Compile-bound graph + session-bound live overlay — see §4.1. |
| **Settings** | editor document | editor area | the `settings` document type ([SettingsDocument.tsx](../packages/studio-ui/src/SettingsDocument.tsx)) | Landed (#93): a singleton tab (`settings.open`, `Mod-,`) over shell services — theme picker (ThemeService §7.4, live + reflecting external switches), keymap-override JSON in a plain textarea validated strictly on Apply through the shell's `KeymapOverridesService` (live rebuild, no reload; invalid JSON shows an inline error and saves nothing), and the one real diagnostic severity flag — external-function checking (`"error"`/`"off"`, the wasm `set_external_check`) via the store action, persisted under `brink-studio.diagnostics.v1` and restored at bootstrap before the first compile. Not session- or compile-bound. VS Code precedent (settings as a document tab, not a modal). |
| Ink files | editor document | editor area | the `ink-file` document type ([InkFileDocument.tsx](../packages/studio-ui/src/InkFileDocument.tsx) + CM6) | Landed (#90): the shell renders per-group tab bars; one CM6 view per (document, group) over a wasm document handle (§7.8). |

The **Toast** system is replaced by the shell notification service (§7.5) in Phase 3;
until then the existing component mounts in the shell unchanged.

**Session-bound surfaces:** Player, State View, the Story Graph's live overlay, the
future Story transcript, and the status bar's story segment all render against the story
session (§7.6). Everything else is compile-bound (driven by the latest compile result)
or static.

### 4.1 Story Graph document

A pan/zoom graph of the story's structure, opened as an editor document. It is the
reason Phase 4's document support must be **component-based, not text-only**: Compiled
Output is a CM6 text document, the Story Graph is a custom-rendered one — both implement
the same document-type API.

- **Granularity:** one node per knot by default; expanding a knot reveals its stitches as
  a nested subgraph. Whole-project graph (the analyzer index is cross-file); per-file
  scoping is a possible later view option.
- **Edges:** plain diverts (solid), choice targets (aggregated from the weave up to the
  owning knot/stitch, distinct style), and tunnels/threads (dashed — control returns to
  the caller). `END`/`DONE` appear as pseudo-nodes. Function-call edges are excluded; a
  default-off toggle is a possible later addition.
- **Data:** landed (#96) — the **story-graph query** lives in
  `brink_ide::story_graph` (divert targets resolved through the analyzer's resolution
  map, the same machinery as goto-definition), exposed as `story_graph()` on the wasm
  `EditorSession` and `getStoryGraph()` on `EditorSessionHandle` (`StoryGraph` in
  `@brink/wasm-types`): nodes (id = qualified name, kind knot/stitch/end/done, file,
  UTF-16 name span, stitch parent id) and edges (from, to, kind
  divert/choice/tunnel/thread), recomputed per call like the outline. Ordering is
  deterministic (nodes sorted by id, edges deduplicated and sorted by from/to/kind —
  the HashMap-iteration rule). Returns `null` before the first analysis.
- **Rendering:** landed (#97) — react-flow (`@xyflow/react`) renders the canvas;
  `@dagrejs/dagre` computes a layered **top-down** layout (stories flow downward from
  their entry knot) as a pure function off the render path
  ([story-graph-layout.ts](../packages/studio-ui/src/story-graph-layout.ts)), memoized
  on structure only — overlay changes (current location, visit counts) restyle nodes
  without re-running layout. Expanded knots are laid out in two passes (dagre's
  compound support is unreliable): each expanded knot's stitches get their own dagre
  run to size the knot as a cluster, then the top level runs with cluster-sized knot
  nodes; stitch positions are parent-relative (react-flow subflow convention). The
  view-model mapping ([story-graph-model.ts](../packages/studio-ui/src/story-graph-model.ts))
  is pure and renderer-agnostic: stitches are visible only while their knot is
  expanded; edges into a collapsed knot's stitches remap up to the knot, duplicates
  fold into one edge with a ×N count, and remap-created self-loops are dropped
  (genuine self-diverts kept). Node count is capped-by-collapse (knots start
  collapsed) per the unbounded-growth guard. Edge styling by kind (divert solid
  muted, choice solid accent, tunnel/thread dashed, custom themable arrowheads) and
  the whole surface — nodes, edges, controls, legend, background — is skinned with
  the semantic `--bs-*` tokens (§7.4), covering both themes; react-flow ships only
  its structural stylesheet. A corner legend names the edge kinds. The graph lands in
  the compile slice (`storyGraph`), refreshed on each successful compile; a failed
  compile keeps the last good graph (like `programInkt`).
- **Interaction:** landed (#97) — pan/zoom (wheel, drag, zoom controls); click a node
  to navigate to its source (`editor.reveal`, §6.1; pseudo-nodes have no source);
  expand/collapse knots via the header chevron or double-click. **Live story
  overlay:** while a story runs, the current location node is highlighted and nodes
  show visit-count badges — consuming the same name-resolved `debugState` the State
  View consumes, session DATA only, never the runner handle (§7.6). A current
  location inside a collapsed knot highlights the knot (longest visible dot-prefix);
  with no session the graph renders plain. Read-only — nothing is draggable,
  connectable, or selectable; authoring actions from the graph (create knot,
  drag-to-divert) are out of scope and would be a separate spec.

## 5. Behavior

### 5.1 Strips

- One icon per tool window, clustered by section: start-section icons at the strip's
  start, end-section icons anchored at the strip's far end (JetBrains-style halves),
  registration order within each cluster.
- Click: toggle that tool window (open-in-section / collapse).
- Drag: ghost icon follows cursor; valid drop targets are the six sections; drop re-docks.
  Strips highlight the target section on hover. (Drag ships in Phase 3, not MVP — §9.)
- Icons show badges where meaningful (Problems: error count).

### 5.2 Keyboard

JetBrains-style numbered mnemonics, dispatched through the command registry:

- `Mod-1…9` — toggle tool windows by a stable, user-visible ordering (Binder `Mod-1`,
  State View `Mod-2`, …; generated from registration order, shown in strip tooltips).
- `Mod-Shift-P` — command palette. `Mod-P` — quick-open (binder files/knots/stitches).
- `Escape` from a tool window returns focus to the editor (JetBrains behavior).
- Editor-internal editing keys (Enter/Tab/element transitions in
  [keybindings.ts](../packages/ink-editor/src/keybindings.ts)) stay inside CodeMirror —
  they are modal text-editing behavior, not shell commands. The shell registry handles
  everything chrome-level.

### 5.3 Responsive tiers

The existing `wide / medium / narrow` tier system
([layout.ts](../packages/studio-store/src/slices/layout.ts), `useTier`) is kept, redefined
over the new model:

- **wide** — full shell as drawn in §3.
- **medium** — side docks become slide-over drawers (today's binder drawer generalized to
  both sides); bottom dock remains docked; strips remain visible.
- **narrow** — single-region presentation: editor, with tool windows as full overlays;
  Editor·Story segmented control becomes Editor + a tool-window switcher.

Tier changes never lose layout state; they change presentation only (the current
"editor panel never remounts" guarantee in App.tsx is preserved — tool windows and editor
mount once and relocate).

### 5.4 Maximize

Two maximize modes, both shell features (the retired player-specific
`playerFullscreen` generalized — maximize is never a feature of one view):

- **Tool-window maximize** (`view.maximize`, args: tool-window id): the tool
  window temporarily covers the whole shell frame (the editor unmounts).
  Unchanged by #120.
- **Editor-group maximize** (`editor.maximizeGroup`, args: optional group id,
  defaulting to the focused group; landed with #120): the group temporarily
  takes the entire editor area — sibling groups hide and the open docks
  collapse. The editor itself never unmounts. Available to any document; the
  Player's header Maximize button drives it, and the Story Graph is the
  expected second consumer.

Both are pure presentation: no open-state, dock sizes, or group sizes change,
so restoring (`Escape` or re-dispatching the command) brings the previous
layout back exactly. Neither has a default keybinding (palette-discoverable).

**Interplay rule:** the two modes are mutually exclusive. Dispatching either
command while the other mode is active restores the other first, so at most
one maximize is ever in effect. A maximized group that collapses (last tab
closed) restores automatically, and splitting while maximized restores first
(the new group must be visible) — this holds for every path that creates a
`"split-right"` group, not only the explicit `editor.split` command:
`splitGroup` itself unconditionally clears `maximizedGroupId`, so
`story.openPlayer`'s restore-the-split behavior (#280, §4 Player row) obeys
the same rule as `editor.split` without each caller having to remember it.

**The general invariant: focus must never land in a group `EditorArea` is
not rendering (#2787, #2797, #2826).** Because `EditorArea` renders only the
maximized group while one is active, any operation that can move
`focusedGroupId` to a different group must also restore — otherwise the
operation moves focus internally but paints nothing, and the trigger (a
Binder click, a tab drag) appears to do nothing. Two store actions carry this
responsibility, each computing its resolved target group first and then
clearing `maximizedGroupId` iff it is set and differs from that target,
leaving it untouched when the target is the maximized group itself (already
the only thing rendered, nothing to restore):

- **`openDocument`** — every target it can resolve to: revealing an
  already-open tab wherever it lives (#2797, the default `"focused"`
  behavior), a `"split-right"` group, an explicit `{ group }` id, and the
  new-tab fall-through into the focused group when the document isn't open
  anywhere yet (#2826). All four share one final clear keyed on the resolved
  `groupId`, so no target can be added later without inheriting the
  protection.
- **`moveTabToGroup`** (#2826, "Related, same invariant") — reachable both
  by tab drag-and-drop and via the `editor.moveTabRight` / `moveTabLeft`
  commands (`editor-commands.ts`), whose `when` clauses check only group
  index and tab count, not maximize state. A move that lands in `toGroupId`
  clears `maximizedGroupId` whenever it differs from that target, which
  also covers the source group collapsing away when it was the maximized
  one (a collapsing source can never equal the target, since
  `fromGroupId === toGroupId` is a no-op).

**Not yet covered: `editor.focusNextGroup`.** Its `when` clause
(`groups.length > 1`) stays true while a group is maximized, so the command
still runs and can park focus in a hidden sibling group with nothing in this
invariant clearing it. Whether the fix should be "clear `maximizedGroupId`
when it moves focus" (matching the two actions above) or "make `when` false
while maximized so the command isn't offered at all" is an open maintainer
call (#2826) — those two options differ in user-visible behavior, so neither
is implemented pending a ruling.

## 6. Command system

A `CommandRegistry` in the shell package:

```ts
interface Command {
  id: string;             // "view.toggle.problems", "story.restart"
  title: string;          // palette display: "View: Toggle Problems"
  keybinding?: string;    // "Mod-3" — single default binding, user remap is future work
  when?: (state: StudioState) => boolean;  // enablement, evaluated at dispatch & palette
  run(ctx: CommandContext): void | Promise<void>;
}
```

- Commands are registered at startup by each feature module (story commands at the app
  boundary, view-toggle commands generated from the tool-window registry).
- One global key handler resolves keybindings → command dispatch. No component-level
  `onKeyDown` for chrome behavior.
- **Keymap layer (required from Phase 1):** the key handler never reads
  `command.keybinding` directly — it resolves through a keymap table built from the
  registry's defaults, with a **user-override JSON** (localStorage
  `brink-studio.keymap.v1`) merged over the defaults. The shell-owned
  `KeymapOverridesService` (#93) wraps the persisted JSON with a change event;
  ShellProvider subscribes and rebuilds the table live, and the Settings document
  edits the JSON through it (plain textarea, strict validation on Apply).
  Resolved §10.3: the indirection is cheap now and expensive to retrofit, and
  with it in place the override merge costs almost nothing. A full
  keymap-editing UI stays out of scope for all phases.
- **Hamburger menu (resolved §10.2):** a single icon at the top of the left strip
  (JetBrains new-UI placement) opens a grouped menu *generated from the command
  registry* — no hand-maintained menu structure, embed-friendly, and the same registry
  feeds the native menu bar in the desktop shell ([desktop-shell-spec.md](desktop-shell-spec.md),
  v1 ruled 2026-08-06 — no longer merely "future"). There is no in-page menu bar.
- The **palette** is a shell overlay listing enabled commands, fuzzy-filtered, showing
  keybindings. **Quick-open** reuses the same overlay component with a different provider
  (binder items instead of commands).
- Buttons/menus/strip icons call `dispatch(commandId)` — never feature functions directly —
  so the palette is automatically complete.

### 6.1 Locations and navigation

Cross-surface linking ("click the thing, go to the thing") is one shared protocol, not
per-view behavior. The studio has four address spaces, and navigation is translation
between them:

```ts
type Location =
  | { kind: "source"; file: FileId; span: Span }          // what the editor understands
  | { kind: "symbol"; name: QualifiedName }               // knot / knot.stitch
  | { kind: "program"; address: ProgramPath }             // container path / bytecode addr
  | { kind: "session"; ref: SessionRef };                 // transcript entry, stack frame
```

- **Resolvers translate toward source.** A small resolver registry maps
  symbol → source (compile result), program → source (debug info), and
  session → program (runtime state). Views emit whatever space they naturally have;
  nobody duplicates translation. MVP implements the source and symbol resolvers;
  program/session resolvers land with their consumers (Compiled Output links, State View
  stack frames).
- **`editor.reveal(location)`** is the navigation verb: resolve to source, open the file,
  scroll, flash-highlight the span. Problems rows, graph nodes, quick-open hits, and
  State View frames all dispatch it. Routing through the command registry means host
  panels (§8) get navigation via `dispatch` with no new `StudioApi` surface.
- **Reverse reveal:** `view.reveal(viewId, item)` — "Reveal in Binder", "Reveal in
  Graph" from the editor cursor. The generic command is part of this concept; each view
  implements its receiver when the view lands (Binder Phase 2, Graph Phase 6).
- **Follow-selection is deferred.** Auto-syncing a view to the editor cursor (JetBrains'
  "link with editor" toggles) is explicitly not specced — navigation is always an
  explicit user action for now.

## 7. Shell infrastructure

### 7.1 View registry

```ts
interface ToolWindowDescriptor {
  id: string;                       // "binder", "state", "problems"
  title: string;
  icon: ReactNode;
  defaultPlacement: { dock: "left" | "right" | "bottom"; section: "start" | "end" };
  defaultOpen: boolean;
  badge?: ComponentType;            // strip badge, e.g. Problems error count (§5.1)
  component: ComponentType;
}
```

The badge is a *component*, not a value selector: the registering app supplies a
component that subscribes to its own store and renders the count bubble (or null).
This keeps badge counts reactive while the shell stays store-agnostic (§7.2) — a
`(state: StudioState) => number` selector would either couple the shell to the
studio store or go stale between strip re-renders.

Registered statically at startup. The shell renders strips/docks purely from the registry
plus a new `ShellLayoutSlice` (replaces today's `LayoutSlice` fields `activeSidebarView`,
`binderDrawerOpen`, `playerVisible`, `playerFullscreen`, `storyOpen`):

```ts
interface ShellLayoutSlice {
  tier: LayoutTier;                                  // kept
  placements: Record<ToolWindowId, Placement>;       // dock+section per tool window
  open: Record<DockSectionId, ToolWindowId | null>;  // current occupant per section
  dockSizes: Record<DockId, number>;
  maximized: ToolWindowId | null;
  // + actions: toggleToolWindow, moveToolWindow, setDockSize, maximize, …
}
```

Persisted (versioned key, e.g. `brink-studio.layout.v1`); unknown tool-window ids in a
stored layout are dropped silently on load.

### 7.2 Package layout

New package `packages/studio-shell`: regions, strips, docks, command registry, palette,
status bar host, notification service. It depends on `studio-store` (or hosts its own
slice composed into it) and is consumed by `studio-ui`. Feature components (Binder, Player,
StateView, …) stay in `studio-ui` and are *registered into* the shell — the shell never
imports feature components.

### 7.3 Status bar

Two segment groups, populated via a small `StatusBarItem` registry (same pattern as tool
windows: id, alignment, priority, component):

- **Left:** compile status (ok / compiling / N errors — click → Problems), story state
  (idle / running / awaiting choice).
- **Right:** cursor position, current element type + the element conversion dropdown
  (existing [StatusBar.tsx](../packages/studio-ui/src/StatusBar.tsx) /
  [ElementDropdown.tsx](../packages/studio-ui/src/ElementDropdown.tsx) content), key hints.

### 7.4 Theming

- Keep plain CSS + custom properties (no Tailwind/CSS-in-JS migration).
- **Semantic token layer** (landed with #92): components reference only semantic
  `--bs-*` tokens; theme files map a private palette onto them. The set is ~35 tokens
  in four groups: surfaces/chrome (`--bs-editor-bg`, `--bs-surface-bg`, `--bs-panel-bg`,
  `--bs-fg`, `--bs-fg-muted`, `--bs-border`, `--bs-accent`, `--bs-on-accent`,
  `--bs-hover-bg`, `--bs-list-active-bg`, `--bs-scrim`, `--bs-shadow`,
  `--bs-shadow-strong`), severity/status (`--bs-error`, `--bs-warning`, `--bs-success`,
  `--bs-info`), story symbols (`--bs-symbol-{file,knot,stitch,function}`), and syntax
  (`--bs-syn-*`, one per `tok-*` class). Alpha variants (hovers, selections, glows)
  derive in component CSS as `rgb(var(--bs-X-rgb) / N%)` over per-theme sRGB triplet
  tokens — not `color-mix()`, which Chromium 88 (RMMZ/NW.js) lacks (#276); themes also
  precompute the few opaque two-color mixes (`--bs-graph-*`, `--bs-conflict-banner-bg`,
  `--bs-active-line-bg`).
- **Theme mechanism:** themes are CSS files in
  [studio-shell/src/styles/themes/](../packages/studio-shell/src/styles/themes/) defining
  the tokens under a `[data-theme="<id>"]` scope on the `.brink-studio` root; Catppuccin
  Mocha (theme #1) doubles as the bare-class default/fallback, Catppuccin Latte is the
  light theme proving no component hardcodes a color. A `ThemeService` in studio-shell
  owns the registry (ids + labels), the current selection, and persistence
  (`brink-studio.theme.v1`, read synchronously before first paint); it generates
  palette-discoverable `theme.select.<id>` commands. Switching flips the root's
  `data-theme` attribute — runtime, no reload. The Settings document's theme picker
  (landed, #93) consumes `list()` / `current` / `select()` / `onDidChange()`.
- The studio.css monolith is gone (#92): shell-region styles live in
  `studio-shell/src/styles/`, feature styles in `studio-ui/src/styles/`, each package
  side-effect importing its own aggregator `index.css`.
- Visual direction per Zed: 1px hairline borders only where regions meet, no boxes-in-boxes,
  strip icons monochrome with an accent for the active state, density closer to a writing
  tool than to JetBrains.

### 7.5 Notification service

Replaces [Toast.tsx](../packages/studio-ui/src/Toast.tsx) (single message, fixed 5s
dismiss, ad-hoc undo callback) in Phase 3.

```ts
interface Notification {
  id: string;
  severity: "info" | "warning" | "error";
  message: string;
  source?: string;        // "binder", "compiler", "host.<vendor>" — shown subdued
  actions?: { label: string; commandId: string; args?: unknown }[];
  timeoutMs?: number;     // defaults by severity: info 5s, warning 8s, error sticky
}
```

- **API:** a shell `notify(n): NotificationHandle` service (handle supports
  dismiss/update). Callable from feature slices and from host extensions via the
  `StudioApi` facade (§8) — and, since #2528, from a `studio-ui` action module
  raising through the store's injected notifier directly
  (`performSymbolRename`). That is the first production producer that is neither
  a slice nor a host extension; it reports through the same store→shell bridge
  rather than a new channel.
- **Actions dispatch commands only** — no raw callbacks. This keeps the model
  serializable and consistent with §6 ("nothing binds a key directly to a function"
  applies to notification buttons too). The Binder's undo toast becomes a notification
  whose action dispatches `binder.undo`.
- **Presentation:** stacked bottom-right above the status bar, newest on top, max 3
  visible with an overflow "+N more" collapser. Hover pauses auto-dismiss. Styling per
  the Zed direction — quiet, hairline border, severity shown by an accent edge, not a
  filled background.
- **History:** a bell item in the status bar's right group with an unread badge; click
  opens a popover listing the session's notifications (cleared on demand). The history is
  capped (e.g. 100 entries, oldest dropped) per the unbounded-growth guard principle.
- **Refused structural operations report here.** A rename/move/delete that the
  underlying op declines raises an `error`-severity notification tagged with the
  same `source` as its success toast, so both outcomes of one operation report
  through one channel and a failure cannot be mistaken for a success.
  ⚠ This states the TARGET, not the current state of every call site. Known
  non-compliant path, pre-existing: the reorder/move/promote/demote ops in
  `dispatchSymbolAction`, which do not report refusals at all (#2544). A second
  known non-compliant path, also pre-existing: the code-actions/extract apply
  seam (`onApplyStructural` → `applyMoveResult`, `packages/brink-studio/src/
  mount.tsx:610`), which since #2564 returns early on `ok: false` so nothing
  gets written, but — like `dispatchSymbolAction` — raises no notification, so
  a refused code action or extract is silently dropped from the host's
  perspective (#2544). A third gap, distinct from the two above: `delete_symbol`
  (`crates/internal/brink-ide/src/structural_delete.rs`) is a real op with real
  refusals, but as of #2636 no studio surface calls it at all — no context-menu
  item, no dispatcher branch — so this clause has nothing to apply to yet. That
  is a reachability gap, not a non-compliant call site. Established
  by the file rename (`applyRename`, studio-store's binder slice); extended to the
  knot/stitch rename in #2528, where `performSymbolRename`'s error was previously
  returned to `SymbolRenamePrompt` and discarded when the prompt closed; and to
  the editor's inline (F2) rename commit path in #2543, where
  `applyComputedRename` / `applyMoveResult` applied a refused rename as if it
  had succeeded. The reason that path's guard is on `ok` and not `safe`:
  `error_json` (`crates/brink-web/src/editor_refactor.rs`) serializes a refusal
  with `safe: true` and no `introduced_diagnostics`, so `isSafeRename`
  (`packages/ink-editor/src/breakage.ts`) — which reads only those two fields —
  calls a refused rename "safe" and `settleCommit` commits it. `safe` describes
  the breakage of edits that were actually computed; `ok` is the field that
  says whether the operation happened, so `ok` is what both consumers guard on
  instead. Guarded by
  `packages/brink-studio/src/__tests__/symbol-rename-error-notify.test.ts` and
  `packages/brink-studio/src/__tests__/inline-rename-refusal.test.ts`.
  PROVISIONAL: this records where a refused rename reports, which follows the
  existing pattern. Whether the rename prompt should additionally *stay open* on
  failure is an open UX question (#2528) and is not settled here.
  **Folder rename is all-or-nothing (#2587).** The Binder's folder rename
  (`renameFolder` → `applyDirRename`, `binder.ts`) used to loop a per-file
  `renameFile` call over every file under the folder: a colliding file was
  silently skipped and the rest moved, batching only the survivors into one
  undo entry — a *partial-success* refusal shape. Once `renameFolder` was
  wired to the atomic `rename_dir` op (#314), that shape changed: `rename_dir`
  refuses the WHOLE move on any destination collision or an empty folder, so
  either every file under the prefix moves or none do. This is deliberate,
  not an oversight — a partial directory move can only be computed by
  falling back to per-file `INCLUDE` rewriting for the files that "succeed,"
  which reintroduces the exact cross-file inconsistency `rename_dir` exists
  to prevent (see §7.7.4's "Fifth enrolment"). A refusal surfaces through the
  same one-error-notification channel as every other refused structural op
  above; nothing moves and no undo entry is pushed. Undo of a successful
  folder move inherits the same all-or-nothing contract (`UndoEntry`'s
  `rename-dir` kind re-applies `rename_dir` with the prefixes swapped): if
  the inverse move is itself refused, the forward move's undo entry stays on
  the stack rather than being popped as if the inverse had succeeded.
  **Applied-but-unsafe rename/move reports too, at `warning` (#2918).** The
  clauses above cover a *refused* op (`ok: false`) reporting at `error`. A
  rename/move can also be computed and *applied* while introducing breakage
  (`safe: false` on an `ok: true` result) — a divert pointing at the renamed
  file, for example. The Binder's `applyRename`/`applyDirRename`
  (`notifyMoveResult`, `binder.ts`) report that case through the same `_notify`
  channel at `warning` severity, with a " (breaks N reference(s))" suffix on
  the move's normal message; the undo entry is still pushed, same as a clean
  move. This is deliberately a post-move report, not the refuse-and-Force
  semantic ruled for symbol rename (decision-log "Studio symbol Rename is
  safe-by-default with an in-place breakage report", #305): the Binder's
  inline tree rename has no confirm/force affordance to hang a preflight off
  of (unlike the symbol rename's dedicated widget), so the floor shipped here
  is notification-only. A fuller preflight for the Binder is tracked as a
  follow-up (#2918), not implied by this clause. **`moveFiles`** (the Binder's
  batch drag-multiple-files-to-folder move) is a known non-compliant path in
  this same sense: it loops the now-fixed `applyRename` per file, so each
  individual move is typed through `safe`/`introducedDiagnostics`, but its one
  summary notification does not aggregate that into a breakage count.
- **Out of scope:** progress notifications (compile/story status lives in the status
  bar, §7.3) and do-not-disturb modes.

### 7.6 Story session

The **story session** is the studio's handle on a live VM instance, and it is a
first-class model object — not a side effect of the Player. Today the `PlayerSlice` owns
the `StoryRunnerHandle` and the State View piggybacks on its `debugState`; that bundles
two things with different lifetimes (the session vs. the player's *UI* state — scroll,
fullscreen, choice hover), violating separate-concerns-by-ownership. The session is
extracted into its own slice/model in Phase 2.

```
status: none → running → awaiting-choice → (done | ended | error)
```

- **Contents:** the running program identity, runner handle, append-only transcript,
  name-resolved debug state, recorded choice history, status.
- **Session-bound views** (see §4) select from the session and never own or create it.
  With no session, they render a placeholder with a `story.start` affordance. Views are
  not auto-opened/closed by session lifecycle — the user controls layout; only content
  reacts.
- **Commands own the lifecycle:** `story.start`, `story.restart`, `story.stop`,
  `story.choose` — with `when` predicates over session status (§6), which also drive
  strip badges and status-bar state. No view mutates the session directly.
- **Recompile-while-running** (formalizing current behavior, per Inky): a successful
  compile invalidates the VM but not the session intent — the session restarts on the new
  program and replays the recorded choice history. If replay diverges (a recorded choice
  index no longer valid), the replay truncates at the divergence point and a notification
  (§7.5) says so. A failed compile leaves the existing session running on the old
  program, with the status bar showing the error state.
- **Single session at MVP.** The runtime supports multiple flows (`FlowInstance`;
  bevy-brink runs per-flow), so multi-session is a plausible future — the contract keys
  session-bound views to *the active session* rather than a global, so extending to a
  session selector later is additive, not a rework.

### 7.7 Overlay primitive

One shared floating/anchored-positioning primitive (candidate: floating-ui) under every
transient surface: the command palette, context menus
([BinderContextMenu.tsx](../packages/studio-ui/src/BinderContextMenu.tsx)), the element
conversion dropdown ([ElementDropdown.tsx](../packages/studio-ui/src/ElementDropdown.tsx)
— whose manual `position: fixed` rect-tracking is exactly the fragility this replaces),
the notification bell popover, and strip tooltips. It owns anchoring, flipping at
viewport edges, dismiss-on-outside-click/Escape, and focus return. Lands in Phase 1
(the palette needs it on day one); existing one-off implementations migrate to it as
their components are touched, not as a big-bang.

#### 7.7.1 Text-input seeding and selection (invariant)

Every text input in the studio obeys two rules — overlay-hosted ones (command
palette, quick pick, [NewFilePrompt.tsx](../packages/studio-ui/src/NewFilePrompt.tsx),
[SymbolRenamePrompt.tsx](../packages/studio-ui/src/SymbolRenamePrompt.tsx)) and
the inputs that live outside an overlay alike (the binder's in-row rename, the
query field in [SearchView.tsx](../packages/studio-ui/src/SearchView.tsx), which
is a tool window rather than an `Overlay`):

1. **Seed synchronously at mount.** An uncontrolled input's initial text is
   supplied by React while mounting it (`defaultValue`, re-keyed when the
   prompt is re-pointed at a different target) — never written from a later
   `requestAnimationFrame` or `setTimeout` callback. A deferred seed leaves a
   window in which the field is mounted, visible and editable but still blank,
   and it overwrites anything typed during that window; where the confirm path
   reads `input.value`, the clobbered edit silently degrades to "no change"
   (#2511). A *controlled* input satisfies this rule by construction.
2. **Never `select()` text the user typed.** `select()` does not destroy text
   itself, but it primes the next keystroke to replace the whole value, which
   loses the edit just as thoroughly. It may run only when the field still
   holds what the code put there, or in direct response to a user action that
   means "replace this value". Deferring `focus()` is harmless — a user who
   has already typed is already focused — but deferring `select()` is not.

Each call site records which arm it relies on.
[SearchView.tsx](../packages/studio-ui/src/SearchView.tsx) is the one place
where an *unguarded* `select()` is correct rather than a defect (#2527): its
query field is controlled, so rule 1 holds by construction, and the effect that
selects it runs only on mount or on an explicit `search.focus` (`Mod-Shift-F`,
palette "Search: Find in Files") — an invocation that means "replace this
query", following the VS Code precedent the command was modelled on. That
correctness rests entirely on `searchFocusSeq` being advanced by nothing except
that command: a focus request raised from a non-user-initiated path (results
arriving, a project reload, a focus-restore effect) would make the same
`select()` fire mid-typing and turn it into a live input-loss bug.
`packages/brink-studio/src/__tests__/search-view-focus.test.tsx` pins both
halves.

The rule reaches beyond the shell into `@brink-lang/editor`.
[inline-name-input.ts](../packages/ink-editor/src/inline-name-input.ts)
(`InlineNameInput`, the shared widget behind F2 rename and extract — exported
at `ink-editor/src/index.ts`, used from `extract-actions.ts`) was the last
outstanding violator: it ran `focus()` and an unguarded `select()` inside a
`setTimeout(…, 0)`. Fixed in #2535 by the same guard `SymbolRenamePrompt` uses,
compared against the widget's seeded `initialValue`. That surface was worse
than #2511's original mechanism, not milder — the clobbered value there
degraded the rename to a no-op, whereas an unguarded `select()` here let the
rename commit *the wrong string*.

The `setTimeout(…, 0)` itself stays, for a reason specific to this call site
and distinct from `SymbolRenamePrompt`'s (there it was `Overlay`'s
focus-return effect): `render()` is called from CM6's
`WidgetType.toDOM()`, which returns the element *before* the view inserts it,
and `focus()` on a detached element is a no-op. CM6's widget lifecycle is
`toDOM`/`updateDOM`/`coordsAt`/`destroy` — no post-mount hook exists to move
the call into. `packages/brink-studio/src/__tests__/inline-name-input-seed.test.ts`
pins all three halves: the guard, the still-selected untouched field, and the
deferral.

`select()` now appears exactly once across `packages/ink-editor/src`, at that
guarded call site; every other deferred call in the package is a bare
`focus()`, which rule 2 explicitly permits.

**Enrolment is itself enforced, not merely instructed (issue #2542).** Before
#2565, `search-view-focus.test.tsx` was the only test pinning rule 2, and it
only covers `SearchView` — a new `.select()`/`.setSelectionRange()` call site
anywhere else in `studio-ui`, `studio-shell`, or `ink-editor` shipped with no
signal, which is exactly the shape `inline-name-input.ts` took until #2548
fixed it. `packages/brink-studio/src/__tests__/select-call-enrolment.test.ts`
scans every workspace `src/` tree — roots derived from
`pnpm-workspace.yaml`'s `packages:` globs via
`packages/brink-studio/src/__tests__/workspace-roots.ts`, the same module
the `SAVE_PATHS` enrolment guard (`docs/embedder-api.md`, "Confirm and
retire in ONE synchronous step") uses, not a hand-typed
`studio-ui`/`studio-shell`/`ink-editor` list — for every real call site of any
of the **ten** spellings in the suite's `CALL_PATTERNS`, in every workspace
`src/` tree except `__tests__`/`dist`/`node_modules`/`.turbo` (so a suite that
races a seeded field does not have to enrol its own keystroke simulation):
zero-argument `.select()`, `.setSelectionRange(`, a `.selectionStart` write, a
`.selectionEnd` write, `execCommand(`, `getSelection()`, `createRange()`,
`.selectNodeContents(`, `.setBaseAndExtent(` and `.addRange(`. Each one
requires a
`// SELECT-INVARIANT <id>: <justification>` (or `SELECT-INVARIANT-EXEMPT`)
marker comment directly above each one, naming an id from
`packages/brink-studio/src/__tests__/select-calls.ts`'s `SELECT_CALL_IDS`.
The zero-argument boundary on `.select()` is deliberate, not a convenience
narrowing: `.select(` alone also matches unrelated same-named methods
elsewhere in the workspace (`themes.select(theme.id)`,
`api.select((s) => s.diagnostics)`), which have nothing to do with
`HTMLInputElement.select()`/`HTMLTextAreaElement.select()`; `.setSelectionRange(`
has no such collision today. An `-EXEMPT` marker's id is deliberately kept
*out* of `SELECT_CALL_IDS` — the registry enrols call sites this invariant
governs, and an exempt site is by definition not one of those — with the
justification alone required to be non-empty. This suite is a structural
sibling of that `SAVE_PATHS` enrolment guard and inherits the same
hardening: scan roots derived rather than hand-typed, every non-exempt id
required to name exactly one call site (the #2515 reuse loophole, closed
from the start), and the call-site count pinned exactly so a scan that stops
matching real calls cannot silently pass every check downstream.

**Why the eight sibling spellings are in that list (#2571).** `.select()` and
`.setSelectionRange(` are not the only ways to clobber an edit: a
`.selectionStart` / `.selectionEnd` write, `document.execCommand("selectAll")`,
and the Selection/Range API on a `contenteditable` (`getSelection()` /
`createRange()` + `selectNodeContents(` / `setBaseAndExtent(` / `addRange(`)
reach the same end state, so they enrol the same way. There were —
and still are — zero instances of any of them in the workspace, and that
emptiness is the argument for widening rather than against it: with nothing to
match there is no false-positive cost and no marker churn, whereas deferring
would bet on a future author picking the one spelling the scan happens to know.
The two property-write patterns match writes only (`\s*=[^=]`), so a
`selectionStart === selectionEnd` comparison is not a call site. A legitimate
future use (a read-only `getSelection()` for a caret coordinate) takes the
`SELECT-INVARIANT-EXEMPT` hatch.

**CodeMirror's `EditorView.dispatch({ selection })` is outside this invariant.**
It selects text programmatically and matches no `CALL_PATTERNS` entry, so it
was previously out by omission; this paragraph only writes down what §7.7.1
already says (#2580), it does not decide anything new. The invariant's subject
is fixed by its own opening sentence — "Every **text input** in the studio
obeys two rules" — and rule 1 is stated entirely in `<input>` vocabulary
("An *uncontrolled input's* initial text… `defaultValue`"; "A *controlled*
input satisfies this rule by construction"), which a CM6 document has no
counterpart for. The enrolment paragraph above then names the governed API by
type when it explains the zero-argument boundary:
`HTMLInputElement.select()` / `HTMLTextAreaElement.select()`, as against
same-named methods that "have nothing to do with" them. A CM6 transaction
moves the selection inside the document the author is *editing*, not the
pending, unsubmitted value of a field a confirm path later reads; the studio
drives it through explicit navigation verbs (`editor.reveal`, §6.1) and §7.8
already classes it as per-view view state ("selection/scroll stay per-view").
The `contenteditable` spellings above do not pull it back in — those are ways
of selecting a *field's* text that happens to be built without an `<input>`,
and CM never reaches for them from workspace `src/`; it owns its own selection
model. Where a CM surface does host a text input, that input is an `<input>`
and is enrolled on its own account — `InlineNameInput` is exactly that case.

**A marker's justification is proven, not just present (#2571).** Enrolment
proves a marker *exists*; it cannot prove the marker is *true* — the same gap
#2515 left open for `SAVE-PATH`. Every call site is therefore also backed by a
behavioural test: `search-view-focus.test.tsx`, `symbol-rename-prompt-seed.test.tsx`,
`inline-name-input-seed.test.ts`, and — for `Binder.tsx`'s two sites —
`packages/brink-studio/src/__tests__/binder-seed-race.test.tsx`. The binder's
two claims differ. The in-row rename pre-select claims there is *no deferred
window at all*, so its test asserts the selection is applied with no animation
frame having run, and separately pins the `key={editing.initial}` remount that
makes the claim true by construction: `RenameInput`'s effect is keyed
`[initial]` over an uncontrolled `defaultValue` input, so without that key a
same-instance seed change re-runs `setSelectionRange` over user-typed text.
The new-file field's call *is* deferred into a `requestAnimationFrame` and is
raced directly; its narrower claim is that the range stays zero-width
(`start === end`) — a caret placement, with nothing to clobber — and that `end`
is read from `input.value` at fire time rather than captured before the frame.

**Those behavioural tests are themselves deletion-mutation audited (#2580).**
Building `binder-seed-race.test.tsx` surfaced a trap that makes a caret
assertion silently worthless: **a write to an input's `.value` property parks
the caret at the end of that value**, so "the caret lands at the end" can hold
with the production call site deleted, and React seeds an uncontrolled field by
writing that property — a freshly mounted `defaultValue={"barter"}` input reads
`(6, 6)`.

This was first measured in jsdom, and #2580 recorded the park as a *jsdom*
behaviour that "a real browser, seeded through the `value` **attribute**,"
would not show, reading `(0, 0)` instead. **That browser half was inferred, not
observed, and it is wrong (#2595).** Every cell below is measured, and every
cell is pinned by a test that fails if it changes — in Chromium 145.0.7632.6 by
the control block in `packages/brink-studio/e2e/symbol-rename.spec.ts` ("a
defaultValue-seeded field parks the caret at the end in a real browser"), and in
jsdom 29.0.1 by `packages/brink-studio/src/__tests__/seed-path-caret-jsdom.test.ts`,
which drives the same four paths the same way. Both columns are held to the same
standard deliberately: this section exists because a reading measured once and
then cited is indistinguishable, later, from one that was assumed.

| how the field is seeded | Chromium 145 | jsdom 29 |
| --- | --- | --- |
| `value` **attribute** (`<input value="barter">`) | `(0, 0)` | `(0, 0)` |
| `setAttribute("value", "barter")` | `(0, 0)` | `(0, 0)` |
| `.value` **property** write | `(6, 6)` | `(6, 6)` |
| React `defaultValue={"barter"}` at mount | `(6, 6)` | `(6, 6)` |

The two environments agree on every row.

Both recorded readings were real; the error was joining them to the wrong
paths. React does **not** seed an uncontrolled field through the `value`
attribute — the e2e observes it writing the `.value` IDL property on
`#brink-rename-input`, which is the end-parking path, and per the HTML
standard the `value` setter is *specified* to "move the text entry cursor
position to the end of the text control". So the park is **platform behaviour
that jsdom faithfully reproduces, not a jsdom artifact**, and the field an
author actually faces on a fresh prompt already reads `(6, 6)` before any
studio code touches it.

That makes the suites' `setSelectionRange(0, 0)` resets *more* load-bearing,
not less, and they must not be removed: their job is to stop an assertion
inheriting its expected answer from the seed — a **vacuity guard**, not a
correction of jsdom toward browser fidelity. Their earlier justification
("reset so the test observes the browser's starting state rather than jsdom's")
was the same inference and is likewise retired.

Every caret/selection assertion in the three older suites was
therefore re-checked by deleting the production call site it covers and
confirming the suite goes red. Thirteen mutations across
`SearchView.tsx`, `SymbolRenamePrompt.tsx`, `inline-name-input.ts` and the
search store's `setSearchQuery`; twelve reddened the intended assertion. The
one that did not
was not a seed park but a missing *positive* half:
`symbol-rename-prompt-seed.test.tsx` asserted only where the selection must
NOT go, so deleting `SymbolRenamePrompt`'s `input.select()` outright — as
opposed to un-guarding it — left the suite green. It now carries the same
shape of preservation guard the binder suite uses. A caret assertion whose
expected position coincides with the end of the value must reset the selection
before the act under test, exactly as `binder-seed-race.test.tsx` does.

Rule 1 ("seed synchronously at mount") still has no structural enforcement of
its own; #2571 tracks the design for a companion guard.

#### 7.7.2 Deferred focus-timer teardown (invariant)

§7.7.1 rule 2 governs *whether* a deferred call is allowed to touch the
selection; this rule governs a related but separate hazard: a deferred
`focus()` call that a teardown path does not, in fact, tear down. "Deferring
`focus()` is harmless" (rule 2) is true only while the callback that eventually
runs is a bare `focus()` on a node that is either still live or has already
been detached before the callback fires — the harm §7.7.1 rules out is a
`focus()`/`select()` racing a keystroke, not a `focus()` firing after its
owning controller believes it has been torn down. #2557 found that gap: a
`setTimeout(…, 0)` handle that is never assigned to a field cannot be
cancelled by `dispose()`/`stop()`/`destroy()`, however completely that method
otherwise "tears them all down."

The rule: **every deferred `setTimeout(…, 0)` that exists only to run
`focus()` (or an equivalent, e.g. a popover's own `onClose`-adjacent hook)
after a widget mounts must store its handle on a field (or closure-local
variable) and cancel it — via `clearTimeout`, guarded by a `!== null` check —
in two places, not one:

1. **Before re-assigning the field**, if the same controller instance can
   schedule a second such timer while an earlier one from the same field is
   still pending (e.g. a badge refresh re-opening an already-open report, or a
   host re-invoking `render()`/`toDOM()` on the same instance before the first
   timer fires). Skipping this clear-before-set step orphans the first handle:
   the field now points only at the second, so the eventual `dispose()` call
   can cancel only the second one, and the first is stranded with no code path
   left that references its ID.
2. **In the teardown method itself** (`dispose()` / `stop()` / `destroy()`),
   so a handle that is still pending when the controller is torn down never
   fires after teardown at all.

Both steps are required; each covers a failure the other does not. Skipping
(1) leaks the pattern back into an already-torn-down instance the moment a
second timer is scheduled before the first resolves. Skipping (2) leaks
whenever teardown itself is the event that ends the widget's life, which is
the common case.

**Enrolled sites (#2557 / #2558).** Four deferred post-mount/post-close focus
timers carry this pattern, each independently confirmed to transfer rather
than assumed by analogy:

- `packages/ink-editor/src/inline-name-input.ts` — `InlineNameInput`'s two
  sites: `render()`'s post-mount `focusTimer` (`this.focusTimer`, guarded
  against a second `render()` call and cancelled in `dispose()`) and
  `renderReport()`'s force-button `forceFocusTimer` (guarded against a badge
  refresh re-opening the already-open report, and likewise cancelled in
  `dispose()`).
- `packages/ink-editor/src/extract-actions.ts` — `ExtractPrompt`'s `stop()`
  deferred `view.focus()`, cancelled in `destroy()`.
- `packages/ink-editor/src/rename.ts` — `InlineRename`'s `stop()` deferred
  `view.focus()` (the same shape as `extract-actions.ts`, found while fixing
  #2558 in the same file), cancelled in `destroy()`.
- `packages/ink-editor/src/argument-widgets.ts` — `openValuePicker`'s
  post-mount filter-input focus timer, held in a closure-local `let` (there is
  no controller instance to hang a field off) and cancelled in `openPopover`'s
  `onClose` callback, the dispose-equivalent hook for a popover.

Not every deferred `setTimeout` in the package is in scope: one in
`argument-widgets.ts` (inside the auto-open `updateListener`, ~line 1042)
does non-focus-shaped work (a `view.dispatch` plus a follow-up call) and was
deliberately left alone — the pattern above is specific to bare
focus-on-mount/focus-on-close timers, not a blanket rule about every deferred
callback in the file.

`InlineNameInput`'s two sites are pinned by
`packages/brink-studio/src/__tests__/inline-name-input-seed.test.ts`: fake
timers, a spy on `HTMLInputElement.prototype.focus` (`render()`'s site) and
`HTMLButtonElement.prototype.focus` (`renderReport()`'s site), a second
call that would orphan the first handle absent the clear-before-set guard,
teardown via `dispose()`, then `vi.runAllTimers()` asserting the spy was
never called.

#### 7.7.3 Dismiss contract for non-`Overlay` surfaces, and the global net (#279)

`Overlay` (§7.7) owns Escape/outside-pointerdown dismissal for anything that
routes through it. A handful of transient surfaces are DOM-level by design
and do not — [`widget-popover.ts`](../packages/ink-editor/src/widget-popover.ts)
and [`widget-modal.ts`](../packages/ink-editor/src/widget-modal.ts) (argument-
widget chrome anchored to CodeMirror decorations; see
`docs/argument-widget-spec.md` §6), the code-actions menu
([`code-actions.ts`](../packages/ink-editor/src/code-actions.ts)), the inline
element-type picker (`keybindings.ts`, Alt+Enter), and
[`BinderContextMenu.tsx`](../packages/studio-ui/src/BinderContextMenu.tsx)
(reused by `SymbolContextMenuHost` for the editor/Story-Graph symbol menu).
Every one of these owes `Overlay`'s dismiss contract without inheriting it for
free: a `document`-level, **capture-phase** `pointerdown` (outside-target) and
`keydown` (Escape) listener pair, attached directly by the surface and torn
down with it. Capture-phase, not bubble — a bubble-phase listener is
defeated by any unrelated ancestor's `stopPropagation()` sharing the bubble
path (#279's original stuck-menu shape: `BinderContextMenu` used bubble-phase
`mousedown`/`keydown` before this fix).

**The global dismiss registry** (`dismiss-registry.ts`) is a second,
independent layer on top of that per-surface contract, not a replacement for
it: a surface registers a close callback (`registerDismissible`, in a
separate effect/lifecycle hook from its own listener setup) while open, and
one shared listener closes everything still registered when nothing else
handled the key. It exists specifically for the failure #279 named — a
surface's own dismiss listener orphaned by a re-render/error while the
surface stays visibly mounted — and is deliberately structured so a bug in
the per-surface listener setup cannot take the registry registration down
with it. **Two independent module instances** exist — `packages/studio-shell/src/dismiss-registry.ts`
(re-exported from that package's `index.ts`) and `packages/ink-editor/src/dismiss-registry.ts`
— because `ink-editor` has no dependency on `studio-shell`; there is no
cross-package net, only two same-shaped ones, each covering the surfaces in
its own package.

**Listener ordering (load-bearing).** The registry's shared listener attaches
on `window`, in the **bubble** phase — not `document`/capture, which is what
every individual surface's own listener uses. This is not incidental: the
registry installs its listener once, the first time anything in the process
ever calls `registerDismissible()`, and never re-attaches after that — so on
every surface opened afterward, the registry's listener was almost always
registered *before* that surface's own (which only gets attached fresh each
time the surface opens). Two same-phase, same-target listeners fire in
registration order; if the registry listener were also `document`-capture, it
would then run *first* on most opens, calling every registered `onClose` —
including the surface's own — before that surface's own capture-phase
handler ever got a chance to run its `preventDefault()`/focus-return logic.
Concretely, that shape stripped `CodeActionsMenu.close()`'s
`this.view.focus()` call on the second-and-later open of the menu, and let
Escape leak to CodeMirror's keymap; and it made
`dismissAllTransientSurfaces()` — which closes *every* registered surface
unconditionally — tear down a whole surface stack on one Escape instead of
just the surface that should have handled it.

Attaching on `window`/bubble instead fixes this by construction, not by
relying on registration order: every `document`-capture listener (every
surface's own) runs to completion in the capture phase strictly *before* any
bubble-phase `window` listener gets a turn, regardless of which was attached
first. A surface that handles Escape itself calls `preventDefault()`, which
the registry listener's `event.defaultPrevented` guard then honors; a surface
that calls `stopPropagation()` keeps the event from reaching `window` at all.
The registry listener fires — and only then sweeps whatever is left
registered — exactly when nothing in the dispatch path already handled the
key, which is the orphan case it exists for.

A regression test for this ordering must not call the test-only
`resetDismissRegistryForTests()` immediately before every mount: doing so
forces the registry's listener to install *after* the surface under test on
every single case, which is the inverse of the production shape above and
cannot expose an ordering bug. The regression coverage
(`overlay-dismiss-safety-net.test.tsx`, `binder-context-menu-dismiss.test.tsx`,
`dismiss-registry-orphan.test.ts`) resets once per `describe` and then opens
a surface a *second* time with the registry listener already live, asserting
the surface's own handler ran (`event.defaultPrevented`, a focus-return spy)
rather than a bare fallback from the net.

**Enrolment is itself enforced, not merely instructed (#2766).** Nothing
above stopped a surface #11 from shipping its own `document`-level
`keydown`/`pointerdown` dismiss listener without also calling
`registerDismissible()` — silently falling back into the unescapable-menu
failure mode #279 was filed for, invisibly (no test failure, no diagnostic).
`packages/brink-studio/src/__tests__/dismiss-registry-enrolment.test.ts`
closes that gap the same way `select-call-enrolment.test.ts` closes it for
§7.7.1's selection invariant: it walks every workspace `src/` tree (roots
derived from `pnpm-workspace.yaml`, not hand-typed) for every real
`document.addEventListener("keydown" | "pointerdown", ...)` call, and
requires each one to be either **module-enrolled** — the file imports
`registerDismissible` (from its own `./dismiss-registry`, or, for a package
with no registry of its own, from a registry-owning package's published
name — e.g. `studio-ui`'s [`BinderContextMenu.tsx`](../packages/studio-ui/src/BinderContextMenu.tsx)
importing from `"@brink/studio-shell"`) and calls it, which covers every
listener call site in that file — or marked with a `// DISMISS-NET-EXEMPT:
<reason>` comment (mirroring `SAVE-PATH-EXEMPT`) directly above the call
site. `studio-shell`'s three Escape-cancels-a-gesture/layout-restore
listeners (`tab-drag.ts`, `strip-drag.ts`, `regions.tsx`) carry that marker:
they manage transient interaction/layout state, not a floating
menu/popover/modal DOM surface, so `registerDismissible()` does not apply.
[`ElementDropdown.tsx`](../packages/studio-ui/src/ElementDropdown.tsx) also
carries one: its listener handles arrow/Enter/shortcut-key navigation only,
not Escape dismissal, which its wrapping `Overlay` (already enrolled) owns.
The guard does not unify the two registries — Escape still only dismisses
surfaces within one package at a time — it only ensures every qualifying
listener, in either package, is accounted for by one or the other. (Whether
the two registries are worth unifying — one shared registry vs. a
coordinator that fans out — is an open design question, not decided here;
`studio-ui` surfaces enrol into `studio-shell`'s registry while `ink-editor`
keeps its own, so a surface stack spanning both packages is only partially
dismissed by a single Escape. Surfaced, not built, by #2846.)

**Widened past `document`-only, and every exempt marker proven, not just
asserted (#2846).** Three gaps review of PR #2838 found in the guard above,
none a defect in it — #2766 asked for a `document`-level scan and that is
what it built:

1. **The LISTENER pattern matched `document` only.** `dismiss-registry.ts`
   itself attaches its net listener on `window` (see "LISTENER ORDERING"
   above), so "attach the way the registry does" was the single most
   plausible next-surface shape, and it would have shipped unguarded and
   unflagged. The pattern now matches `document`/`window`/`ownerDocument` ×
   `keydown`/`keyup`/`pointerdown` — both axes #2846 named, widened
   symmetrically, and deliberately no further: over-widening trades the
   coverage hole for `DISMISS-NET-EXEMPT` marker-noise that erodes the
   convention itself (the same boilerplate-nobody-reads failure mode #2766
   already had to weigh). Widening surfaced exactly two real call sites —
   both registries' own `window.addEventListener("keydown", ...)` net
   installs — which now each carry a `DISMISS-NET-EXEMPT` marker of their
   own: that call site *is* the net, not a surface enrolling into it.
2. **Exempt markers asserted a claim nothing checked.** Modelled on the
   `SAVE-PATH` precedent this guard was built from (§7.7.1 above, #2571:
   "a marker's justification is proven, not just present"), every
   `DISMISS-NET-EXEMPT` marker — the three drag/maximize ones, `ElementDropdown.tsx`,
   and the two net-listener ones from point 1 — now has a dedicated
   behavioural test asserting its specific claim against the real
   production module: `dismiss-net-exempt-claims.test.ts` for the first
   four, `dismiss-registry-net-listener.test.ts` (in both `ink-editor` and
   `brink-studio`, one per registry) for the net-listener two.
3. **A JSDoc example mentioning the pattern counted as a real call.**
   `scanListenerSites` used to skip `//`-prefixed lines only; a block
   comment quoting the listener shape in prose had no way to be marked
   exempt (the walk-up only recognizes a `//` marker directly above a real
   call). Fixed by blanking block-comment spans before scanning for calls,
   while still using the raw source for the exempt-marker walk-up and the
   reported call text — see `dismiss-registry-enrolment.test.ts`'s
   `blankBlockComments`.

#### 7.7.4 Off-paint-path analysis deferral (invariant)

A third, related hazard: a synchronous call that is not merely deferred (§7.7.2)
but *heavy* — expensive enough to itself block the main thread, and therefore
paint, for however long it runs. §7.7.2 governs cancelling a deferred call;
this rule governs how a call that cannot be made cheap or asynchronous is kept
from racing the paint of its own "this is running" feedback.

The rule: a symbol-rename/collision analysis that runs as a synchronous wasm
call (no `Promise`, no yield point of its own) must not be invoked inline in
the same frame as the triggering event. Instead: commit a `busy`/`pending`
state **synchronously** (so React has something to paint before the heavy call
starts), then defer the call itself to the next idle slot via
`scheduleIdleWork` (`packages/ink-editor/src/idle-schedule.ts`) —
`requestIdleCallback` with an 300ms timeout, falling back to a `setTimeout`
macrotask where `requestIdleCallback` is unavailable (Safari, jsdom).
`cancelIdleWork` cancels a still-pending handle.

**This is a mitigation, not a bound.** `idle-schedule.ts`'s own doc comment
states the limit directly: "It is not a substitute for a worker: a call that
itself blocks for seconds still blocks once it starts." Deferring the call
guarantees a paint of the pending indicator lands *before* the heavy work
begins; it does not shorten the heavy work itself, and a test asserting a
fixed wall-clock bound on the call's completion is asserting something this
discipline does not provide.

**Enrolled sites (#722, #696).**

- `packages/ink-editor/src/inline-name-input.ts` — `InlineNameInput` (F2
  inline rename / extract), the original site. Commits
  `.brink-inline-rename-badge--pending` synchronously, defers the analysis via
  `scheduleIdleWork`, and cancels a stale handle via `abandonPending()`.
- `packages/studio-ui/src/SymbolRenamePrompt.tsx` — the modal knot/stitch
  rename prompt (context-menu "Rename…"), added in #696 after two recurrences
  (PR #1500, PR #1888) of the flake #722 had already fixed on the sibling
  inline surface but never carried to this one. Commits `.brink-rename-pending`
  ("Checking for conflicts…") synchronously, defers via the same
  `scheduleIdleWork`/`cancelIdleWork` pair (now exported from
  `@brink-lang/editor`'s package entry point rather than kept internal to
  `ink-editor`, so a second surface can consume them without reaching past the
  package boundary).

**The pending state must stay current across a cancel.** Because the deferral
opens a window between commit and call, `Overlay`'s Escape/outside-pointerdown
dismissal (§7.7, `packages/studio-shell/src/overlay.tsx`) can close the prompt
while the analysis is still queued — those listeners are not gated on `busy`.
`SymbolRenamePrompt` handles this two ways: an effect keyed on the overlay's
`open` state cancels any pending idle handle the moment the prompt closes
(not only on unmount, since the component is mounted once near the studio
root and a close does not unmount it), and `run()` re-checks that the store's
live `renamePrompt` is still reference-equal to the request it captured before
calling `performSymbolRename` after the await, bailing otherwise. Pinned by
`packages/brink-studio/src/__tests__/symbol-rename-prompt-pending.test.tsx`.

**The pending flash is not asserted in the e2e spec.** Measured with
`--repeat-each=15` against a real (Chromium) browser on a mostly-idle page,
`.brink-rename-pending`'s visibility window was too brief for Playwright's CDP
polling to reliably observe: 10/15 runs missed the indicator, 0/15 missed the
eventual report. `packages/brink-studio/e2e/symbol-rename.spec.ts` therefore
asserts only the report, not the pending flash — a live browser's polling
granularity is the wrong tool for a sub-poll-interval ordering claim. The
ordering itself (pending-committed-before-analysis-runs) is instead proven
deterministically in `symbol-rename-prompt-pending.test.tsx`: jsdom has no
`requestIdleCallback`, so `scheduleIdleWork` always takes the `setTimeout`
fallback, and with fake timers the test asserts the pending indicator is
already in the DOM *before advancing any timer at all* — i.e. before the
deferred analysis could possibly have run.

**Third enrolment (#2767): the Binder's structural symbol menu.** The same
hazard recurred a third time in `dispatchSymbolAction`'s `moveStitch` /
`promoteStitch` / `demoteKnot` branches (`packages/studio-ui/src/
symbolMenuActions.ts`), reached from the Binder's context menu and its
drag-drop `onDrop` — one-shot dispatches with no open editing surface, unlike
the two enrolments above. `runGatedStructuralOp` applies the same shape
(commit pending state synchronously, defer via `scheduleIdleWork`) with two
differences from the rename enrolments, both intentional:

- **The pending state renders in the status bar, not a notification.**
  `moveStitch`/`promoteStitch`/`demoteKnot` have no persistent widget to hold
  component-local `busy` state, so the pending description lives in the store
  (`structuralOpPending: string | null`, `symbol-menu` slice) and renders via
  `StructuralOpSegment` (`packages/studio-ui/src/StatusBar.tsx`, registered
  left-group priority 9 in `mount.tsx`). It is deliberately NOT raised through
  `state._notify`/the shell notification service: §7.5 states progress
  notifications are out of scope for that service, and an earlier draft of
  this fix raised one anyway, which both violated §7.5 and double-toasted on
  the success path (`applyMoveResult` already raises its own "Move X to Y"
  notification with Undo).
- **No staleness re-check.** The rename enrolments re-validate that the
  captured request/session state is still current before applying a result
  computed after the idle wait (#2761's lesson). `runGatedStructuralOp` does
  not: its `compute` thunk calls the wasm op fresh, against whatever the
  session's live source is at invocation time, never against a pre-idle
  snapshot — and the op itself already refuses cleanly when its target has
  moved out from under it. A `session.generation` equality check was tried
  and removed: `generation` bumps on every content-mutating call, including
  one keystroke in a *different, unrelated* mounted editor view
  (`ink-editor`'s `elementTypeField` pushes the document on every changed
  transaction), so the check silently dropped legitimate queued moves on
  totally unrelated edits far more often than it caught a genuine staleness
  hazard — and there was no genuine hazard on this path for it to catch.
  Trust `result.ok` instead. (This is a staleness claim, distinct from
  *teardown* safety: an earlier version of this bullet also claimed "no
  widget instance to `cancelIdleWork` on unmount ... so there is no
  staleness guard to run here at all," which read as covering
  `ProjectSession.destroy()` too. It didn't — `runGatedStructuralOp` rolled
  its own bare `scheduleIdleWork` yield outside `ProjectSession` entirely, so
  destroying the session mid-defer could still reach a freed `session`
  handle in `compute()`. See "Two structural gaps closed (#2794)" below for
  the fix.)

**Which structural ops are gated vs cheap** (`crates/brink-web/src/editor/
refactor.rs`, backed by `crates/internal/brink-ide/src/structural_result.rs`):
the op-agnostic breakage gate (`gate_with_source`, wired through
`gated_move_json`/`structural_result_json`/`dir_move_result_json`) runs a
full-project overlay reanalysis and is therefore in this invariant's scope;
`move_result_json_simple` skips it (`StructuralResult::safe_source` — a
reorder changes no qualification, so it is genuinely cheap, not merely
assumed so).

| op | helper | gated? | off-paint-path? |
|---|---|---|---|
| `move_stitch` / `promote_stitch` / `demote_knot` | `gated_move_json` | yes | yes (`runGatedStructuralOp`, #2767) |
| `rename_file` | `structural_result_json` | yes | yes (deferred in `ProjectSession.renameFile`; pending state committed by `applyRename` in `binder.ts`, #2776) |
| `rename_dir` | `dir_move_result_json` | yes | yes (deferred in `ProjectSession.renameDir`; pending state committed by `applyDirRename` in `binder.ts`, #2587 — see "Fifth enrolment" below) |
| `rename_symbol` / `rename_symbol_at` | `structural_result_json` | yes | yes (#722 inline, #696/#2761 modal) |
| `extract_to_knot` / `extract_to_function` | `structural_result_json` | yes | yes, transitively (built on `InlineNameInput`, #722) |
| `reorder_stitch` / `reorder_knot` / `reorder_stitches` / `reorder_knots` | `move_result_json_simple` | no | n/a — not heavy, runs inline |
| code actions routed through `resolve_code_action_impl` (`crates/brink-web/src/editor/code_actions.rs`) | `gated_move_json` for a `MoveStitch`/`PromoteStitch`/`DemoteKnot`-tagged action, `move_result_json_simple` for everything else | **conditionally** | n/a — the gated branch is unreachable from the studio menu (see below); left synchronous |

**Marker convention and its guard.** A call site enrolled in this invariant
carries a `// PAINT-PATH-DEFERRED <id>: <reason>` (wrapped by the remedy) or
`// PAINT-PATH-EXEMPT <id>: <reason>` (genuinely cheap, matched by pattern but
out of scope) comment directly above it. `packages/brink-studio/src/
__tests__/paint-path-call-enrolment.test.ts` statically scans every
`packages/*/src` file (roots derived from `pnpm-workspace.yaml`, same pattern
as `select-call-enrolment.test.ts`) for `.moveStitch(`/`.promoteStitch(`/
`.demoteKnot(`/`.session.renameFile(`/`.session.renameDir(` call sites and
fails if one lacks a marker with a real (>10 char) reason — deliberately
scoped to exactly those five method-call shapes, not every gated op in the
table above (see the test file's own header for why). It is a marker-presence
scan, not a control-flow check: it cannot verify a `DEFERRED` site is
actually wrapped correctly, only that a human wrote a marker and so had to
look. Real behavioral verification is `symbol-structural-ops.test.ts`'s "run
off the paint path" describe block for the symbol-menu trio,
`file-rename.test.ts`'s "runs off the paint path" describe block for
`rename_file`, and `folder-rename.test.ts`'s equivalent describe block for
`rename_dir`.

**Fourth enrolment (#2776): the Binder's file rename-and-move.** The
same hazard recurred a fourth time in `ProjectSession.renameFile`
(`packages/ink-editor/src/project-session.ts`), reached from the Binder's
inline rename, drag-move, and multi-select move (`applyRename` in
`binder.ts`, backing `renameFile`/`moveFile`/`moveFiles`). At the time,
`renameFolder` still looped one `renameFile` call per contained file — see
"Fifth enrolment" below for where that changed. Unlike the
symbol-menu trio, the deferred call and the busy-state commit sit in
different packages: `ProjectSession` (`ink-editor`) has no store to commit UI
state into, so it only does the `scheduleIdleWork` half — `renameFile` yields
before touching the gated wasm call, which means the deferral applies to
*every* caller of `ProjectSession.renameFile`, not just the Binder. The
synchronous half — committing `structuralOpPending` before that first
`await` — lives in `applyRename` (`studio-store`'s `binder.ts`), the
store-aware caller, reusing the SAME field and `StructuralOpSegment` #2767
introduced rather than a parallel mechanism (see that field's doc comment in
`symbol-menu.ts`, which #2776 generalized from "the symbol-menu trio" to "any
gated structural op"). Same staleness posture as the trio: no
`session.generation` re-check, trusting `rename_file`'s own refusal
(`RenameFileError::DestinationExists`, surfaced as `result.ok: false`)
against whatever the session's live source is when the deferred call actually
runs.

**Fifth enrolment (#2587): the Binder's folder rename-and-move.** `rename_dir`
runs the identical gate to `rename_file`
(`crates/internal/brink-ide/src/dir_rename.rs`'s `gate_dir_move`) and was
named alongside it in #2776's ask, but at that time **no code under
`packages/*/src` called it** — the Binder's folder rename (`renameFolder` in
`binder.ts`) looped per-file `renameFile` calls instead of calling the wasm
`renameDir`/`rename_dir` op, so there was nothing on a real user path to
defer (#2776 audited and left it out on those grounds). #2587 closed that
gap: `renameFolder` now calls `ProjectSession.renameDir` (the directory
analog of `renameFile`, same file) through a new `applyDirRename` helper in
`binder.ts` (the directory analog of `applyRename`), replacing the per-file
loop entirely. `ProjectSession.renameDir` gets the identical treatment as
`renameFile` — `deferGatedCall`'s yield before the gated
`this.session.renameDir(...)` call — and `applyDirRename` commits
`structuralOpPending` before the first `await`, the same split as the fourth
enrolment above. All-or-nothing, unlike the old per-file loop: `rename_dir`
refuses the WHOLE move on any collision or an empty folder rather than
silently skipping a colliding file, so a refusal surfaces as one error
notification with nothing moved.

**Audited and left out, on purpose (#2776).** #2776's ask named three sites:
`rename_file` and `rename_dir` both eventually got enrolled (the fourth and
fifth enrolments above — `rename_dir` only once #2587 gave it a caller), and
the third turned out not to need this treatment at all:

- **`applyCodeAction`** (`document-sessions.ts`'s `resolveCodeAction`
  callback) routes through `resolve_code_action_impl`, which CAN take the
  gated `gated_move_json` branch — but only for a `MoveStitch`/
  `PromoteStitch`/`DemoteKnot`-tagged `CodeActionData`. `brink_ide::
  code_actions::code_actions` — the only function that builds the list the
  studio's code-actions menu actually offers — never constructs any of those
  three variants, nor do the import-fix/creation-site-fix/value-call-fix
  generators merged alongside it; every code action the menu can offer today
  resolves through the cheap `move_result_json_simple` pure-rewrite branch.
  (The LSP backend, `crates/brink-lsp/src/backend.rs`, does construct those
  variants for its own resolver — a different transport, not a path through
  this callback.) Wrapping an always-cheap call would add an idle-hop and a
  pending-indicator flash for zero benefit — the regression this whole
  invariant exists to prevent, not a fix. Left synchronous.

**Two structural gaps closed (#2794).** #2788's adversarial re-review of the
fourth enrolment found two hazards in the shared shape itself — "the
enrolment family's gap, not this PR's":

- **A deferred gated call can outlive `ProjectSession.destroy()`.** The
  `scheduleIdleWork` yield opens a window (up to the 300ms
  `requestIdleCallback` timeout) between a caller committing
  `structuralOpPending` and the deferred wasm call actually running. Before
  #2794, `renameFile` had no `this.destroyed` check after that yield, and its
  idle handle was never `cancelIdleWork`'d on teardown — an unmount landing
  inside the window let the callback fire anyway and call
  `this.session.renameFile(...)` on a wasm handle `destroy()` had already
  freed. This was CONTAINED, not unreachable, before the fix: the throw
  landed in `applyRename`'s `catch` and surfaced as an ordinary error
  notification — but containment is not a fix, and the hazard is generic to
  every call this class defers via `scheduleIdleWork`, present or future, not
  specific to `renameFile`.

  ⚠ **Scoped to the idle-yield window only — corrected by #2802, see the
  bullet below.** This claim, and the "one guard, applied once" framing two
  paragraphs down, are about the `scheduleIdleWork` yield specifically: they
  say nothing about — and do not fix — the much larger window that opens
  right after it, when a method goes on to `await` the host provider itself
  (Tauri IPC, unbounded, not a handle `deferGatedCall`'s tracking can cancel).
  A reader auditing this family for "is `ProjectSession` destroy()-safe" must
  not stop here; see #2802's bullet below for the guard that actually covers
  post-host-IO-await continuations.

  The fix: `ProjectSession.deferGatedCall` (the
  yield every deferring method now goes through, replacing a bare
  `scheduleIdleWork` await) tracks its idle handle and rejects the caller's
  await — instead of resolving into a freed session — if `destroy()` runs
  first; `destroy()` cancels every still-pending handle and rejects its
  caller before freeing the wasm handle. One guard, applied once, covering
  every gated call this class defers rather than a per-site sprinkle. Pinned
  by `packages/ink-editor/src/__tests__/project-session-destroy.test.ts`.

  **Half-fixed at first landing — closed by #2794's own follow-up review.**
  "Every gated call this class defers" only covered calls that actually went
  through `ProjectSession`. `runGatedStructuralOp` (the third enrolment,
  above) never did: it rolled its own bare `scheduleIdleWork` yield inside
  `studio-ui`, entirely outside this guard, so `ProjectSession.destroy()`
  landing mid-defer there could still reach a freed `session` handle in
  `compute()` — the identical hazard this bullet describes for `renameFile`,
  just less contained (`dispatchSymbolAction` is dispatched `void`, fire-and-
  forget, so the throw would have been an unhandled rejection, not even
  `applyRename`'s caught-and-notified one). The follow-up review caught this
  before it shipped: `deferForGatedCall` was made public as
  `ProjectSession.deferGatedCall()` for exactly this reuse, and
  `runGatedStructuralOp` now awaits it instead of its own yield, catching and
  swallowing the destroy rejection (the `void` dispatch has no caller to
  rethrow to) and skipping `applyMoveResult`. Pinned by a case in
  `symbol-structural-ops.test.ts` mirroring
  `project-session-destroy.test.ts`'s first case. The family is now actually
  closed, not merely believed to be.
- **`structuralOpPending` is a two-writer field with last-writer-wins
  clearing.** `runGatedStructuralOp` (symbol-menu ops) and `applyRename`
  (Binder rename/move, #2776) are independent fire-and-forget (`void`)
  dispatches that both write this field, and before #2794 both cleared it
  unconditionally in `finally`. An overlapping Binder drag-move and
  symbol-menu op — e.g. a drag-move started while a `moveStitch` from the
  context menu is still mid-flight — let whichever settled LAST erase
  whichever description was actually live, not necessarily its own: the
  status-bar indicator for the still-running op could vanish, or get
  silently replaced by a stale "done" state for the op that already
  finished. The fix is compare-and-clear: `clearStructuralOpPending(
  description)` (`SymbolMenuSlice`, `studio-store`'s `symbol-menu.ts`) only
  nulls the field when the live value still equals the description the
  clearing call itself set, a no-op otherwise. Both writers now clear through
  this instead of calling `setStructuralOpPending(null)` directly. Pinned by
  `symbol-structural-ops.test.ts`'s "structuralOpPending compare-and-clear
  across the two writers" describe block (both overlap orders).

  The field still lives in the `symbol-menu` slice even though `binder.ts`
  has written it since #2776 — noted, not relocated, in #2794: the two
  correctness gaps above were the fix this PR scoped to; moving the field to
  a more neutral home (and updating every reference here) is a follow-up, not
  bundled with a race-condition fix.

**A second, larger window closed (#2802).** #2798's fix (above) covers the
`scheduleIdleWork` yield only — `deferGatedCall` tracks that one idle handle
and rejects it on `destroy()`. It has nothing to say about what happens
right after: `renameFile`, `deleteFile`, `requestFile`, `resolveIncludes`,
`addFile`, and the initial `initialize()` load all `await` the host provider itself
(Tauri IPC — unbounded, and often far longer than the idle window's ≤300ms
ceiling) and then resume touching `this.session`/`this.changes` with no
re-check. `destroy()` cannot reject those awaits — they are not idle handles
it tracks — so a teardown landing during any of them reached a freed wasm
handle one `await` later, the same use-after-free #2794 set out to close.
Both adversarial reviewers on #2798 found this independently and disagreed
on whether it blocked that PR; the fix that shipped there applied the
non-blocking read, so this gap shipped unowned until #2802.

The fix generalizes past the idle-yield-specific guard rather than adding a
per-site `if (this.destroyed)` check at each of the six call sites above:
`ProjectSession.assertLive()` (private) is one seam every post-host-IO-await
continuation calls the instant it resumes, before touching session state
again — throwing the same error family `deferGatedCall` rejects with, so a
caller catching a destroy()-during-await race sees one shape regardless of
which await it landed in. Pinned by
`packages/ink-editor/src/__tests__/project-session-destroy.test.ts`'s
"destroy() during the post-host-IO-await window (#2802)" cases, using a stub
provider whose `renameFile` resolves *after* `destroy()` runs — the #2794
suite only ever exercised the idle-yield window above.

With this closed, "is `ProjectSession` destroy()-safe" now has one real
answer covering both windows — the `scheduleIdleWork` yield (#2794/#2798)
and every host-IO await (#2802) — rather than the idle-window-only claim
above.

### 7.8 Editor groups & the document-type API

The editor area's counterpart to §7.1: the shell owns document *structure*
(groups, tabs, pin state, focus) and renders it purely from two pieces it
hosts — a **document-type registry** and an **editor-groups store** — while
document *content* belongs to registered components. Text documents (CM6) are
one implementation of the contract, not the contract: Compiled Output (#91),
the Story Graph (§4.1), and the Player document (#120) implement the same API.

```ts
interface DocumentRef {            // small and serializable — a tab is one of these
  typeId: string;                  // registered document type, e.g. "ink-file"
  docId: string;                   // type-scoped identity, e.g. "main.ink::intro"
  title: string;                   // tab label
}
interface DocumentViewProps {
  doc: DocumentRef;
  groupId: string;                 // one component instance per (document, group)
  active: boolean;                 // this view is the focused group's active tab
}
interface DocumentTypeDescriptor {
  id: string;
  component: ComponentType<DocumentViewProps>;
}
```

Types register at bootstrap (same discipline as tool windows: duplicate-id and
host-prefix rejection; hosts get the same door in Phase 5 via §8). The shell
never imports feature components (§7.2); anything heavier than the ref —
content, wasm handles, view state — lives behind the registered component,
keyed by `docId` + `groupId`.

**Editor groups** are a flat, ordered list of vertical columns; each group is
a tab strip over DocumentRefs with one active tab. Splitters reuse the dock
pattern. Group structure lives in a shell store (sibling of the layout store):
groups, the focused group, per-group active tab, pin state, splitter sizes.

- **Open/reveal policy.** A plain open (binder click, quick-open,
  `editor.reveal`) focuses an existing tab *wherever it lives* — a document is
  open at most once unless the user explicitly asks for more. Duplicates come
  only from explicit actions: `editor.split` and opening into an explicit
  target group. `editor.reveal` then scrolls/flashes in that view.
- **Split duplicates (VS Code exact).** `editor.split` (Mod-\) duplicates the
  focused group's active tab into a new group immediately to its right and
  focuses it. Two views of one text document live-mirror via the CM6
  sync-dispatch pattern (changes forwarded under an annotation that prevents
  echo; selection/scroll stay per-view). Fragment⇄file overlaps (a symbol tab
  and its file open at once) mirror through the wasm document-handle change
  specs (#122), refreshing from the file where a change can't be mapped.
- **Preview/pin is generic tab behavior.** At most one unpinned (preview) tab
  per group, replaced in place by the next preview open; editing or
  double-clicking pins. (Moved here from the old editor state manager — it
  was never ink-specific.)
- **Collapse.** A group collapses when its last tab closes; the editor area
  always keeps ≥ 1 group.
- **Tab drag (#142).** Tabs drag with the pointer (the §5.1 strip-drag
  gesture: 5px threshold, ghost, Escape cancel, click suppression) — within a
  group to reorder, across groups to move at the insert-indicator gap (pin
  state kept; a duplicate target focuses the existing tab; an emptied source
  collapses).
- **Commands:** `editor.split` (Mod-\), `editor.moveTabRight` /
  `editor.moveTabLeft` (move the focused group's active tab between neighbor
  groups, creating/collapsing groups at the edges), `editor.focusNextGroup`
  (unbound; palette-discoverable).
- **Tiers:** groups keep working in all tiers with no special narrow-tier
  handling — cramped is acceptable; the narrow tier remains a degraded
  presentation (§11).

Text documents ride the wasm **document-handle** API (#122): each mounted view
opens its own handle (`open_document` / `open_fragment`) and every IDE query
routes through that view's DocId — IDE intelligence works in every group
simultaneously, with no active-file choreography and no module-global session
ref. Handles open on mount and close on unmount; backgrounded tabs keep a
cached editor state, rebuilt from the session's authoritative content when it
changed underneath.

Not every document type is handle-backed — the contract demands nothing
beyond the component. **Compiled Output** (#91, the first non-ink-file type)
is a *compile-bound* document: a singleton ref whose component renders a
plain string (`programInkt`) from the store in its own read-only CM6 view,
live-updating on each successful compile and showing a quiet placeholder
before the first one. No wasm document handle, no DocumentSessions slot, no
session binding (§7.6 does not apply — the dump survives `story.stop`, like
the Program Explorer). Custom-rendered types (Story Graph §4.1, Player #120)
follow the same pattern: whatever state they need, they own.

**Non-goals (for now):** group-layout persistence (dock layout persists, #88;
groups reset on reload — future work), nested grids, and horizontal splits.
Vertical columns only.

#### 7.8.1 `documentKey()` identity encoding (invariant)

The "open at most once" policy above and every activeKey/tab-match comparison
in `editor-groups.ts`, `editor-area.tsx`, and `tab-drag.ts` rest entirely on
`documentKey(ref)` producing the same string for the same document and a
different string for any other — `packages/studio-shell/src/document.ts`'s
`DocumentRef` identity function. The encoding is:

```ts
documentKey(ref) = JSON.stringify([ref.typeId, ref.docId]);
```

`JSON.stringify` of a fixed 2-element array is injective — `JSON.parse`
recovers the exact two original values, so two distinct `(typeId, docId)`
pairs can never serialize to the same string, regardless of what characters
either field contains (JSON escapes them, including any embedded NUL). This
replaced an earlier NUL-byte-separated template literal (`` `${typeId}\x00${docId}` ``)
that carried the same defect class #2558 fixed in `ink-editor/src/rename.ts`
(#2733, #2737): a literal NUL byte makes the file register as binary to
`grep`/`rg` without `-a`, silently hiding every match in it — including
`documentKey()`'s own definition — from a repo-wide sweep. Guarded
repo-wide, not just here, by `scripts/check-no-nul-bytes.mjs` (CLAUDE.md
"Rules").

`documentKey()`'s output is **in-memory identity only** — it is never
persisted. `LayoutSnapshot` (`layout-persistence.ts`) carries no tab keys, so
the key's shape can change freely across releases with no migration
concern; only same-session `===` comparisons depend on it.

## 8. Embedder extension API

brink-studio is embedded programmatically (the embedded playground today; RPG Maker MZ
planned). An embedding host already runs its own code in the page and mounts the studio —
so hosts can be allowed to provide their own surfaces (e.g. an **"RPG Maker functions
panel"** that cannot be built into the studio) without anything resembling a plugin
system. The extension point is **mount-time registration into the same registries the
built-ins use** — no dynamic loading, no marketplace, no sandboxing, no separate
extension code path.

### 8.1 Contract

**Landed (#95).** The host-facing documentation is
[embedder-api.md](embedder-api.md); this section is the design contract.

```ts
interface StudioExtensions {
  toolWindows?: ToolWindowDescriptor[];   // §7.1 shape; ids must be "host.<vendor>.<name>"
  commands?: Command[];                   // §6 shape; same id namespacing
  statusBarItems?: StatusBarItem[];       // §7.3 shape; same id namespacing
}
```

Passed once at mount alongside the existing initialization wiring — concretely
`mountStudio(container, { files, entryFile, extensions })`, where `extensions` is the
config or an `(api: StudioApi) => StudioExtensions` factory (for host commands that need
the facade). Installation goes through the registries' host-only `registerHost` doors
(`installStudioExtensions`), after every built-in registration so built-in strip
mnemonics never shift; a rejected install rolls back atomically. Rules:

- **Namespacing:** host ids must carry the `host.<vendor>.` prefix; registration
  validates this and rejects collisions with a clean error. Built-in ids never use the
  prefix.
- **Equal citizens:** host tool windows dock, toggle, drag, persist, and appear in
  strips/palette/hamburger exactly like built-ins. Layout persistence already drops
  unknown ids silently on load (§7.1), which handles a host removing a panel between
  sessions.
- **React components:** host views are React components (the host bundles the studio and
  therefore React). A DOM-mount escape hatch (`mount(el: HTMLElement)`) for non-React
  hosts is a possible later addition, not in scope.

### 8.2 StudioApi facade

**Landed (#95).** Host components receive a **curated facade** via React context
(`useStudioApi()`; also returned from `mountStudio` for host code outside the React
tree) — never the raw Zustand store (consumer-first API principle: store internals stay
free to change):

```ts
interface StudioApi {
  insertText(text: string): void;                 // at cursor in the focused editor view
  dispatch(commandId: string, args?: unknown): boolean;
  notify(n: NotificationInput): NotificationHandle;  // §7.5
  select<T>(sel: (s: StudioPublicState) => T): T;
  subscribe<T>(sel: (s: StudioPublicState) => T, cb: (value: T) => void): () => void;
  getFiles(): Record<string, string>;             // #154 pull egress — session file snapshot
  getDirtyFiles(): string[];                      // per-file detail behind dirtyFiles
  getOrphanedFiles(): string[];                   // #2371 — externally-deleted, buffer kept
  getStoryBytes(): Uint8Array | null;              // #2391 — latest compile's bytes, or null
}
```

`StudioPublicState` is an explicit, versioned subset (`version: 1`): active file,
cursor/element info, diagnostics summary, compile status, story session status (§7.6),
and the `dirtyFiles` count (#154 — files diverging from the last-saved/last-notified
baseline). Anything a host needs that isn't in it is a deliberate API addition, not a
store leak. The full field list and versioning policy live in
[embedder-api.md](embedder-api.md).

**File-content egress (#154, closing #137).** Hosts that own project persistence (RPG
Maker MZ writing `data/brink/**`) get studio→host file sync as: a debounced, batched
`onFilesChanged(changes: FileChange[])` mount option (each change names the file, a kind
`"modified" | "created" | "deleted"` — deleted designed-in, currently unreachable — and
the content); the `getFiles()` pull; `file.save` (Mod-S) / `file.saveAll` commands that
flush editor text and deliver pending notifications immediately (and degrade to an
internal flush + info notification without a host hook); and the dirty summary above.
Every mutation path — CM6 edit flushes, binder structural ops, search replace,
`file.new` — routes through one shared notify seam (`ProjectSession`'s `FileChangeHub`),
so omission is structurally impossible. File *contents* never enter `StudioPublicState`
(they are big and change per keystroke — the reference-stability contract forbids them);
push/pull ride the dedicated surfaces instead. Host-facing documentation:
[embedder-api.md](embedder-api.md) "File egress".

**Motivating example (Track B synergy):** the RPG Maker functions panel renders the
host-capability manifest the host already registers via `set_host_manifest`
(see [host-capability-manifest.md](host-capability-manifest.md)) — surfacing the metadata
the manifest carries (signatures, doc comments, semantic types) — with click-to-insert of
call sites only (`~ fn(args)`) through `insertText`. The panel browses what the host
already provides; it never inserts `EXTERNAL` declarations — those live in the story (or
a dedicated declarations file). The shipped `createExampleExtension` (mounted by the
playground, which registers a pretend manifest via the `hostManifest` mount option;
`?ext=none` disables the panel) is the worked example of exactly this shape.

### 8.3 Timing

The registries are written to these contract shapes **from Phase 1** (namespaced ids,
descriptor discipline, command-only actions) so the public exposure is a thin door, not a
retrofit. The exposure itself — `StudioExtensions` mount config, `StudioApi`,
`StudioPublicState`, and their documentation — landed in **Phase 5** (#95), once docking
and persistence were stable enough to promise hosts a non-churning contract.

## 9. Migration plan

Each phase lands independently; the studio remains shippable after every phase.

- **Phase 0 — this spec.** Review, revise, approve.
- **Phase 1 — shell skeleton.** `studio-shell` package: regions, docks+strips
  (click-to-toggle only), `ShellLayoutSlice`, tool-window + status-bar registries,
  `CommandRegistry` + global key handler resolving through the keymap layer (defaults +
  user-override JSON, §6) + palette on the shared overlay primitive (§7.7), and the
  `Location`/`editor.reveal` navigation protocol (§6.1, source + symbol resolvers).
  Existing components registered as-is:
  Binder/Search-less left dock, Player right, State View right-end, Program Explorer
  bottom. App.tsx's pane logic is deleted; tiers reimplemented per §5.3. *Visual parity is
  not a goal; structural correctness is.*
- **Phase 2 — fill the regions.** Problems tool window (from existing diagnostics),
  Output log, status bar segments wired to commands, quick-open over binder items,
  maximize (replacing `playerFullscreen`), hamburger menu generated from the registry,
  story session model extracted from `PlayerSlice` with lifecycle commands (§7.6).
- **Phase 3 — drag & persistence.** Strip-icon drag-to-re-dock, layout persistence,
  notification service (§7.5) replacing Toast.
- **Phase 4 — editor groups.** Split support in the editor area (vertical first),
  per-group tab bars, `Mod-\` split command, and **component-based document support** —
  the §7.8 document-type API, implemented by both text documents (CM6; the Compiled
  Output `.inkt` tab moves out of the Program Explorer here, #91) and custom-rendered
  documents (consumed by the Story Graph in Phase 6). Sequenced behind #122 (wasm
  document handles), so text documents ride per-view DocIds instead of the active-file
  singleton. Groups + the document API + the ink-file type landed as #90; file creation
  moved from the old tab-bar "+" to the palette `file.new` command. Compiled Output
  landed as #91 (the first non-ink-file document type). The Player document landed as
  #120: the `player` type, the bootstrap two-up (entry file left, player in a right
  split, focus on the editor), `story.openPlayer`, the player tool window removed
  (State View takes right/start), and `editor.maximizeGroup` (§5.4) replacing the
  player-specific fullscreen.
- **Phase 5 — polish, theme & embedder API.** Semantic token layer, light theme, CSS
  decomposition completes (all landed, #92), Search tool window (landed, #94 — see
  §4), Settings document
  (landed, #93: theme picker, keymap-override JSON, the external-check severity flag —
  see §4), embedder extension API exposure (§8: `StudioExtensions` mount config,
  `StudioApi`, `StudioPublicState`).
- **Phase 6 — Story Graph.** The story-graph extraction query (analyzer/IDE layer,
  wasm-exposed) landed as #96; the custom-rendered document (§4.1) landed as #97:
  the `story-graph` document type (react-flow canvas, dagre top-down auto-layout off
  the render path), expand/collapse with collapse-time edge aggregation,
  click-to-jump via `editor.reveal`, the live story overlay from `debugState`, and
  `story.openGraph` (palette/hamburger).

What is **kept** throughout: Zustand slices (editor/compile/documents/session/binder —
the old tabs slice dissolved into the shell's editor-groups store in Phase 4),
the CM6 stack and its keybindings, player/debug domain logic, binder tree logic,
the wasm data flow, `useTier`.

## 10. Resolved questions (2026-06-10)

All three open questions were resolved with the user; details in the decision log
("Studio shell spec: open questions resolved").

1. **Program Explorer: split.** The structured tables stay a tool window; the raw `.inkt`
   dump becomes the read-only **Compiled Output** editor document in Phase 4 (§4, §9).
   Rationale: tables are glanceable lookup (tool-window behavior); the dump is a long
   searchable text artifact that earns CM6 search/folding as a document.
2. **Menu bar: none — registry-driven hamburger instead.** A single icon at the top of
   the left strip opens a grouped menu generated from the command registry (§6, Phase 2).
   Rationale: menus exist for discoverability; an in-page bar costs vertical space and is
   wrong in embeds, while the hamburger patches the palette's discoverability hole
   nearly free.
3. **Keybindings: keymap layer + JSON override, no UI.** Phase 1's key handler resolves
   through a keymap table (registry defaults merged with a user-override JSON, no editing
   UI; §6). Full keymap UI remains out of scope. Rationale: with the layer specced anyway
   the override merge is nearly free — cheap now, annoying later.

## 11. Non-goals

- Free-form docking, floating tool windows, multi-window. (Floating/undocked tool windows
  are a JetBrains feature we deliberately skip — edge docks only.)
- A third-party **plugin** system: dynamic loading, a marketplace, sandboxing, untrusted
  code. The embedder extension API (§8) is a build-time contract for the host that mounts
  the studio — the host is trusted code that already owns the page.
- Mobile-first redesign — the narrow tier remains a degraded presentation, not a product.
- Replacing CodeMirror, the store, or the wasm pipeline.
