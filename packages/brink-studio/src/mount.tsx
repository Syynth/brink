/**
 * mountStudio — the embedding entry point for brink-studio.
 *
 * The whole app bootstrap (wasm init, project session, store, registries,
 * navigation wiring, default layout) behind one call, so an embedding host
 * (the embedded playground; RPG Maker MZ planned) mounts the studio into an
 * element and optionally extends it (docs/studio-shell-spec.md §8):
 *
 * - `extensions` — host tool windows / commands / status-bar items,
 *   registered into the same registries as the built-ins, after them (so
 *   built-in Mod-1…N strip mnemonics stay put). Accepts the plain
 *   `StudioExtensions` config or a factory receiving the `StudioApi` facade,
 *   for host commands that need `dispatch`/`insertText`/`notify`.
 * - The returned handle carries the same `StudioApi` facade host components
 *   get via `useStudioApi()` — never the raw store (spec §8.2).
 *
 * The standalone app (main.tsx) is itself a caller of this function.
 */

import { createRoot } from "react-dom/client";
import { useEffect } from "react";
import { initWasm } from "@brink-lang/web";
import type { CompileResult, FileOutline, HostManifest, DialogueDialect } from "@brink/wasm-types";
import {
  DocumentSessions,
  ProjectSession,
  InMemoryFileProvider,
  setHostWidgets,
  brinkTheme,
  type FileChange,
  type FileConflict,
  type FileProvider,
} from "@brink-lang/editor";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import {
  CommandRegistry,
  DocumentTypeRegistry,
  EDITOR_REVEAL_COMMAND_ID,
  LocationResolvers,
  NotificationBell,
  NotificationCenter,
  ShellProvider,
  StatusBarRegistry,
  ToolWindowRegistry,
  VIEW_REVEAL_COMMAND_ID,
  ViewRevealHandlers,
  createEditorGroupsStore,
  documentKey,
  focusedTab,
  installStudioExtensions,
  resolveQualifiedSymbol,
  type DocumentRef,
  type EditorGroupsState,
  type EditorGroupsStore,
  type Location as ShellLocation,
  type SourceLocation,
  type StudioExtensions,
} from "@brink/studio-shell";
import {
  App,
  Binder,
  COMPILED_OUTPUT_TYPE_ID,
  CompileStatusSegment,
  CompiledOutputDocument,
  CursorSegment,
  ElementSegment,
  INK_FILE_TYPE_ID,
  InkFileDocument,
  KeyHintsSegment,
  OutputView,
  PLAYER_TYPE_ID,
  PlayerPane,
  ProblemsBadge,
  ProblemsView,
  ProgramView,
  SEARCH_TOOL_WINDOW_ID,
  SETTINGS_TYPE_ID,
  STORY_GRAPH_TYPE_ID,
  SearchView,
  SessionPicker,
  SettingsDocument,
  StateView,
  StorySegment,
  StoreProvider,
  SymbolContextMenuHost,
  SymbolRenamePrompt,
  applyComputedRename,
  StoryGraphDocument,
  StudioApiProvider,
  createStudioApi,
  inkFileRef,
  loadDiagnosticsSettings,
  loadEditorSettings,
  openPlayerSplit,
  registerCompiledOutputCommand,
  registerOpenPlayerCommand,
  registerSettingsCommand,
  registerStoryGraphCommand,
  type StudioApi,
} from "@brink/studio-ui";
import { registerStoryCommands } from "./story-commands.js";
import { registerFileCommands } from "./file-commands.js";
import { pushArgumentProviderValues } from "./argument-providers.js";
import { installAdoptedStyleSheetsShim } from "./adopted-style-sheets.js";

// ── Public types ───────────────────────────────────────────────────

