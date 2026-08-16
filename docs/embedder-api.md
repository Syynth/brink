# Embedder API — mounting and extending brink-studio

**Status:** landed (issue #95, shell Phase 5). The contract here is the public
embedding surface of `@brink-lang/studio`; the design rationale lives in
[studio-shell-spec.md §8](studio-shell-spec.md#8-embedder-extension-api) and
the decision log ("embedder extension API").

brink-studio is embedded programmatically — the embedded playground, and
RPG Maker MZ as the first real external host
(e.g. a game-engine plugin that owns the project on disk). The
embedding host already runs its own code in the page, so it can be trusted
with extension points without anything resembling a plugin system: **no
dynamic loading, no marketplace, no sandboxing**. Host surfaces register at
mount time into the same registries the built-ins use.

## Mounting

```ts
import { mountStudio } from "@brink-lang/studio";

const handle = await mountStudio(document.getElementById("app")!, {
  files: { "main.ink": "-> start\n=== start ===\nHello.\n-> END\n" },
  entryFile: "main.ink",
  extensions: myExtensions,    // optional, see below
  hostManifest: myManifest,    // optional: the host-capability manifest, see below
  onFilesChanged: persist,     // optional: file-content egress, see below
  wasmLocation: myWasmUrl,     // optional: explicit wasm binary location, see below
});

// later:
handle.api.dispatch("compile.run");
handle.unmount(); // unmount React, dispose editor views, free the wasm session
```

`mountStudio` performs the whole bootstrap: wasm init, project session,
store, registries, navigation wiring, default layout. It is the only way to
mount the studio — the standalone playground app is itself a caller.

`entryFile` is only the fallback for a **configless** project (issue #2331,
ruled 2026-08-07 "`[project] entry` beats `mountStudio`'s `entryFile`"): if
the project's `brink.toml` sets `[project] entry` to a path that exists in
`files`, that value wins for both compilation and the initial tab, and
`entryFile` is never consulted again. Pass `entryFile` for the configless
case (no `brink.toml`, or one that doesn't set `entry`) and whenever
`brink.toml` might not have loaded yet — it must still name a real key of
`files`.

`hostManifest` registers the host-capability manifest
([host-capability-manifest.md](host-capability-manifest.md)) before the
first compile, so manifest-driven diagnostics (literal type mismatches,
arity disagreements — toggleable in Settings via the external-check flag),
hover, and completions are live from the start. The host owns this data; the
wasm session itself stays unexposed.

`wasmLocation` is forwarded to `initWasm` (a URL/path string, `Request`, or
a precompiled `WebAssembly.Module`). By default the `.wasm` binary resolves
relative to the module URL — which cannot work inside an IIFE plugin bundle
(no usable `import.meta.url`; e.g. an RPG Maker MZ plugin). Pass the
location explicitly there instead of pre-calling `initWasm` and relying on
its double-init guard.

**Old-engine hosts:** the mount bootstrap feature-detects the Chromium-88
`document.adoptedStyleSheets` shape (a frozen array — NW.js as shipped by
RPG Maker MZ) and installs a mutable wrapper syncing through the native
setter, so CodeMirror's style injection works without a host-side shim.
Modern browsers fail the detect and take zero overhead.

## File egress — studio → host persistence (issue #154)

Disk → studio is the `files` mount option. Studio → host is the egress
surface: hosts that own the project on disk (RPG Maker MZ writing
`data/brink/**/*.ink`; reference consumer:
a host that persists files to disk) receive edits back out of the
session instead of losing them on unmount.

```ts
type FileChange = {
  path: string;                                  // e.g. "main.ink"
  type: "modified" | "created" | "deleted";
  content?: string;                              // full text; omitted for "deleted"
};
```

- **Push — `onFilesChanged(changes: FileChange[])` (mount option).** Called
  with batched changes, debounced ~500 ms trailing; pending changes flush
  immediately on `file.save` / `file.saveAll` and on `unmount()`. Every
  mutation path reports: editor typing, binder structural ops
  (move/reorder/promote/demote and undo), search replace (per-match and
  replace-all), and `file.new` (as `"created"`). `"deleted"` is part of the
  contract and reachable in production: the binder's Delete action
  (`BinderContextMenu`, gated on `ProjectSession.canDeleteFiles`) calls
  `ProjectSession.deleteFile`, and `ProjectSession.renameFile` reports one
  for the old path of every rename/move alongside a `"created"` for the
  new path.
- **Pull — `api.getFiles(): Record<string, string>`.** A snapshot of every
  session file's current content, sorted by path.
- **Save commands.** `file.save` (default keybinding Mod-S; palette "File:
  Save") flushes the focused editor's text to the session — bypassing the
  editor's own debounce — and delivers pending host notifications
  immediately; `file.saveAll` ("File: Save All") does it for every dirty
  file. Without an `onFilesChanged` hook both still flush internally,
  clear dirty state, and raise an info notification ("Saved main.ink") —
  they never error in the standalone playground. With a host save in
  flight (the overlay contract, `requestSave`; docs/desktop-shell-spec.md
  line 64), a path whose content no longer matches the snapshot taken
  before the save started is re-checked against what the provider actually
  wrote (`ProjectSession.readProviderFile`) rather than trusting that
  snapshot: a genuine mid-write divergence stays dirty and raises a
  "…changed while saving — still unsaved" warning notification (issue
  #2426), while a write that was merely queued behind another in-flight one
  and legitimately caught up to a later edit is NOT warned about (issue
  #2435) — `file.saveAll`'s "Saved N files" count reflects only the
  confirmed subset. Dispatchable as `api.dispatch("file.save")` (ids
  exported as `FILE_SAVE_COMMAND_ID` / `FILE_SAVE_ALL_COMMAND_ID`).
- **Dirty state.** `StudioPublicState.dirtyFiles` is the count of files
  whose session content diverges from the *baseline* — the content last
  loaded from the host (mount `files`, external changes) or last synced to
  it. A path only re-baselines when a save's write is confirmed — against
  the provider's own written content, not just a pre-save snapshot — to
  have persisted that path's current content; a path with a genuine
  mid-write divergence is not synced by that save and keeps contributing to
  the count (issues #2426/#2435). This requires `FileProvider.readFile` to
  report PERSISTED content — never content a `requestSave` merely staged: an
  implementation that mirrors edits straight into the store `readFile`
  answers from makes the disk-confirmation check vacuously pass every time,
  turning the #2426 guard into a permanent no-op. Use it to warn before
  `unmount()`/reload would discard edits. Per-file detail is
  `api.getDirtyFiles(): string[]`. File contents deliberately never enter
  `StudioPublicState` (they are big and change per keystroke — the
  reference-stability contract); use the push/pull surfaces above.
- **Confirm and retire in ONE synchronous step (issue #2455).** Any save
  path that re-baselines must perform the read that CONFIRMS what the write
  persisted and the `markFilesSaved` call that RETIRES those paths with no
  `await` between them: re-read the session content immediately before
  marking, never carry a snapshot taken before an `await` across it. A
  snapshot is stale the moment the path yields — an edit landing in that
  window was never written, so retiring it drops the author's work with no
  warning at all, which is strictly worse than the false-positive "changed
  while saving" warning the disk-confirmation check exists to avoid. The
  rule was learned three times over (`OverlayPersistence.saveDirty` #2417,
  `file.save`/`file.saveAll` #2426, and again inside PR #2447, where a
  pre-`await` snapshot reused after the `readProviderFile` round trip was
  caught only in review). It is pinned for every save path by
  `packages/brink-studio/src/__tests__/save-retire-invariant.test.ts`, which
  drives each one with an edit injected after every session read and fails
  if any path retires content the provider never persisted — extend that
  suite's `SAVE_PATHS` when adding a save path rather than writing another
  race test for one window. Enrolment is itself enforced, not merely
  instructed (issue #2480): every production `markFilesSaved`/`markAllSaved`
  call site must carry a `// SAVE-PATH …` (or `SAVE-PATH-EXEMPT …`) marker
  comment naming the save-path id(s) that sweep it, and
  `packages/brink-studio/src/__tests__/save-path-enrolment.test.ts` scans
  `packages/*/src` for every real call site, failing on one with no marker,
  a marker naming an id no driver sweeps, or a file the scan finds that its
  `SCANNED_FILES` list does not know about. The ids live in
  `packages/brink-studio/src/__tests__/save-paths.ts`, which types
  `SAVE_PATHS` and is asserted to match it entry for entry — so adding a save
  path means three edits that fail loudly until all three are made: the id,
  the `SAVE_PATHS` driver, and the marker above the call. Two gaps flagged
  by #2510's review are hardened as of #2515: the scan's roots are derived
  from `pnpm-workspace.yaml`'s `packages:` globs
  (`packages/brink-studio/src/__tests__/workspace-roots.ts`) rather than a
  hand-typed `packages/` assumption, and every non-exempt id must name
  exactly one call site, so a new call site cannot enrol by reusing an id
  already claimed elsewhere. Neither closes the remaining, harder gap —
  proving the `SAVE_PATHS` driver behind an id actually exercises that exact
  call site at runtime — which needs call-site-level instrumentation and is
  intentionally left open (#2515).

  **Enrolment blind spot — Rust-side save paths (issue #2545).** The enrolment
  guard at `packages/brink-studio/src/__tests__/save-path-enrolment.test.ts`
  derives its scan roots from `pnpm-workspace.yaml`'s `packages:` globs,
  which expands to directories listed in that workspace configuration. The
  desktop shell's Rust crate (`packages/brink-desktop/src-tauri`) is
  deliberately its **own** cargo workspace, excluded from the pnpm globs by
  design — so the enrolment scan can never reach any Rust-side code. Today,
  no save/retire path exists in `src-tauri` (no `markFilesSaved`/`markAllSaved`
  analog on the Rust side; only filesystem operations like read/write/delete).
  If a future Rust-side save path is added that calls a hypothetical retirement
  method, it will enrol **nowhere** in `SAVE_PATHS` and the guard will report
  nothing wrong — a silent gap in exactly the invariant this guard was built to
  protect. Either add a parallel enrolment guard in `src-tauri`'s own test
  suite at that time (option a), or document the gap explicitly here so a
  future save-path author isn't assumed to be covered (option b, currently
  favored as no Rust-side path exists today).
- **Orphaned files (2026-08-07 decision, "keep the view, mark orphaned").**
  When a file open in the editor is deleted externally, the session drops
  the file but the open view keeps its buffer — never auto-closed — marked
  "deleted on disk" and dirty; `file.save`/`file.saveAll` recreates it (on
  disk for a host-save provider, in the session either way). Per-file
  detail is `api.getOrphanedFiles(): string[]`, for a host to render a tab
  badge or strikethrough; cleared by a save or by the file reappearing on
  disk.

### Save loop example (host-persisted project, RPG Maker MZ shaped)

```ts
import { mountStudio, type FileChange } from "@brink-lang/studio";

const projectDir = "data/brink/";

const handle = await mountStudio(overlayEl, {
  files: readProjectFromDisk(projectDir),        // disk → studio
  entryFile: "main.ink",
  wasmLocation: pluginWasmUrl,                   // IIFE plugin bundle: explicit location
  onFilesChanged: (changes: FileChange[]) => {   // studio → disk (autosave)
    for (const change of changes) {
      if (change.type === "deleted") {
        fs.unlinkSync(projectDir + change.path);
      } else {
        fs.writeFileSync(projectDir + change.path, change.content!);
      }
    }
    game.reloadStorySessions();                  // hot-reload the running game
  },
});

// Closing the overlay: edits are already persisted (autosave above), but a
// dirty check guards against a teardown racing the debounce —
if (handle.api.select((s) => s.dirtyFiles) > 0) {
  handle.api.dispatch("file.saveAll");           // flushes + delivers immediately
}
handle.unmount();                                // also flushes pending changes
```

## `StudioExtensions` — host surfaces (spec §8.1)

Passed once at mount. Either a plain config or a factory
`(api: StudioApi) => StudioExtensions` when host commands need the facade:

```ts
import type { StudioExtensions } from "@brink-lang/studio";

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
import { useStudioApi } from "@brink-lang/studio";

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
  /** Snapshot of every session file's content (pull egress, #154). */
  getFiles(): Record<string, string>;
  /** Files diverging from the last-saved/notified baseline (#154). */
  getDirtyFiles(): string[];
  /** Files deleted externally while a kept editor buffer survives (#2371). */
  getOrphanedFiles(): string[];
  /** The latest successful compile's story bytes, or `null` if none (#2391). */
  getStoryBytes(): Uint8Array | null;
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
  dirtyFiles: number;                                // unsaved-file count (#154; additive,
                                                     // so version stays 1)
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

`createExampleExtension` (shipped in `@brink-lang/studio`, mounted by the
playground; `?ext=none` loads without it) validates the RPG Maker MZ story:
a panel that browses the external vocabulary the host already provides. The
host registers its capability manifest at mount (`hostManifest`), the panel
renders that same manifest's metadata — signature, doc comment, kind — and
click inserts **only a call site** (`~ fn(args)`) at the cursor. It never
inserts `EXTERNAL` declarations: the host functions are already declared in
the story (or a dedicated declarations file); the panel's job is browsing
the catalog, not declaring it.

```tsx
import {
  mountStudio,
  useStudioApi,
  type HostManifest,
  type StudioApi,
  type StudioExtensions,
} from "@brink-lang/studio";

// The host's vocabulary — the same object feeds the analyzer (diagnostics,
// hover, completion) and the panel below.
const manifest: HostManifest = {
  types: [{ name: "item_id", base: "string" }],
  externals: [
    {
      name: "has_item",
      params: [{ name: "item", ty: "item_id" }],
      returns: "bool",
      kind: "query",
      doc: "True if the party carries the item.",
    },
    // …
  ],
};

function HostFunctionsPanel() {
  const api = useStudioApi();
  return (
    <ul>
      {(manifest.externals ?? []).map((ext) => (
        <li key={ext.name}>
          <button
            onClick={() => {
              const args = (ext.params ?? []).map((p) => p.name).join(", ");
              api.insertText(`~ ${ext.name}(${args})\n`); // a call — never EXTERNAL
              api.notify({ severity: "info", source: "my host", message: `Inserted ${ext.name}` });
            }}
          >
            {ext.name}({(ext.params ?? []).map((p) => `${p.name}: ${p.ty}`).join(", ")})
            {ext.returns && ext.returns !== "void" ? ` -> ${ext.returns}` : ""}
            <small>{ext.doc}</small>
          </button>
        </li>
      ))}
    </ul>
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

await mountStudio(el, { files, entryFile: "main.ink", extensions, hostManifest: manifest });
```

The manifest (see
[host-capability-manifest.md](host-capability-manifest.md) — Track B) gives
the analyzer/IDE diagnostics, hover, and completion for host verbs, and the
same vocabulary feeds the panel's list, with `insertText` closing the
authoring loop. The story carries the matching `EXTERNAL` declarations —
the panel surfaces what is already defined.

## What is deliberately NOT exposed

- **The Zustand store.** `@brink-lang/studio` exports no `createStudioStore`, no
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
