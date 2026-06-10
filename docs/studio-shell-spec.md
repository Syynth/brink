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
- **Tool window** — a registered, dockable view (Binder, Player, Problems, …). Tool windows
  can live in any dock section, can be toggled, and remember size/placement. They can never
  enter the editor area.
- **Editor area** — the center region. Hosts **editor groups** (tabbed documents,
  splittable). Documents are files or read-only document views (e.g. the `.inkt` dump).
  Editors can never enter a dock.
- **Command** — a named action (`player.restart`, `view.toggle.problems`). Keybindings,
  palette entries, strip clicks, menu items, and buttons all dispatch commands; nothing
  binds a key directly to a function.

## 3. Region layout

```
┌─────────────────────────────────────────────────────────────┐
│ ┌─┐ ┌─────────────────────────────────────────────────┐ ┌─┐ │
│ │s│ │ left dock │   editor area (groups)  │ right dock│ │s│ │
│ │t│ │  start    │  ┌─tabs────┐┌─tabs────┐ │   start   │ │t│ │
│ │r│ │           │  │         ││         │ │  (Player) │ │r│ │
│ │i│ ├───────────┤  │  ink    ││  ink    │ ├───────────┤ │i│ │
│ │p│ │ left dock │  │  editor ││  editor │ │ right dock│ │p│ │
│ │ │ │  end      │  └─────────┘└─────────┘ │   end     │ │ │ │
│ └─┘ └─────────────────────────────────────────────────┘ └─┘ │
│      ┌────────────────────────────────────────────────┐     │
│      │ bottom dock (start | end)   [strip along edge] │     │
│      └────────────────────────────────────────────────┘     │
│ status bar                                                  │
└─────────────────────────────────────────────────────────────┘
```

Rules:

1. **The editor area always exists and is always center.** It cannot be collapsed (the
   "player fullscreen" mode is a tool-window maximize, not an editor removal — see §5.4).
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
| **Binder** | tool window | left dock, start | [Binder.tsx](../packages/studio-ui/src/Binder.tsx) (928 LOC) | Open by default. Component gets decomposed (tree / selection / DnD / context menu) during migration, not before. |
| **Player** | tool window | right dock, start | [PlayerPane.tsx](../packages/studio-ui/src/PlayerPane.tsx) | Open by default (Inky two-up). Maximize replaces today's `playerFullscreen`. |
| **State View** (debugger) | tool window | right dock, end | [StateView.tsx](../packages/studio-ui/src/StateView.tsx) | Closed by default; opens when a story is running and the user toggles it. Pairs vertically with Player. |
| **Problems** | tool window | bottom dock, start | *new* (data exists in `CompileSlice` diagnostics) | Clickable diagnostics list → jumps editor to location. Status-bar error/warning segment opens it. |
| **Output / compile log** | tool window | bottom dock, end | *new* | Compile timings, wasm/runtime errors that aren't source diagnostics. Replaces nothing; today this information is dropped. |
| **Search** | tool window | left dock, start | *new, later phase* | Project-wide find/replace. In the strip from day one only if trivially stubbed; otherwise added when implemented. |
| **Program Explorer** | tool window | bottom dock or right dock (user-movable) | [ProgramView.tsx](../packages/studio-ui/src/ProgramView.tsx) | Stays a tool window for now (matches its current sidebar life). **Open question §9.1:** promote the raw `.inkt` dump to a read-only editor document. |
| **Story transcript** | tool window | bottom dock | *future* | Append-only transcript view; listed to validate the model, not scheduled. |
| Ink files | editor document | editor area | [EditorPane.tsx](../packages/studio-ui/src/EditorPane.tsx) + CM6 | Tabbed via [FileTabBar.tsx](../packages/studio-ui/src/FileTabBar.tsx), which becomes the per-group tab bar. |

The **Toast** system is replaced by a shell notification service (bottom-right, stacking,
command-actionable) in a later phase; until then the existing component mounts in the shell
unchanged.

## 5. Behavior

### 5.1 Strips