export interface MountStudioOptions {
  /** Project files (path → ink source). */
  files: Record<string, string>;
  /** The project's entry file (must be a key of `files`). */
  entryFile: string;
  /**
   * Host-provided surfaces (spec §8.1), registered once at mount. A factory
   * receives the `StudioApi` facade for host commands that need it.
   */
  extensions?: StudioExtensions | ((api: StudioApi) => StudioExtensions);
  /**
   * The host-capability manifest (docs/host-capability-manifest.md): the
   * host's external-function vocabulary, registered once at mount — before
   * the first compile — so manifest-driven diagnostics, hover, and
   * completions are live from the start. The host owns this data; the wasm
   * session itself stays unexposed (spec §8.2).
   */
  hostManifest?: HostManifest;
  /**
   * The dialogue dialect (#368, docs/dialect-spec.md): the project's
   * dialogue-line conventions (cues, parentheticals, dialogue chains),
   * registered once at mount — before the first `line_contexts` query — so
   * screenplay classification/decorations/transitions/conversions are live
   * from the start. Absent ⇒ `AT_CUE_DIALECT` (byte-identical to the
   * pre-#368 hardcoded `@Name:<>` behavior); `null` ⇒ headless (the entire
   * screenplay layer is torn down). Mount-time only — like `hostManifest`,
   * there is no live-reconfigure handle exposed here; a host needing that
   * calls `setDialect(view, dialect)` directly against a specific editor
   * view (see `@brink-lang/editor`).
   */
  dialect?: DialogueDialect | null;
  /**
   * File-content egress (issue #154): called with batched change
   * notifications whenever project files change in the session — CM6 edits,
   * binder structural ops, search replacements, `file.new`. Debounced
   * (~500 ms trailing); pending changes flush immediately on `file.save` /
   * `file.saveAll` and on `unmount()`. Each change names the file, its kind
   * ("modified" | "created" | "deleted" — the latter designed-in but
   * currently unreachable: the studio has no delete UI yet), and the full
   * content. A host that persists files writes these back (e.g. RPG Maker
   * MZ writing `data/brink/**`; see docs/embedder-api.md "File egress").
   */
  onFilesChanged?: (changes: FileChange[]) => void;
  /**
   * File provider override (issue #320 / testability). Defaults to an
   * {@link InMemoryFileProvider} seeded from `files`. A host can pass its own
   * provider (e.g. one whose `onExternalChange` is driven by a real filesystem
   * watcher) so external on-disk changes — and the conflict merge view they
   * surface — work against live host I/O. When omitted, `files` seeds the
   * default in-memory provider as before.
   */
  provider?: FileProvider;
  /**
   * Where to load the wasm binary from — forwarded to `initWasm`. By
   * default the binary resolves relative to the module URL, which cannot
   * work inside an IIFE plugin bundle (no usable `import.meta.url`, e.g. an
   * RPG Maker MZ plugin): pass a URL/path or a precompiled
   * `WebAssembly.Module` there instead of relying on a pre-call to
   * `initWasm` and its double-init guard.
   */
  wasmLocation?: Parameters<typeof initWasm>[0];
}

export interface StudioHandle {
  /** The curated facade (spec §8.2) — the same one `useStudioApi()` serves. */
  api: StudioApi;
  /** Tear down: unmount React, dispose editor views, free the wasm session. */
  unmount(): void;
}

// ── Tool-window icons ──────────────────────────────────────────────
//
// Simple monochrome inline SVGs (currentColor) for the dock strips.

const iconProps = {
  width: 16,
  height: 16,
  viewBox: "0 0 16 16",
  fill: "none",
  stroke: "currentColor",
  strokeWidth: 1.5,
  strokeLinecap: "round",
  strokeLinejoin: "round",
  "aria-hidden": true,
} as const;

const BINDER_ICON = (
  <svg {...iconProps}>
    <path d="M3 3.5h10M3 6.5h10M5.5 9.5h7.5M5.5 12.5h7.5" />
  </svg>
);

const STATE_ICON = (
  <svg {...iconProps}>
    <path d="M6 2.5c-1.6 0-1.8 1.2-1.8 2.7S3.9 7.6 2.5 8c1.4.4 1.7 1.3 1.7 2.8S4.4 13.5 6 13.5" />
    <path d="M10 2.5c1.6 0 1.8 1.2 1.8 2.7s.3 2.4 1.7 2.8c-1.4.4-1.7 1.3-1.7 2.8s-.2 2.7-1.8 2.7" />
  </svg>
);

const PROGRAM_ICON = (
  <svg {...iconProps}>
    <rect x="4.5" y="4.5" width="7" height="7" rx="1" />
    <path d="M6.5 1.5v3M9.5 1.5v3M6.5 11.5v3M9.5 11.5v3M1.5 6.5h3M1.5 9.5h3M11.5 6.5h3M11.5 9.5h3" />
  </svg>
);

const PROBLEMS_ICON = (
  <svg {...iconProps}>
    <circle cx="8" cy="8" r="6" />
    <path d="M8 5v3.5M8 10.8v.2" />
  </svg>
);

const OUTPUT_ICON = (
  <svg {...iconProps}>
    <path d="M3 4.5l3 3-3 3M8.5 11.5H13" />
  </svg>
);

const SEARCH_ICON = (
  <svg {...iconProps}>
    <circle cx="7" cy="7" r="4.5" />
    <path d="M10.3 10.3L14 14" />
  </svg>
);

