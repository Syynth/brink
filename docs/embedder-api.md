# Embedder API — mounting and extending brink-studio

**Status:** landed (issue #95, shell Phase 5). The contract here is the public
embedding surface of `@brink/studio`; the design rationale lives in
[studio-shell-spec.md §8](studio-shell-spec.md#8-embedder-extension-api) and
the decision log ("embedder extension API").

brink-studio is embedded programmatically — the embedded playground today,
RPG Maker MZ planned. The embedding host already runs its own code in the
page, so it can be trusted with extension points without anything resembling
a plugin system: **no dynamic loading, no marketplace, no sandboxing**. Host
surfaces register at mount time into the same registries the built-ins use.

## Mounting

```ts
import { mountStudio } from "@brink/studio";

const handle = await mountStudio(document.getElementById("app")!, {
  files: { "main.ink": "-> start\n=== start ===\nHello.\n-> END\n" },
  entryFile: "main.ink",
  extensions: myExtensions, // optional, see below
});

// later:
handle.api.dispatch("compile.run");
handle.unmount(); // unmount React, dispose editor views, free the wasm session
```

`mountStudio` performs the whole bootstrap: wasm init, project session,
store, registries, navigation wiring, default layout. It is the only way to
mount the studio — the standalone playground app is itself a caller.

## `StudioExtensions` — host surfaces (spec §8.1)

Passed once at mount. Either a plain config or a factory
`(api: StudioApi) => StudioExtensions` when host commands need the facade:

```ts
import type { StudioExtensions } from "@brink/studio";

const myExtensions: StudioExtensions = {
  toolWindows?: ToolWindowDescriptor[];   // §7.1 shape
  commands?: Command[];                   // §6 shape
  statusBarItems?: StatusBarItem[];       // §7.3 shape
};
```

### Namespacing rules

- Every host id MUST be `host.<vendor>.<name>` — non-empty vendor and name
  segments (`host.example.functions`, `host.rmmz.events`). Registration
  validates this and throws a clean error otherwise.
- Built-in ids never carry the `host.` prefix (the registries reject
  built-ins claiming it).
- Collisions (two registrations of the same id) are rejected. A rejected
  install rolls back everything it already registered — the registries are
  left untouched.

### Equal citizens

Host tool windows get every shell behavior for free: they dock, toggle,
drag-to-re-dock, persist placement, and appear in the strips, the command
palette (via their generated `view.toggle.<id>` command), and the hamburger
menu exactly like built-ins. Host windows register after all built-ins, so
built-in Mod-1…N strip mnemonics never shift.

Layout persistence drops unknown tool-window ids silently on load (spec
§7.1) — when a host removes a panel between sessions, the stored layout
referencing it loads cleanly.

Host views are React components (the host bundles the studio and therefore
React). A DOM-mount escape hatch for non-React hosts is a possible later
addition, not in scope.

## `StudioApi` — the host facade (spec §8.2)

Host components receive a curated facade via React context — **never the raw
Zustand store**, so store internals stay free to change:

```ts
import { useStudioApi } from "@brink/studio";

function MyHostPanel() {
  const api = useStudioApi();
  // ...
}
```

```ts
interface StudioApi {
  /** Insert at the cursor in the focused editor view (replaces selection). */
  insertText(text: string): void;
  /** Command dispatch (§6); returns true if the command ran. */
  dispatch(commandId: string, args?: unknown): boolean;
  /** Shell notification service (§7.5). */
  notify(n: NotificationInput): NotificationHandle;
  /** Read from the current public state. */
  select<T>(sel: (s: StudioPublicState) => T): T;
  /** Observe a selected value (Object.is change detection). */
  subscribe<T>(sel: (s: StudioPublicState) => T, cb: (value: T) => void): () => void;
}
```

The same facade is returned from `mountStudio` (`handle.api`) for host code
living outside the React tree.

**Navigation is `dispatch`,** not extra API surface: host panels navigate
with `dispatch("editor.reveal", location)` using the §6.1 `Location` shapes
(`{ kind: "source", file, span }` or `{ kind: "symbol", name: "knot.stitch" }`).

## `StudioPublicState`

An **explicit, versioned subset** of studio state. Every field is a
deliberate exposure; anything a host needs that isn't here is an API
addition, not a store leak.

```ts
interface StudioPublicState {
  version: 1;                                        // bumped on breaking changes
  activeFile: string | null;                         // focused editor's file path
  cursor: { line: number; col: number };             // 1-based
  element: { type: string; depth: number } | null;   // cursor-line element info
  diagnostics: { errors: number; warnings: number }; // latest compile summary
  compileStatus: "ok" | "errors";
  sessionStatus:                                     // story session (§7.6)
    | "none" | "running" | "awaiting-choice" | "done" | "ended" | "error";
}
```

`element.type` is the stable element-type name ("KnotHeader",
"NarrativeText", "Choice", …). The derived state is reference-stable between
relevant store changes, so identity selectors don't spuriously fire.

### Versioning policy

- `version` is a literal on the state; it bumps only on **breaking** shape
  changes (removed/renamed/retyped fields). Additive fields don't bump it.
- The `StudioExtensions` / `StudioApi` shapes follow semver through the
  package version; the registries were written to these contract shapes from
  Phase 1, so churn risk is low by construction.

## Worked example — the "host functions panel" use case

`createExampleExtension` (shipped in `@brink/studio`, mounted by the
playground; `?ext=none` loads without it) validates the RPG Maker MZ story:
a panel listing the host's external vocabulary with click-to-insert.

```tsx
import { mountStudio, useStudioApi, type StudioApi, type StudioExtensions } from "@brink/studio";

function HostFunctionsPanel() {
  const api = useStudioApi();
  return (
    <button
      onClick={() => {
        api.insertText('EXTERNAL has_item(item_id)\n~ has_item(item_id)\n');
        api.notify({ severity: "info", source: "my host", message: "Inserted has_item" });
      }}
    >
      has_item(item_id)
    </button>
  );
}

const extensions = (api: StudioApi): StudioExtensions => ({
  toolWindows: [{
    id: "host.example.functions",
    title: "Host Functions",
    icon: myIcon,
    defaultPlacement: { dock: "right", section: "end" },
    defaultOpen: false,
    component: HostFunctionsPanel,
  }],
  commands: [{
    id: "host.example.revealStart",
    title: "Example Host: Go to Story Entry",
    run: () => {
      api.dispatch("editor.reveal", { kind: "source", file: "main.ink", span: { start: 0, end: 0 } });
    },
  }],
});

await mountStudio(el, { files, entryFile: "main.ink", extensions });
```

A real host would render the host-capability manifest it already registers
via `set_host_manifest` (see
[host-capability-manifest.md](host-capability-manifest.md) — Track B): the
manifest gives the analyzer/IDE diagnostics and hover for host verbs, and
the same vocabulary feeds the panel's list, with `insertText` closing the
authoring loop.

## What is deliberately NOT exposed

- **The Zustand store.** `@brink/studio` exports no `createStudioStore`, no
  `useStudioStore`, no `StudioState`. Hosts observe `StudioPublicState`
  only. (The internal workspace packages `@brink/studio-store` /
  `@brink/studio-ui` are not part of the public surface.)
- **Registry instances.** Hosts register through the `StudioExtensions`
  config; they don't get the registries to mutate later. Dynamic
  registration after mount is out of scope until a host needs it.
- **The wasm `EditorSession` / project internals.** The compiler-facing
  `initWasm`/`compile`/`StoryRunnerHandle` bindings remain exported for
  hosts that drive a compiled story directly (the docs/book examples); they
  carry no studio UI state.