- One icon per tool window, in section order (start section icons at the strip's start).
- Click: toggle that tool window (open-in-section / collapse).
- Drag: ghost icon follows cursor; valid drop targets are the six sections; drop re-docks.
  Strips highlight the target section on hover. (Drag ships in Phase 3, not MVP — §8.)
- Icons show badges where meaningful (Problems: error count).

### 5.2 Keyboard

JetBrains-style numbered mnemonics, dispatched through the command registry:

- `Mod-1…9` — toggle tool windows by a stable, user-visible ordering (Binder `Mod-1`,
  Player `Mod-2`, Problems `Mod-3`, …; shown in strip tooltips).
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

### 5.4 Player maximize

Today's `playerFullscreen` becomes a general "maximize tool window" command: the tool
window temporarily covers the editor area; `Escape` or the command restores the previous
layout. Only Player is expected to use it initially, but it's a shell feature, not a
player feature.

## 6. Command system

A `CommandRegistry` in the shell package:

```ts
interface Command {
  id: string;             // "view.toggle.problems", "player.restart"
  title: string;          // palette display: "View: Toggle Problems"
  keybinding?: string;    // "Mod-3" — single default binding, user remap is future work
  when?: (state: StudioState) => boolean;  // enablement, evaluated at dispatch & palette
  run(ctx: CommandContext): void | Promise<void>;
}
```

- Commands are registered at startup by each feature module (player commands by the player
  slice owner, view-toggle commands generated from the tool-window registry).
- One global key handler resolves keybindings → command dispatch. No component-level
  `onKeyDown` for chrome behavior.
- The **palette** is a shell overlay listing enabled commands, fuzzy-filtered, showing
  keybindings. **Quick-open** reuses the same overlay component with a different provider
  (binder items instead of commands).
- Buttons/menus/strip icons call `dispatch(commandId)` — never feature functions directly —
  so the palette is automatically complete.

## 7. Shell state, registry, and theming

### 7.1 View registry

```ts
interface ToolWindowDescriptor {
  id: string;                       // "binder", "player", "problems"
  title: string;
  icon: ReactNode;
  defaultPlacement: { dock: "left" | "right" | "bottom"; section: "start" | "end" };
  defaultOpen: boolean;
  badge?: (state: StudioState) => number | undefined;
  component: ComponentType;
}
```

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
- Introduce a **semantic token layer**: components reference only semantic tokens
  (`--bs-editor-bg`, `--bs-dock-bg`, `--bs-strip-icon-active`, `--bs-status-fg`, …);
  a theme file maps palette → semantic tokens. Current Catppuccin Mocha becomes the first
  theme; a light theme validates the layer (and is the test that no component hardcodes a
  color).
- Split the monolithic [studio.css](../packages/studio-ui/src/studio.css) per component as
  components are touched during migration — not as a big-bang rewrite.
- Visual direction per Zed: 1px hairline borders only where regions meet, no boxes-in-boxes,
  strip icons monochrome with an accent for the active state, density closer to a writing
  tool than to JetBrains.

## 8. Migration plan

Each phase lands independently; the studio remains shippable after every phase.

- **Phase 0 — this spec.** Review, revise, approve.
- **Phase 1 — shell skeleton.** `studio-shell` package: regions, docks+strips
  (click-to-toggle only), `ShellLayoutSlice`, tool-window + status-bar registries,
  `CommandRegistry` + global key handler + palette. Existing components registered as-is:
  Binder/Search-less left dock, Player right, State View right-end, Program Explorer
  bottom. App.tsx's pane logic is deleted; tiers reimplemented per §5.3. *Visual parity is
  not a goal; structural correctness is.*
- **Phase 2 — fill the regions.** Problems tool window (from existing diagnostics),
  Output log, status bar segments wired to commands, quick-open over binder items,
  maximize (replacing `playerFullscreen`).
- **Phase 3 — drag & persistence.** Strip-icon drag-to-re-dock, layout persistence,
  notification service replacing Toast.
- **Phase 4 — editor groups.** Split support in the editor area (vertical first),
  per-group tab bars from FileTabBar, `Mod-\` split command. Until this phase the editor
  area is a single group — the shell API is written for groups from day one so this is
  additive.
- **Phase 5 — polish & theme.** Semantic token layer, light theme, CSS decomposition
  completes, Search tool window.

What is **kept** throughout: Zustand slices (editor/compile/tabs/player/binder),
the CM6 stack and its keybindings, player/debug domain logic, binder tree logic,
the wasm data flow, `useTier`.

## 9. Open questions

1. **Program Explorer's final form.** Tool window (status quo) vs. read-only editor
   document ("compiled output" tab, like a disassembly view in a real IDE). Document-tab
   fits the model better (it's content you read, not a tool you glance at) but means
   building read-only document support in Phase 4 rather than reusing the tool window for
   free. Default if undecided: stays a tool window through Phase 3; revisit at Phase 4.
2. **Menu bar.** Neither tier of the current app has one; Zed and Inky do (native).
   A web app can have a slim in-page menu bar or rely entirely on palette + strips.
   Default: no menu bar; palette is the command surface. Revisit if discoverability
   suffers.
3. **Keybinding remapping.** Registry stores a single default binding per command; user
   remapping is out of scope for all phases here. Flagged so the `Command` shape doesn't
   preclude it.

## 10. Non-goals

- Free-form docking, floating tool windows, multi-window. (Floating/undocked tool windows
  are a JetBrains feature we deliberately skip — edge docks only.)
- A plugin/extension system. The registries are internal APIs, not public extension points.
- Mobile-first redesign — the narrow tier remains a degraded presentation, not a product.
- Replacing CodeMirror, the store, or the wasm pipeline.