// ── Compile log messages (Output tool window, spec §4) ─────────────
//
// CompileResult carries no timing, so entries log outcome + counts.

function compileLogMessage(
  ok: boolean,
  errors: number,
  warnings: number,
  error: string | undefined,
): string {
  const plural = (n: number, word: string) => `${n} ${word}${n === 1 ? "" : "s"}`;
  if (ok) {
    return warnings > 0
      ? `Compile succeeded (${plural(warnings, "warning")})`
      : "Compile succeeded";
  }
  const summary = `Compile failed (${plural(errors, "error")})`;
  return error ? `${summary}: ${error}` : summary;
}

// Build an ink-file DocumentRef for a re-keyed docId ("path" or "path::symbol")
// after a rename/move — recomputes the tab title (basename, or "symbol
// (basename)") to match docTitleFor without needing the symbol's range.
function rekeyInkRef(docId: string): DocumentRef {
  const sep = docId.indexOf("::");
  const path = sep < 0 ? docId : docId.slice(0, sep);
  const symbol = sep < 0 ? null : docId.slice(sep + 2);
  const slash = path.lastIndexOf("/");
  const base = slash >= 0 ? path.slice(slash + 1) : path;
  return {
    typeId: INK_FILE_TYPE_ID,
    docId,
    title: symbol === null ? base : `${symbol} (${base})`,
  };
}

// ── Root component ─────────────────────────────────────────────

interface RootProps {
  store: StudioStore;
  project: ProjectSession;
  documents: DocumentSessions;
  commands: CommandRegistry;
  toolWindows: ToolWindowRegistry;
  statusBarItems: StatusBarRegistry;
  documentTypes: DocumentTypeRegistry;
  editorGroups: EditorGroupsStore;
  notifications: NotificationCenter;
  api: StudioApi;
}

function Root({
  store,
  project,
  documents,
  commands,
  toolWindows,
  statusBarItems,
  documentTypes,
  editorGroups,
  notifications,
  api,
}: RootProps) {
  // Tear down the wasm session + story runner when the app unmounts. The
  // standalone playground never unmounts, but the embeddable/host case does —
  // this keeps the lifecycle owned instead of leaking the cached parse/HIR.
  useEffect(
    () => () => {
      store.getState().disposeSession();
      documents.dispose();
      project.destroy();
    },
    [store, documents, project],
  );

  return (
    <ShellProvider
      commands={commands}
      toolWindows={toolWindows}
      statusBarItems={statusBarItems}
      documents={documentTypes}
      editorGroups={editorGroups}
      notifications={notifications}
    >
      <StoreProvider store={store}>
        <StudioApiProvider api={api}>
          <App />
          <SymbolContextMenuHost />
          <SymbolRenamePrompt />
        </StudioApiProvider>
      </StoreProvider>
    </ShellProvider>
  );
}

// ── Mount ──────────────────────────────────────────────────────────

export async function mountStudio(
  container: HTMLElement,
  options: MountStudioOptions,
): Promise<StudioHandle> {
  // Old-engine hosts first (NW.js / RPG Maker MZ ships Chromium 88, whose
  // frozen adoptedStyleSheets breaks CodeMirror's style injection): the
  // feature-detect is a no-op on modern browsers. Before any CM6/style-mod
  // code can run.
  installAdoptedStyleSheetsShim();

  await initWasm(options.wasmLocation);

  // Initialize the project BEFORE rendering so the wasm session has files
  // loaded. The store is constructed here (rather than after, as before
  // #2324) purely so its callbacks below have somewhere real to write to —
  // `createStudioStore()` takes no arguments and nothing else here depends
  // on ordering; `initialize()` (which binds `project`/`documents` into it)
  // still runs later, at its original call site.
  const provider = options.provider ?? new InMemoryFileProvider(options.files);
  const { entryFile } = options;
  const store = createStudioStore();
  const project = new ProjectSession({
    provider,
    entryFile,
    // Host egress (#154): every session-content mutation reports through
    // the project's FileChangeHub, which batches + debounces into this.
    onFilesChanged: options.onFilesChanged,
    // External-conflict surface (#320, Track V): the B1 hook fires here when
    // an on-disk change collides with an unsaved buffer. Mirror it into the
    // store so the merge view (banner + 2-way MergeView) can render + resolve.
    onFileConflict: (conflict: FileConflict) => {
      store.getState().setConflict(conflict);
    },
    // `brink.toml` project-config warnings (#2324): unrecognized `[project]`/
    // `[lints]` keys from `discoverProjectConfig` — surfaced through Output
    // (Mod-5) rather than dropped, since a silently-ignored typo in the one
    // file this whole feature exists to make effective would defeat the point.
    onProjectConfigWarnings: (warnings) => {
      for (const w of warnings) {
        store.getState().appendOutput("compile", `brink.toml: ${w}`);
      }
    },
    // `brink.toml` discovery/apply error (#2324 review finding): malformed
    // TOML or a recognized key with an invalid value used to propagate as an
    // uncaught exception out of `initialize()` (aborting mount entirely, with
    // no editor open yet to fix the file in) or out of every subsequent
    // keystroke's `notifyFileChanged`. Surfaced through the same Output
    // channel as the warnings above instead.
    onProjectConfigError: (message) => {
      store.getState().appendOutput("compile", `brink.toml: ${message}`);
    },
  });
  await project.initialize();

  // Register the host's capability manifest before anything compiles, so
  // the very first analysis already validates call sites against it.
  if (options.hostManifest !== undefined) {
    project.getSession().setHostManifest(options.hostManifest);
  }

  // Mirror the project's dirty-file count into the store — it feeds the
  // StudioPublicState.dirtyFiles summary (#154). Cheap scalar only; file
  // contents never enter public state.
  project.setDirtyListener((count) => store.getState().setDirtyFiles(count));

  // Shell command registry (spec §6). ShellProvider owns the keymap and the
  // global key handler, generates the `view.toggle.<id>` commands (Mod-1…9 by
  // registration order) from the tool-window registry below, and registers
  // the editor-group commands (editor.split Mod-\, move-tab, focus-next).
  const commands = new CommandRegistry();

  // Story session lifecycle (spec §7.6): story.start / restart / stop /
  // choose / continue, gated by session status. Commands own the session —
  // views dispatch these instead of mutating it.
  registerStoryCommands(commands, store);

  // Recompile on demand (the player's "Run" button). A successful compile
  // auto-starts the session via the compile-result handler below.
  commands.register({
    id: "compile.run",
    title: "Compile: Run",
    run: () => store.getState().compile(),
  });

  // Notification service (spec §7.5). The center is created here — not
  // inside ShellProvider — because the store→shell bridge below needs it
  // before React mounts: slices emit plain-data StoreNotifications through
  // the injected notifier (the store sits below the shell and cannot import
  // it; spec §7.2 layering).
  const notifications = new NotificationCenter();
  store.getState().setNotifier((n) => void notifications.notify(n));

  // Binder undo as a command (spec §7.5): the post-move notification's Undo
  // button dispatches this — actions carry command ids, never callbacks.
  commands.register({
    id: "binder.undo",
    title: "Binder: Undo Last Operation",
    when: () => store.getState().undoStack.length > 0,
    run: () => void store.getState().undo(),
  });

  // ── Editor groups + document types (spec §7.8) ────────────────────
  //
  // The shell owns tab/group structure; the app registers the "ink-file"
  // document type, whose component mounts one CM6 view per (document, group)
  // through DocumentSessions below.
  const editorGroups: EditorGroupsStore = createEditorGroupsStore();
  const documentTypes = new DocumentTypeRegistry();
  documentTypes.register({ id: INK_FILE_TYPE_ID, component: InkFileDocument });
  // Compiled Output (#91): a read-only, compile-bound singleton document over
  // the current compile's .inkt dump — no wasm document handle, just a string
  // (the component subscribes to programInkt). Opened by command (palette or
  // the Program Explorer toolbar); reopening focuses the existing tab.
  documentTypes.register({
    id: COMPILED_OUTPUT_TYPE_ID,
    component: CompiledOutputDocument,
  });
  registerCompiledOutputCommand(commands, editorGroups);
  // Player (#120): the session document — singleton, session-bound (§7.6),
  // opened in a right split at bootstrap (Inky two-up) and via
  // story.openPlayer. The old player tool window is gone; State View takes
  // its right/start strip slot.
  documentTypes.register({ id: PLAYER_TYPE_ID, component: PlayerPane });
  registerOpenPlayerCommand(commands, editorGroups);
  // Settings (#93): static UI over shell services — not session-bound, not
  // compile-bound. Singleton; settings.open (Mod-,) focuses an existing tab.
  documentTypes.register({ id: SETTINGS_TYPE_ID, component: SettingsDocument });
  registerSettingsCommand(commands, editorGroups);
  // Story Graph (#97, spec §4.1): custom-rendered, compile-bound singleton
  // over the wasm story-graph query (the component subscribes to storyGraph,
  // refreshed below on each successful compile), with the live session
  // overlay from debugState. Opened via story.openGraph (palette/hamburger);
  // reopening focuses the existing tab.
  documentTypes.register({ id: STORY_GRAPH_TYPE_ID, component: StoryGraphDocument });
  registerStoryGraphCommand(commands, editorGroups);

  // Compile-result handler shared by every path that compiles (per-view
  // debounced compiles, compile.run, the initial compile). DocumentSessions
  // collapses reference-equal (cached) deliveries.
  const handleCompileResult = (result: CompileResult): void => {
    const state = store.getState();
    const outline: FileOutline[] = project.getSession().getProjectOutline();

    let errors = 0;
    let warnings = 0;
    if (result.warnings) {
      for (const w of result.warnings) {
        if (w.severity === "Error") errors++;
        else warnings++;
      }
    }
    if (result.error) errors++;

    const storyBytes = result.ok && result.story_bytes
      ? new Uint8Array(result.story_bytes)
      : null;

    state.setCompileResult(outline, { errors, warnings }, result.warnings ?? [], storyBytes);
    state.appendOutput("compile", compileLogMessage(result.ok, errors, warnings, result.error));

    // Story Graph data (#97, spec §4.1): recompute from the analyzer on each
    // successful compile, like the outline. A failed compile (or a pre-analysis
    // null) keeps the last good graph — same policy as programInkt.
    if (result.ok) {
      const storyGraph = project.getSession().getStoryGraph();
      if (storyGraph !== null) state.setStoryGraph(storyGraph);
    }

    // Recompile-while-running (spec §7.6): a successful compile auto-starts
    // the session on the new program through the same code path as the
    // story.start command — startSession replays the recorded choice log,
    // truncating with a notification on divergence. A failed compile takes
    // the `storyBytes === null` branch and leaves the existing session
    // running on the old program.
    if (storyBytes) {
      state.startSession(storyBytes);
    }
  };

  // Per-(document, group) editor views over wasm document handles. Cursor,
  // line info, auto-pin, focus tracking, and the e2e `__brinkView` hook all
  // flow through these callbacks; the manager keeps them targeted at the
  // focused group's active view.
  const documents = new DocumentSessions(project, {
    onCursorChange: (line, col) => store.getState().setCursor(line, col),
    onLineInfoChange: (info, hints) => store.getState().setLineInfo(info, hints),
    onCompileResult: handleCompileResult,
    onDocEdited: (docKey, groupId) =>
      editorGroups
        .getState()
        .pinTab(groupId, documentKey({ typeId: INK_FILE_TYPE_ID, docId: docKey })),
    onViewFocused: (_docKey, groupId) => editorGroups.getState().focusGroup(groupId),
    onFocusedViewChange: (view) => {
      (window as unknown as Record<string, unknown>).__brinkView = view ?? undefined;
    },
    onNavigateToFile: (location) =>
      revealSource({
        kind: "source",
        file: location.file,
        span: { start: location.start, end: location.end },
      }),
    // "Play from here" (#186): a fresh session entered at the knot/stitch path.
    onPlayFrom: (inkPath, label) => store.getState().openSession({ path: inkPath, label }),
    // Right-click a knot/stitch → the shared symbol context menu (rendered by
    // <SymbolContextMenuHost/>).
    onSymbolContextMenu: (info, x, y) =>
      store.getState().openSymbolMenu({ ...info, x, y, source: "editor" }),
    // Inline rename (#323/#324): the editor's F2 / context-menu rename runs
    // fully in the editor — the badge computes the breakage live, and this
    // commit applies the (already-computed) edits + re-keys the symbol tab.
    // The modal <SymbolRenamePrompt/> stays for Binder/Story-Graph renames.
    onRenameCommit: (req) => {
      const state = store.getState();
      void applyComputedRename(state, state.applyMoveResult, req);
    },
    // Code-actions menu + Extract to knot/function (#315 H / #321 studio side):
    // apply the resolved/extracted StructuralResult through the same undoable
    // apply seam as binder moves — one step, toast + Undo. Safe-by-default is
    // enforced in the editor (unsafe surfaces the inline report and applies only
    // on force), so an unsafe result never reaches here unforced.
    onApplyStructural: (req) => {
      const state = store.getState();
      void state.applyMoveResult(
        req.result,
        req.description,
        req.result.path ? [req.result.path] : [],
      );
    },
    // The studio skin, opted into EXPLICITLY (#363): the editor package is
    // headless-ready and hosts may pass `theme: false`; studio pins the
    // `--bs-*`-token theme so its look never depends on the package default.
    //
    // Dialect (#368): forwarded straight through to `brinkStudio` per mounted
    // view (`slotOptions`). Absent ⇒ AT_CUE_DIALECT there already, so leaving
    // this undefined when the host doesn't pass one preserves the
    // byte-identical default with no extra wiring needed here.
  }, [], { theme: brinkTheme, dialect: options.dialect });

  // File save commands (#154): file.save (Mod-S) / file.saveAll flush
  // editor text to the session and deliver pending host change
  // notifications immediately (bypassing the egress debounce). They work —
  // and notify — with or without an onFilesChanged host hook.
  registerFileCommands(commands, {
    project,
    documents,
    notify: (n) => void notifications.notify(n),
  });

  // The store's document opener (Binder rows, addFile): note the target so
  // symbol mounts can fall back to the outline range, then open through the
  // shell's groups store (which applies the §7.8 reveal policy).
  store.getState().setDocumentOpener((target, pinned) => {
    documents.noteTarget(target);
    editorGroups.getState().openDocument(inkFileRef(target), { pinned });
  });

  // The store's tab-closer (binder delete): close every tab for a file path —
  // the file document and any of its `path::symbol` fragment tabs — across all
  // groups. Closing the tabs is enough; the syncFromGroups subscriber below
  // then prunes the matching DocumentSessions view-slots (growth guard).
  store.getState().setDocCloser((path) => {
    const symbolPrefix = `${path}::`;
    const toClose: Array<{ groupId: string; key: string }> = [];
    for (const group of editorGroups.getState().groups) {
      for (const tab of group.tabs) {
        if (tab.ref.typeId !== INK_FILE_TYPE_ID) continue;
        if (tab.ref.docId === path || tab.ref.docId.startsWith(symbolPrefix)) {
          toClose.push({ groupId: group.id, key: documentKey(tab.ref) });
        }
      }
    }
    // Collect first, then close — closeTab mutates the groups we iterated.
    for (const { groupId, key } of toClose) {
      editorGroups.getState().closeTab(groupId, key);
    }
  });

  // The store's tab-renamer (binder rename/move): re-key every tab for a file
  // path in place — the file document and its `path::symbol` fragment tabs —
  // preserving pin/split/active state, then migrate the matching view slots.
  store.getState().setDocRenamer((oldPath, newPath) => {
    const symbolPrefix = `${oldPath}::`;
    const updates: Array<{ oldKey: string; newRef: ReturnType<typeof rekeyInkRef> }> = [];
    for (const group of editorGroups.getState().groups) {
      for (const tab of group.tabs) {
        if (tab.ref.typeId !== INK_FILE_TYPE_ID) continue;
        const id = tab.ref.docId;
        const newDocId =
          id === oldPath
            ? newPath
            : id.startsWith(symbolPrefix)
              ? newPath + id.slice(oldPath.length)
              : null;
        if (newDocId === null) continue;
        updates.push({ oldKey: documentKey(tab.ref), newRef: rekeyInkRef(newDocId) });
      }
    }
    for (const { oldKey, newRef } of updates) {
      editorGroups.getState().updateTabRef(oldKey, newRef);
    }
    // Keep the per-view document machinery aligned with the re-keyed tabs.
    documents.renameDocPath(oldPath, newPath);
  });

  // The store's symbol-tab-renamer (#305 knot/stitch rename): re-key the open
  // `path::oldName` symbol tab to `path::newName` in place, then migrate the
  // matching view slot, so a symbol view survives its own rename.
  store.getState().setDocSymbolRenamer((path, oldName, newName) => {
    if (oldName === newName) return;
    const oldDocId = `${path}::${oldName}`;
    const newDocId = `${path}::${newName}`;
    const updates: Array<{ oldKey: string; newRef: ReturnType<typeof rekeyInkRef> }> = [];
    for (const group of editorGroups.getState().groups) {
      for (const tab of group.tabs) {
        if (tab.ref.typeId !== INK_FILE_TYPE_ID) continue;
        if (tab.ref.docId !== oldDocId) continue;
        updates.push({ oldKey: documentKey(tab.ref), newRef: rekeyInkRef(newDocId) });
      }
    }
    for (const { oldKey, newRef } of updates) {
      editorGroups.getState().updateTabRef(oldKey, newRef);
    }
    documents.renameSymbolDoc(path, oldName, newName);
  });

  // Keep the focused-view tracking and the store's activeDocKey mirror in
  // sync with the shell's groups store, and prune cached view slots for
  // closed tabs (unbounded-growth guard).
  const syncFromGroups = (state: EditorGroupsState): void => {
    const tab = focusedTab(state);
    const inkDocKey =
      tab !== null && tab.ref.typeId === INK_FILE_TYPE_ID ? tab.ref.docId : null;
    store.getState().setActiveDocKey(inkDocKey ?? "");
    documents.setFocused(inkDocKey, inkDocKey !== null ? state.focusedGroupId : null);

    const liveSlots = new Set<string>();
    const liveDocKeys = new Set<string>();
    for (const group of state.groups) {
      for (const t of group.tabs) {
        if (t.ref.typeId !== INK_FILE_TYPE_ID) continue;
        liveSlots.add(DocumentSessions.slotId(t.ref.docId, group.id));
        liveDocKeys.add(t.ref.docId);
      }
    }
    documents.retainSlots(liveSlots, liveDocKeys);
  };
  editorGroups.subscribe(syncFromGroups);

  // Navigation protocol (spec §6.1): resolvers translate Locations toward
  // source; editor.reveal opens the file (focusing an existing tab in any
  // group per the §7.8 reveal policy) and scrolls to the span. The symbol
  // resolver reads the latest compile outline; program/session resolvers
  // land with their consumers (#91, State View links).
  const locations = new LocationResolvers();
  locations.register("symbol", (location) =>
    location.kind === "symbol"
      ? resolveQualifiedSymbol(store.getState().outline, location.name)
      : null,
  );
  const revealSource = (target: SourceLocation): void => {
    store.getState().openTarget({ kind: "file", path: target.file }, true);
    documents.revealAt(target.file, target.span.start);
  };
  const revealHandlers = new ViewRevealHandlers();
  commands.register({
    id: EDITOR_REVEAL_COMMAND_ID,
    title: "Editor: Reveal Location",
    run: (args) => {
      const target = locations.resolve(args as ShellLocation);
      if (target !== null) revealSource(target);
    },
  });
  commands.register({
    id: VIEW_REVEAL_COMMAND_ID,
    title: "View: Reveal Item",
    run: (args) => {
      const { viewId, item } = (args ?? {}) as { viewId?: string; item?: unknown };
      if (typeof viewId === "string") revealHandlers.reveal(viewId, item);
    },
  });

  // Exposed for e2e/manual verification, like __brinkView.
  (window as unknown as Record<string, unknown>).__brinkCommands = commands;
  (window as unknown as Record<string, unknown>).__brinkNotifications = notifications;
  (window as unknown as Record<string, unknown>).__brinkEditorGroups = editorGroups;

  // Tool-window registry (spec §7.1, §4). Registration order is the stable,
  // user-visible Mod-N ordering: Binder Mod-1, State Mod-2, Program Mod-3,
  // Problems Mod-4, Output Mod-5, Search Mod-6. (The Player is an editor
  // document now — #120 — not a tool window.) Search registers last so the
  // established mnemonics stay put (#94); host extension windows register
  // after all built-ins (below) for the same reason. The shell never imports
  // these components — they are registered into it here, at the app boundary.
  //
  // Dock-section sharing: Program Explorer and Problems both default to
  // bottom/start (spec §4) — a section holds multiple windows, one open at a
  // time, the strip tabs between them. Output takes bottom/end; Search
  // shares left/start with the Binder the same way.
  const toolWindows = new ToolWindowRegistry();
  toolWindows.register({
    id: "binder",
    title: "Binder",
    icon: BINDER_ICON,
    defaultPlacement: { dock: "left", section: "start" },
    defaultOpen: true,
    component: Binder,
  });
  toolWindows.register({
    id: "state",
    title: "State View",
    icon: STATE_ICON,
    defaultPlacement: { dock: "right", section: "start" },
    defaultOpen: false,
    component: StateView,
  });
  toolWindows.register({
    id: "program",
    title: "Program Explorer",
    icon: PROGRAM_ICON,
    defaultPlacement: { dock: "bottom", section: "start" },
    defaultOpen: false,
    component: ProgramView,
  });
  toolWindows.register({
    id: "problems",
    title: "Problems",
    icon: PROBLEMS_ICON,
    defaultPlacement: { dock: "bottom", section: "start" },
    defaultOpen: false,
    badge: ProblemsBadge,
    component: ProblemsView,
  });
  toolWindows.register({
    id: "output",
    title: "Output",
    icon: OUTPUT_ICON,
    defaultPlacement: { dock: "bottom", section: "end" },
    defaultOpen: false,
    component: OutputView,
  });
  // Search (#94): project-wide find/replace, sharing left/start with the
  // Binder (the strip tabs between them). Closed by default; opened by
  // search.focus (Mod-Shift-F, registered by <SearchCommands/> in App) or
  // its generated Mod-6 toggle.
  toolWindows.register({
    id: SEARCH_TOOL_WINDOW_ID,
    title: "Search",
    icon: SEARCH_ICON,
    defaultPlacement: { dock: "left", section: "start" },
    defaultOpen: false,
    component: SearchView,
  });

  // Status-bar segments (spec §7.3). Higher priority renders further left
  // within its group. Left: app status; right: editor context.
  const statusBarItems = new StatusBarRegistry();
  statusBarItems.register({
    id: "status.compile",
    alignment: "left",
    priority: 20,
    component: CompileStatusSegment,
  });
  statusBarItems.register({
    id: "status.story",
    alignment: "left",
    priority: 10,
    component: StorySegment,
  });
  // Multi-session picker (#182) — sits just after the story status, hidden
  // until there's more than one session.
  statusBarItems.register({
    id: "status.sessions",
    alignment: "left",
    priority: 9,
    component: SessionPicker,
  });
  statusBarItems.register({
    id: "status.cursor",
    alignment: "right",
    priority: 30,
    component: CursorSegment,
  });
  statusBarItems.register({
    id: "status.element",
    alignment: "right",
    priority: 20,
    component: ElementSegment,
  });
  statusBarItems.register({
    id: "status.keyhints",
    alignment: "right",
    priority: 10,
    component: KeyHintsSegment,
  });
  // Notification bell (spec §7.5): rightmost in the right group. The
  // component lives in studio-shell (it only needs shell context) but is
  // registered here with the rest — the shell never registers itself.
  statusBarItems.register({
    id: "status.notifications",
    alignment: "right",
    priority: 5,
    component: NotificationBell,
  });

  // The curated host facade (spec §8.2) — built over the store, commands,
  // and notifications; handed to host components via context, never the
  // raw store.
  const api = createStudioApi({ store, commands, notifications });

  // Host extensions (spec §8.1): registered through the host-only registry
  // doors AFTER every built-in, so built-in ids keep their strip mnemonics
  // and the namespacing/collision rules are enforced with clean errors.
  if (options.extensions !== undefined) {
    const extensions =
      typeof options.extensions === "function"
        ? options.extensions(api)
        : options.extensions;
    installStudioExtensions(extensions, { commands, toolWindows, statusBarItems });
    // Host argument providers (#175): enumerate + push into the editor's value
    // cache so the picker shows the host's live vocabulary. Data-only, applied
    // to the session rather than a registry.
    if (extensions.argumentProviders !== undefined) {
      await pushArgumentProviderValues(
        project.getSession(),
        extensions.argumentProviders,
      );
    }
    // Host argument widgets (argument-widget-spec §3): register into the editor's
    // widget registry so a host can render the inline chip's label + a popover
    // editor for a semantic type. The studio owns the chrome.
    if (extensions.argumentWidgets !== undefined) {
      setHostWidgets(extensions.argumentWidgets);
    }
  }

  // Restore the persisted diagnostics setting (Settings document, #93)
  // before initialize: pre-bind, the action only seeds the state, and
  // initialize applies it to the wasm session ahead of the first compile.
  store.getState().setExternalCheck(
    loadDiagnosticsSettings(window.localStorage).externalCheck,
  );

  // Bind handles, kick the initial compile, and open the entry file (the
  // groups-store subscription above keeps focus tracking in sync as the
  // document component mounts).
  store.getState().initialize(project, documents);

  // Restore the persisted editor settings (Settings → Editor). After initialize,
  // so the actions reach `documents`; new views read them from slotOptions, open
  // ones get the live switch.
  {
    const editor = loadEditorSettings(window.localStorage);
    store.getState().setFormGlyph(editor.formGlyph);
    store.getState().setAutoOpenForm(editor.autoOpenForm);
  }
  store.getState().openTarget({ kind: "file", path: entryFile }, true);

  // Default layout (spec §4): the Inky two-up — entry file left, player in a
  // right split, focus back on the editor. Group layout is not persisted, so
  // every fresh load reproduces this.
  openPlayerSplit(editorGroups);

  const root = createRoot(container);
  root.render(
    <Root
      store={store}
      project={project}
      documents={documents}
      commands={commands}
      toolWindows={toolWindows}
      statusBarItems={statusBarItems}
      documentTypes={documentTypes}
      editorGroups={editorGroups}
      notifications={notifications}
      api={api}
    />,
  );

  return {
    api,
    // Unmounting runs Root's cleanup effect: dispose session + views + project.
    // Editor views unmount (child effects) before Root's cleanup runs, so the
    // egress flush must happen first, while the views still exist: push every
    // mounted view's text, then deliver pending host notifications (#154).
    unmount: () => {
      documents.flushAll();
      project.flushFileChanges();
      root.unmount();
    },
  };
}
