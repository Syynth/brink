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
import { Profiler, useEffect } from "react";
import { initWasm } from "@brink-lang/web";
import type {
  CompileResult,
  FileOutline,
  HostManifest,
  DialogueDialect,
  StoryGraph,
} from "@brink/wasm-types";
import {
  DEFAULT_EDITOR_FONT_SIZE,
  DocumentSessions,
  ProjectSession,
  InMemoryFileProvider,
  setHostWidgets,
  brinkTheme,
  attachPerfObservers,
  perfMark,
  perfRecord,
  perfReport,
  perfReset,
  perfSpan,
  perfTime,
  setPerfEnabled,
  type FileChange,
  type FileConflict,
  type FileProvider,
  type HostPerfBundle,
} from "@brink-lang/editor";
import {
  loadProblemsPrefs,
  loadTodosPrefs,
  saveTodosPrefs,
  saveProblemsPrefs,
  BINDER_SIDECAR_PATH,
  createStudioStore,
  isDebugSessionProvider,
  sessionDegraded,
  parseBinderOrder,
  withDictionaryWord,
  toProseDiagnostics,
  type StudioStore,
} from "@brink/studio-store";
import {
  CommandRegistry,
  DocumentTypeRegistry,
  EDITOR_REVEAL_COMMAND_ID,
  encodeProgramAddress,
  LocationResolvers,
  NotificationBell,
  NotificationCenter,
  ShellProvider,
  StatusBarRegistry,
  ToolWindowRegistry,
  VIEW_REVEAL_COMMAND_ID,
  ViewRevealHandlers,
  createEditorGroupsStore,
  createShellLayoutStore,
  attachEditorPersistence,
  loadEditorSnapshot,
  reconcileEditorSnapshot,
  documentKey,
  focusedTab,
  installStudioExtensions,
  type DocumentRef,
  type EditorGroupsState,
  type EditorGroupsStore,
  type ShellLayoutStore,
  type Location as ShellLocation,
  type SourceLocation,
  type StudioExtensions,
} from "@brink/studio-shell";
import {
  App,
  Binder,
  COMPILED_OUTPUT_TYPE_ID,
  CompileStatusSegment,
  ScopeNoteSegment,
  CompiledOutputDocument,
  CursorSegment,
  ElementSegment,
  INK_FILE_TYPE_ID,
  InkFileDocument,
  KeyHintsSegment,
  OutputView,
  PerfView,
  type PerfViewBridge,
  type WasmCounterMap,
  PLAYER_TYPE_ID,
  PlayerPane,
  ProblemsActions,
  ProblemsBadge,
  ProblemsView,
  TodosBadge,
  TodosActions,
  TodosView,
  ProgramView,
  SEARCH_TOOL_WINDOW_ID,
  STORY_GRAPH_TYPE_ID,
  SearchView,
  SessionPicker,
  SettingsDocument,
  StateView,
  StorySegment,
  StoreProvider,
  StructuralOpSegment,
  EditorTextMenuHost,
  SymbolContextMenuHost,
  SymbolRenamePrompt,
  applyComputedRename,
  StoryGraphDocument,
  StudioApiProvider,
  createStudioApi,
  inkFileRef,
  loadDebugSettings,
  loadDiagnosticsSettings,
  loadEditorSettings,
  saveEditorSettings,
  openPlayerSplit,
  playerRef,
  DocumentIcon,
  StudioContinuousView,
  registerCompiledOutputCommand,
  registerOpenPlayerCommand,
  registerSettingsCommand,
  SETTINGS_SECTION_IDS,
  SETTINGS_TYPE_ID,
  SettingsModal,
  settingsSections,
  isConfigPath,
  registerStoryGraphCommand,
  type StudioApi,
} from "@brink/studio-ui";
import { registerStoryCommands } from "./story-commands.js";
import { registerDebugCommands } from "./debug-commands.js";
import { registerLocationResolvers } from "./location-resolvers.js";
import { loadBreakpoints, saveBreakpoints } from "./breakpoint-persistence.js";
import { executionHighlightsFor } from "./execution-highlights.js";
import { subscribeDebugRefresh } from "./debug-refresh-subscription.js";
import { registerFileCommands } from "./file-commands.js";
import { pushArgumentProviderValues } from "./argument-providers.js";
import { installAdoptedStyleSheetsShim } from "./adopted-style-sheets.js";
import { studioProseChecker } from "./prose-checker.js";

// ── Public types ───────────────────────────────────────────────────

export interface MountStudioOptions {
  /**
   * W4 (docs/editor-worker-spec.md): run the project-level query road —
   * compile, outline, story graph, closure — in a Web Worker with its own
   * wasm session. Feature-detected; environments without workers keep the
   * in-process road. Default TRUE since the W5 flip (decision log
   * 2026-08-25); pass false to force the in-process road.
   */
  workerSession?: boolean;
  /**
   * Runtime performance instrumentation (prod-perf ruling 2026-08-25):
   * the probe, browser observers, wasm counters, the `__brinkPerf`
   * harvesting global, and the Performance tool window. Default TRUE in
   * ALL builds — collection is allocation-free per event and every
   * retained structure is bounded, and real projects run in production
   * builds where dev-only data can't see them. Pass false to strip the
   * whole surface (the tool window is not registered and nothing records).
   */
  perf?: boolean;
  /** Project files (path → ink source). */
  files: Record<string, string>;
  /**
   * The project's entry file (must be a key of `files`) — used to seed
   * `brink.toml` discovery and, absent a config override, as the
   * compile/initial-tab entry. A discovered `brink.toml` naming a valid
   * `[project] entry` SUPERSEDES this argument (issue #2331, ruled
   * 2026-08-07 "`[project] entry` beats `mountStudio`'s `entryFile`") — see
   * `ProjectSession.getEntryFile`'s doc
   * (`packages/ink-editor/src/project-session.ts`) for the full precedence
   * rule. This argument is therefore only the fallback for a configless
   * project (no `brink.toml`, or one that doesn't set `entry`); a host
   * whose project always carries a `brink.toml` with `entry` set can pass
   * any file that exists in `files` here — its value never surfaces once
   * discovery supersedes it.
   */
  entryFile: string;
  /**
   * Whether `entryFile` is a human's EXPLICIT choice rather than a host
   * default (the file-anchored project open model, ruled 2026-08-23). When
   * true, a discovered `brink.toml`'s `[project] entry` never supersedes
   * `entryFile` — the #2331 precedence applies to host defaults only, and
   * an explicit open is not a default. Forwarded verbatim to
   * `ProjectSessionOptions.entryIsExplicit` (`@brink-lang/editor`); see
   * that option's doc for the full rule. Default `false`.
   */
  entryIsExplicit?: boolean;
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
  /**
   * Identity of the project this mount is editing, used to scope the
   * per-project editor state that survives a reload — open tabs, tab order,
   * pin state, splits, and each document's cursor + scroll (decision log
   * 2026-08-26). A host that opens real projects passes something stable and
   * unique to one project, such as its absolute root path.
   *
   * Omit it and editor state is not persisted at all: a host with no notion
   * of "which project" (the playground, a fixture, an embedded demo) has no
   * honest scope to key by, and a shared one would restore one fixture's
   * tabs over another's.
   */
  sessionScope?: string;
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
   * ("modified" | "created" | "deleted" — the binder's Delete action and
   * renames/moves both report it), and the full content. A host that
   * persists files writes these back (e.g. RPG Maker MZ writing
   * `data/brink/**`; see docs/embedder-api.md "File egress").
   */
  onFilesChanged?: (changes: FileChange[]) => void;
  /**
   * Whether `onFilesChanged` delivery counts as persistence (default
   * `true`, the write-through contract). Overlay hosts whose egress
   * handler feeds a backup ring rather than canonical storage (the
   * celeris file model; brink-desktop D2 + `OverlayPersistence`) set
   * `false`: batches still deliver, but dirty means "diverges from the
   * last canonical save" and only the save commands clear it. See
   * `ProjectSessionOptions.egressPersists`.
   */
  egressPersists?: boolean;
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
  /**
   * The project's EFFECTIVE entry file, project-relative —
   * `ProjectSession.getEntryFile()`'s result once `initialize()` has run,
   * i.e. with `[project] entry` precedence already applied (issue #2331,
   * ruled 2026-08-07 "`[project] entry` beats `mountStudio`'s
   * `entryFile`"). A host that needs to act on "the file the editor
   * actually treats as the entry" (batch tooling, an export command) must
   * read this rather than echoing back its own `MountStudioOptions.entryFile`
   * argument — that argument is only the fallback for a configless project,
   * and a host that used it directly would silently disagree with the
   * editor for any project whose `brink.toml` names a different entry
   * (2026-08 review finding, brink-desktop's `exportXliff`).
   */
  entryFile: string;
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

const TODO_ICON = (
  <svg {...iconProps}>
    <path d="M13.5 7.5V12a2 2 0 0 1-2 2h-7a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h5" />
    <path d="M5.5 7.5L8 10l6-6.5" />
  </svg>
);

// Dev-only Performance HUD (measure-first ruling, 2026-08-24): a stopwatch.
const PERF_ICON = (
  <svg {...iconProps}>
    <circle cx="8" cy="9" r="5.5" />
    <path d="M8 6.5V9l2 1.5M6.5 2h3" />
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
  layout: ShellLayoutStore;
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
  layout,
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
      layout={layout}
      notifications={notifications}
      // Single File view's native split (decision log 2026-08-26). The shell
      // is told WHICH document sits beside the file, not what it means — so
      // "run the scene you are writing" is one prop rather than the shell
      // learning what a player is.
      companionDocument={playerRef()}
      // Continuous view's content. An element, not a list: the ORDER is
      // binder order, which lives in the studio store, and passing the
      // element lets it render inside the store's provider (decision log
      // 2026-08-26).
      continuousView={<StudioContinuousView />}
      // The file icon before every document name the shell writes (#3145,
      // ruled 2026-08-27: a file's name and its draft status never appear
      // apart, and the icon is what carries the status). One seam, four
      // surfaces — see DocumentIcon.
      documentIcon={DocumentIcon}
    >
      <StoreProvider store={store}>
        <StudioApiProvider api={api}>
          <App>
            {/* Inside the .brink-studio root, or their fixed positioning
                and tokens never apply (#3054 review — the eaten menu). */}
            {/* Settings (#3174): a modal over the whole studio, inside the
                .brink-studio root so tokens apply — the same placement the
                other host surfaces need (#3054's eaten menu). */}
            <SettingsModal sections={settingsSections("settings")} />
            <SymbolContextMenuHost />
            <EditorTextMenuHost />
            <SymbolRenamePrompt />
          </App>
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

  // Perf probe (measure-first ruling 2026-08-24; prod-perf ruling
  // 2026-08-25): collection, observers, and the HUD tool window ship in
  // ALL builds, on by default — real projects are opened in production
  // builds, and dev-only instrumentation can't see them. The probe's
  // enabled path is allocation-free and every retained structure bounded
  // (probe.ts contract). `perf: false` strips the whole surface.
  const perfOn = options.perf ?? true;
  if (perfOn) {
    setPerfEnabled(true);
    attachPerfObservers();
    perfMark("studio.mountStart");
  }

  await initWasm(options.wasmLocation);
  perfMark("studio.wasmReady");

  // Initialize the project BEFORE rendering so the wasm session has files
  // loaded. The store is constructed here (rather than after, as before
  // #2324) purely so its callbacks below have somewhere real to write to —
  // `createStudioStore()` takes no arguments and nothing else here depends
  // on ordering; `initialize()` (which binds `project`/`documents` into it)
  // still runs later, at its original call site.
  const provider = options.provider ?? new InMemoryFileProvider(options.files);
  const { entryFile } = options;
  const store = createStudioStore();
  // Late-bound: DocumentSessions is constructed after ProjectSession but
  // external-change events only fire after initialize(), by which point the
  // assignment below has run.
  let documentsRef: DocumentSessions | null = null;

  const project = new ProjectSession({
    provider,
    entryFile,
    // W5 flip (decision log 2026-08-25): ON by default — the worker road
    // is fully feature-detected, so environments without workers (jsdom,
    // old bundlers) silently keep the in-process road.
    workerSession: options.workerSession ?? true,
    // File-anchored open (ruled 2026-08-23): an explicit open's entry is
    // never superseded by a discovered `[project] entry`.
    entryIsExplicit: options.entryIsExplicit,
    // Host egress (#154): every session-content mutation reports through
    // the project's FileChangeHub, which batches + debounces into this.
    onFilesChanged: options.onFilesChanged,
    // The #320 CLEAN path's view half: the session content just changed
    // under an open editor, so re-sync its mounted views — otherwise the
    // stale view's next flush silently reverts the external update (found
    // live by brink-desktop's D2 watcher, this hook's first real producer).
    // Deletions (#2371 ruling, "keep the view, mark orphaned"): never
    // auto-close the tab or touch its buffer — `markOrphaned` only recreates
    // the session-side content from the kept buffer so IDE queries and a
    // later save keep working; the hub already flags the path orphaned
    // (`FileChangeHub.applyExternal`) for tab badging.
    onExternalFileChange: (path, content) => {
      if (content !== null) documentsRef?.refreshExternal(path);
      else documentsRef?.markOrphaned(path);
    },
    // Overlay hosts (backup-ring egress) declare that delivery is NOT
    // persistence, so dirty survives until a canonical save (D2 model).
    egressPersists: options.egressPersists,
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
  perfMark("studio.projectInitialized");
  // The perf bridge: the session planes the probe module itself can't
  // reach. Feature-detected throughout — injected sessions/mocks predate
  // the perf API — and the worker fetches guard on `workerActive()` so
  // the in-process road never answers a host-realm query with a mirror
  // of the main realm's own state.
  const perfSession = project.getSession() as {
    setPerfEnabled?: (on: boolean) => void;
    getPerfCounters?: () => WasmCounterMap | null;
    resetPerfCounters?: () => void;
    perfCompileProbe?: (entry: string) => [number, number];
  };
  const fetchWorkerPerf = (): Promise<HostPerfBundle | null> =>
    project.workerActive()
      ? project
          .projectQuery<HostPerfBundle>("hostPerfReport", [], { coalesceKey: "perf:hud" })
          .catch(() => null)
      : Promise.resolve(null);
  const perfBridge: PerfViewBridge = {
    wasmCounters: () => perfSession.getPerfCounters?.() ?? null,
    fetchWorker: fetchWorkerPerf,
    setWorkerEnabled: (on) => {
      perfSession.setPerfEnabled?.(on);
      if (project.workerActive())
        void project.projectQuery("hostPerfSetEnabled", [on]).catch(() => {});
    },
    resetWorker: () => {
      perfSession.resetPerfCounters?.();
      if (project.workerActive()) void project.projectQuery("hostPerfReset", []).catch(() => {});
    },
  };
  if (perfOn) {
    perfSession.setPerfEnabled?.(true);
    // Harvesting hook for the scenario runner (perf-runs/), e2e specs, and
    // hand-driven console sessions: the probe report + wasm counters +
    // the worker bundle + the #2885 compile probe, in one place.
    (globalThis as Record<string, unknown>).__brinkPerf = {
      report: (worstCount?: number) => perfReport(worstCount ?? 25),
      reset: () => {
        perfReset();
        perfBridge.resetWorker?.();
      },
      wasmCounters: () => perfSession.getPerfCounters?.() ?? null,
      workerReport: fetchWorkerPerf,
      compileProbe: () => perfSession.perfCompileProbe?.(project.getEntryFile()) ?? null,
    };
  } else {
    // The worker realm boots with its own probe on (it can't see this
    // option); an opted-out host turns it off right after mount.
    if (project.workerActive())
      void project.projectQuery("hostPerfSetEnabled", [false]).catch(() => {});
  }

  // The .binder.json order sidecar (#3038): loaded straight from the
  // provider — it never enters the wasm session (presentation, not
  // source). Mutations persist through the provider's canonical write.
  const sidecarText = await provider.requestFile?.(BINDER_SIDECAR_PATH).catch(() => null);
  store.getState().setBinderOrder(parseBinderOrder(sidecarText ?? null));
  store.setState({
    _persistBinderOrder: (text: string) => provider.createFile(BINDER_SIDECAR_PATH, text),
  });

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

  // Debug session control (D8's breakpoint/pause/step bridged through wasm,
  // #3232): debug.run / stepInto / stepOver / stepOut / breakpointAdd /
  // breakpointRemove / breakpointToggle, gated by the bound provider's
  // "debug" capability. Real plumbing today, inert until a studio compile
  // can carry debug info (#3229) — see `debug-commands.ts`'s own doc.
  registerDebugCommands(commands, store);

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
  // Editor state that survives a reload (decision log 2026-08-26), scoped to
  // the project the host named. Restoring happens HERE, at construction,
  // rather than by setting state afterwards: the store's group-id counter
  // lives in its closure, so a store handed "group-3" after the fact would
  // mint a second "group-3" on the next split.
  //
  // Tabs are filtered against the files this mount actually has. A tab whose
  // file was deleted (or which belongs to a payload edited by hand) is
  // dropped, and `reconcileEditorSnapshot` repairs what the drop invalidates.
  const editorScope = options.sessionScope;
  const storedEditor =
    editorScope === undefined ? null : loadEditorSnapshot(window.localStorage, editorScope);
  const restoredEditor =
    storedEditor === null
      ? null
      : reconcileEditorSnapshot(storedEditor, (ref) => {
          // Tool documents are NOT part of "what I had open". Settings and
          // Compiled Output are things you consult and dismiss, so bringing
          // them back on every launch is noise — and worse, restoring one as
          // the active tab means a reload can land on a document that is not
          // the manuscript at all. The Player is deliberately not in this
          // list: it is half of the default two-up, so restoring it is what
          // keeps a restored session looking like the one you left.
          // Settings is a modal now (#3174) and no longer a document type
          // at all — but a layout persisted before that change can still
          // name one, and without this it would restore as a tab whose type
          // is not registered. Kept deliberately, not left over.
          if (ref.typeId === SETTINGS_TYPE_ID) return false;
          if (ref.typeId === COMPILED_OUTPUT_TYPE_ID) return false;
          // Other non-ink documents (the player, the story graph) have no
          // file behind them and are always available.
          if (ref.typeId !== INK_FILE_TYPE_ID) return true;
          // A symbol tab's docId is "path::symbol"; the file is the path.
          const path = ref.docId.split("::")[0];
          return Object.hasOwn(options.files, path);
        });
  // The layout store is created HERE rather than inside ShellProvider so the
  // takeover commands below — registered outside the React tree — can reach
  // it (decision log 2026-08-26). The provider restores the persisted
  // snapshot into whichever store it is given.
  const shellLayout: ShellLayoutStore = createShellLayoutStore();
  const editorGroups: EditorGroupsStore = createEditorGroupsStore(
    restoredEditor === null
      ? undefined
      : {
          groups: restoredEditor.groups,
          focusedGroupId: restoredEditor.focusedGroupId,
          groupSizes: restoredEditor.groupSizes,
        },
  );
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
  // Settings (#93) is a MODAL, not a document (#3174, ruled 2026-08-27) —
  // consult-and-adjust, so it should not cost you the file you were
  // reading. The document type is gone with the takeover it needed.
  registerSettingsCommand(commands, (section) =>
    store.getState().setSettingsSection(section),
  );

  // Editor text size (beta feedback 2026-08-25). Mod-= / Mod-- / Mod-0 are
  // the universal zoom chords; here they size the EDITOR specifically, which
  // is what an author means by "make the text bigger". Each command persists
  // through the same settings record the Settings document writes, so the
  // choice survives a restart either way it was made.
  const persistFontSize = (px: number): void => {
    const current = loadEditorSettings(window.localStorage);
    saveEditorSettings(window.localStorage, { ...current, fontSize: px });
  };
  const stepFontSize = (delta: number): void => {
    store.getState().adjustEditorFontSize(delta);
    persistFontSize(store.getState().editorFontSize);
  };
  commands.register({
    id: "editor.fontSize.increase",
    title: "Editor: Increase Font Size",
    // ⌘= and ⌘+ (which is ⌘⇧= on most layouts and reports "+").
    keybinding: ["Mod-=", "Mod-Shift-="],
    run: () => stepFontSize(1),
  });
  commands.register({
    id: "editor.fontSize.decrease",
    title: "Editor: Decrease Font Size",
    // "Mod--" is unwritable (it parses as malformed), hence the alias.
    keybinding: ["Mod-Minus", "Mod-Shift-Minus"],
    run: () => stepFontSize(-1),
  });
  commands.register({
    id: "editor.fontSize.reset",
    title: "Editor: Reset Font Size",
    keybinding: "Mod-0",
    run: () => {
      store.getState().setEditorFontSize(DEFAULT_EDITOR_FONT_SIZE);
      persistFontSize(DEFAULT_EDITOR_FONT_SIZE);
    },
  });
  // Story Graph (#97, spec §4.1): custom-rendered, compile-bound singleton
  // over the wasm story-graph query (the component subscribes to storyGraph,
  // refreshed below on each successful compile), with the live session
  // overlay from debugState. Opened via story.openGraph (palette/hamburger);
  // reopening focuses the existing tab.
  documentTypes.register({ id: STORY_GRAPH_TYPE_ID, component: StoryGraphDocument });
  registerStoryGraphCommand(commands, shellLayout);

  // Compile-result handler shared by every path that compiles (per-view
  // debounced compiles, compile.run, the initial compile). DocumentSessions
  // collapses reference-equal (cached) deliveries.
  let fanOutSeq = 0;
  const handleCompileResult = (result: CompileResult): void => {
    // W2d (docs/editor-worker-spec.md): the three panel pulls ride the
    // async session facade at background priority with per-panel coalesce
    // keys, and the store fan-out lands when they resolve — in the same
    // relative order as the old synchronous body. The seq guard applies
    // §5.3's whole-project staleness class: a newer compile's fan-out
    // supersedes this one wholesale (and a query dropped by coalescing
    // rejects, which the catch below folds into the same skip).
    const seq = ++fanOutSeq;
    void (async () => {
      // W4: projectQuery routes these through the worker road when enabled
      // (same coalesce keys, same ordering guarantees).
      const [outline, closure, drafts, graph] = await Promise.all([
        project.projectQuery<FileOutline[]>("getProjectOutline", [], {
          coalesceKey: "panel:outline",
        }),
        project.projectQuery<string[]>("getCompilationClosure", [], {
          coalesceKey: "panel:closure",
        }),
        // Draft status (#3145) rides the closure's fan-out because it is
        // derived from the same compile — pulling it on a different beat
        // would let the Binder show a draft mark against a closure that
        // has already moved on.
        project.projectQuery<string[]>("getDraftPaths", [], {
          coalesceKey: "panel:drafts",
        }),
        // Failed compile: keep the last good graph (same policy as before)
        // without spending the query.
        result.ok
          ? project.projectQuery<StoryGraph | null>("getStoryGraph", [], {
              coalesceKey: "panel:graph",
            })
          : Promise.resolve(null),
      ]);
      if (seq !== fanOutSeq) return; // a newer compile's fan-out supersedes
      landCompileResult(result, outline, closure, drafts, graph);
    })().catch(() => {
      // Dropped/failed panel pull: keep the last good panels — a newer
      // compile is superseding (coalesce keys) or the session is tearing
      // down (cancelled on destroy).
    });
  };

  const landCompileResult = (
    result: CompileResult,
    outline: FileOutline[],
    closure: string[],
    drafts: string[],
    storyGraph: StoryGraph | null,
  ): void => {
    // Fan-out spans (measure-first ruling, 2026-08-24): this handler runs
    // after EVERY debounced compile and is a prime whole-project-work
    // suspect — each phase is timed so a report splits compile-reaction
    // cost from compile cost. The wasm proxy separately times the
    // underlying `wasm.getProjectOutline`/`wasm.getStoryGraph` calls;
    // `store.set.*` spans cover the selector sweeps each `state.*` triggers.
    const endFanOut = perfSpan("studio.compileFanOut");
    const state = store.getState();

    let errors = 0;
    let warnings = 0;
    if (result.warnings) {
      for (const w of result.warnings) {
        // Info/Hint (E189 TODO notes, #3050) are advisory — they belong to
        // neither count, or every TODO would inflate the warning badge.
        if (w.severity === "Error") errors++;
        else if (w.severity === "Warning") warnings++;
      }
    }
    if (result.error) errors++;

    const storyBytes = result.ok && result.story_bytes
      ? new Uint8Array(result.story_bytes)
      : null;

    state.setCompileResult(outline, { errors, warnings }, result.warnings ?? [], storyBytes);
    // The compile closure (#3017): read-only, keyed by the entry this very
    // compile just set — a file in `outline` but not here is on disk, not
    // in the story (the out-of-scope banner + Binder marks read this).
    state.setClosureFiles(closure);
    state.setDraftFiles(drafts);
    // The effective entry (config precedence applied) — the Binder's entry
    // badge and its ink-project Library gate (#3014) read this.
    state.setEntryFile(project.getEntryFile());
    state.appendOutput("compile", compileLogMessage(result.ok, errors, warnings, result.error));

    // Story Graph data (#97, spec §4.1): recompute from the analyzer on each
    // successful compile, like the outline. A failed compile (or a pre-analysis
    // null) keeps the last good graph — same policy as programInkt.
    if (result.ok && storyGraph !== null) state.setStoryGraph(storyGraph);

    // Recompile-while-running (spec §7.6): a successful compile auto-starts
    // the session on the new program through the same code path as the
    // story.start command — startSession replays the recorded choice log,
    // truncating with a notification on divergence. A failed compile takes
    // the `storyBytes === null` branch and leaves the existing session
    // running on the old program.
    if (storyBytes) {
      perfTime("studio.startSession", () => state.startSession(storyBytes));
    }
    endFanOut();
  };

  // Per-(document, group) editor views over wasm document handles. Cursor,
  // line info, auto-pin, focus tracking, and the e2e `__brinkView` hook all
  // flow through these callbacks; the manager keeps them targeted at the
  // focused group's active view.
  documentsRef = null; // (re)assigned immediately below
  const documents = new DocumentSessions(project, {
    onCursorChange: (line, col) => store.getState().setCursor(line, col),
    onLineInfoChange: (info, hints) => store.getState().setLineInfo(info, hints),
    onCompileResult: handleCompileResult,
    // Prose findings into the Problems panel (#3256). Mapped to the
    // `Diagnostic` shape the panel renders, and marked with the
    // `prose:` code prefix that puts them in their own filter bucket —
    // off by default, so they never bury a compile error.
    onProseLints: (path, lints) => {
      store.getState().setProseDiagnostics(path, toProseDiagnostics(path, lints));
    },
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
    // Breakpoints (W4/#3297): the gutter renders the store's source
    // anchors (0-based there, 1-based in the editor — the fencepost lives
    // at this edge and nowhere else). Bound renders solid only while the
    // running program matches the latest compile (suppressed-never-stale).
    getBreakpoints: (path) => {
      const st = store.getState();
      const degraded = sessionDegraded(st.programChecksum, st.compiledChecksum);
      return st.sourceBreakpoints
        .filter((b) => b.file === path)
        .map((b) => ({
          line: b.line + 1,
          state: !b.enabled
            ? ("disabled" as const)
            : b.address !== null && !degraded
              ? ("bound" as const)
              : ("unbound" as const),
        }));
    },
    onToggleBreakpoint: (path, line) =>
      store.getState().breakpointToggleAtLine(path, line - 1),
    onBreakpointsMoved: (path, moves) =>
      store
        .getState()
        .breakpointsMoved(path, moves.map((m) => ({ from: m.from - 1, to: m.to - 1 }))),
    // Execution highlights (W6/#3299 — "play is stepping"). Policy lives
    // in execution-highlights.ts, tested over a real store state.
    getExecutionHighlights: (path) => executionHighlightsFor(store.getState(), path),
    // Right-click a knot/stitch → the shared symbol context menu (rendered by
    // <SymbolContextMenuHost/>).
    onSymbolContextMenu: (info, x, y) =>
      store.getState().openSymbolMenu({ ...info, x, y, source: "editor" }),
    // Everything that isn't a symbol header gets the editor text menu — the
    // native context menu never appears inside the editor
    // (docs/editor-context-menu-spec.md).
    onTextContextMenu: (request) => store.getState().openTextMenu(request),
    // Find References (menu + Shift-Alt-F) presents through the Search
    // panel (context-menu spec ruling) — grouped results, click-to-reveal,
    // inline-editable rows, cross-file included.
    onShowReferences: (symbol, locations, declaration) =>
      store.getState().showReferences(symbol, locations, declaration ?? null),
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
    // Prose checking (#3209). The checker is registered here rather than
    // depended on by the editor package: it lazily imports a 6.5 MB wasm
    // module, so an embedder that never registers one pays nothing at all.
    // The dictionary is NOT passed — it comes from the session, which is the
    // only thing that knows the project's knot and cue names (#3210).
    // "Add to dictionary" IS passed, because the author's own word list
    // lives in `brink.toml` and editing that file is the embedder's job,
    // not the editor package's (decision log, "Prose dictionary lives in
    // `brink.toml`").
  }, [], {
    theme: brinkTheme,
    dialect: options.dialect,
    proseChecker: studioProseChecker,
    onAddToDictionary: (word) => addWordToProjectDictionary(word),
  });

  // File save commands (#154): file.save (Mod-S) / file.saveAll flush
  // editor text to the session and deliver pending host change
  // notifications immediately (bypassing the egress debounce). They work —
  // and notify — with or without an onFilesChanged host hook.
  registerFileCommands(commands, {
    project,
    documents,
    notify: (n) => void notifications.notify(n),
  });
  documentsRef = documents;

  /**
   * Store `word` in `[prose] dictionary` in the project's `brink.toml`.
   *
   * Silently does nothing when the project has no `brink.toml` — there is
   * nowhere to put the word, and inventing a config file as a side effect
   * of a spelling action would be a surprising thing for a tooltip to do.
   * The Prose settings panel already tells an author when their project has
   * no config, which is where that gap belongs.
   */
  function addWordToProjectDictionary(word: string): void {
    const configPath = store
      .getState()
      .outline.find((f) => !f.mounted && isConfigPath(f.path))?.path;
    if (configPath === undefined) return;
    const source = project.getSession().getFileSource(configPath);
    if (source === null) return;
    const next = withDictionaryWord(source, word);
    if (next === null) return; // already present, or blank
    // `applyEdit` re-applies `brink.toml` itself (via `notifyFileChanged`),
    // so the session's `[prose] dictionary` is current by the time the
    // recompile below asks for it. A second explicit apply here would
    // double-apply the config, which `project-config-application.test.ts`
    // catches by counting warning batches.
    project.applyEdit(configPath, next);
    documentsRef?.refreshExternal(configPath);
    // The dictionary cache is keyed on the project's analysis, and this edit
    // changed the config rather than any manuscript file — so invalidate it
    // explicitly, or the checker keeps its pre-edit word list and the word
    // stays underlined. That is precisely the "it did nothing" report.
    project.invalidateProseDictionary();
    documentsRef?.triggerCompile();
  }

  // The store's document opener (Binder rows, addFile): note the target so
  // symbol mounts can fall back to the outline range, then open through the
  // shell's groups store (which applies the §7.8 reveal policy).
  store.getState().setDocumentOpener((target, pinned) => {
    documents.noteTarget(target);
    // Continuous view renders FILES, so a symbol target has to become a
    // position within one. Opening `path::symbol` there did nothing visible:
    // it is a different document, and this view never mounts it — clicking a
    // knot in the Binder's structure mode simply sat there.
    //
    // Everything else already works, because every other navigation surface
    // (search, Problems, go-to-definition) reveals a FILE plus a span, which
    // is exactly the shape this turns a symbol into.
    // `brink.toml` opens as the Settings TAKEOVER, in every view (ruled
    // 2026-08-27, #3166). It has no home in Continuous view — that view
    // renders the project's MANUSCRIPT, and the config file is deliberately
    // filtered out of it (`binderOrderedFiles`), so clicking it there did
    // nothing at all. Routing to Settings answers that once rather than
    // per-view, and puts project settings where app settings already live.
    //
    // Settings' Project section carries the WHOLE config document — the
    // form and the raw text under it — so nothing an editor tab could do is
    // lost. That matters: the form models four keys, and #3015 ruled the
    // text below it to be the escape hatch for everything else, which now
    // includes `drafts` and `indent`.
    if (target.kind === "file" && isConfigPath(target.path)) {
      store.getState().setSettingsSection(SETTINGS_SECTION_IDS.general);
      return;
    }
        if (target.kind === "symbol" && shellLayout.getState().editorView === "continuous") {
      editorGroups
        .getState()
        .openDocument(inkFileRef({ kind: "file", path: target.path }), { pinned });
      documents.revealAt(target.path, target.start);
      return;
    }
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
  // group per the §7.8 reveal policy) and scrolls to the span. All three
  // non-source spaces register here (W3/#3296): symbol over the compile
  // outline, session → program over a position-shaped ref, program →
  // source through the live provider's DebugInfo road, degraded-gated.
  const locations = new LocationResolvers();
  registerLocationResolvers(locations, store);
  const revealSource = (target: SourceLocation): void => {
    // Reveal opens the file as the group's PREVIEW tab, not a pinned one.
    // `editor.reveal` is the shared destination of every navigation surface —
    // search cards, Problems, TODOs, Find References, cross-file
    // go-to-definition — so opening pinned meant each jump minted a permanent
    // tab and a browsing session buried the strip (beta feedback, 2026-08-25).
    // Preview semantics are already implemented end to end: the next preview
    // replaces this one in place, editing it auto-pins (onDocEdited), a
    // double-click pins it, and opening a file that is ALREADY pinned leaves
    // it pinned (editor-groups only ever upgrades preview -> pinned, never the
    // reverse).
    //
    // NOTE: this is a deliberate stopgap, not the endgame. The maintainer
    // wants a single "main editor" mode with tabs as an opt-in gesture
    // (Inky-style); that design supersedes this and is tracked separately.
    // Revealing source means "take me to the code", so anything that has
    // taken the editor area over steps aside — otherwise clicking a node in
    // the Story Graph reveals a location underneath the graph still covering
    // it, and nothing appears to happen (caught by the graph's own e2e once
    // the takeover landed).
    shellLayout.getState().setTakeover(null);
    store.getState().openTarget({ kind: "file", path: target.file }, false);
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
  (window as unknown as Record<string, unknown>).__brinkStore = store;
  // Dev double-mounts (StrictMode / playground remounts) make a single
  // handle ambiguous — keep them all so a probe can tell dead from live.
  {
    const w = window as unknown as { __brinkStores?: unknown[] };
    (w.__brinkStores ??= []).push(store);
  }
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
    actions: ProblemsActions,
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
  // TODOs (#3050): author notes over the E189 diagnostics the compile
  // already carries. Right/end by default (maintainer placement request) —
  // the bottom dock stays the compile-feedback row (Problems/Output).
  toolWindows.register({
    id: "todos",
    title: "TODOs",
    icon: TODO_ICON,
    defaultPlacement: { dock: "right", section: "end" },
    defaultOpen: false,
    badge: TodosBadge,
    actions: TodosActions,
    component: TodosView,
  });
  // Performance HUD (prod-perf ruling 2026-08-25): all builds, closed by
  // default — it costs nothing until opened. `perf: false` strips it.
  if (perfOn) {
    const StudioPerfView = () => <PerfView bridge={perfBridge} />;
    toolWindows.register({
      id: "perf",
      title: "Performance",
      icon: PERF_ICON,
      defaultPlacement: { dock: "bottom", section: "end" },
      defaultOpen: false,
      component: StudioPerfView,
    });
  }

  // Status-bar segments (spec §7.3). Higher priority renders further left
  // within its group. Left: app status; right: editor context.
  const statusBarItems = new StatusBarRegistry();
  statusBarItems.register({
    id: "status.compile",
    alignment: "left",
    priority: 20,
    component: CompileStatusSegment,
  });
  // Out-of-scope note (#3017): sits right after the compile status —
  // "No issues — file not analyzed" reads as one statement, which is the
  // point (absent diagnostics are not clean diagnostics here).
  statusBarItems.register({
    id: "status.scope-note",
    alignment: "left",
    priority: 19,
    component: ScopeNoteSegment,
  });
  statusBarItems.register({
    id: "status.story",
    alignment: "left",
    priority: 10,
    component: StorySegment,
  });
  // Gated structural-op busy indicator (#2767/#2769) — a local status-bar
  // affordance, not a notification (spec §7.5's "out of scope: progress
  // notifications"). Sits between story status and the session picker;
  // renders nothing (StructuralOpSegment returns null) outside the brief
  // window one of moveStitch/promoteStitch/demoteKnot is deferred, or the
  // Binder's file/folder rename-and-move (#2776, `applyRename` in
  // `binder.ts`) is.
  statusBarItems.register({
    id: "status.structural-op",
    alignment: "left",
    priority: 9,
    component: StructuralOpSegment,
  });
  // Multi-session picker (#182) — sits just after the story status, hidden
  // until there's more than one session.
  statusBarItems.register({
    id: "status.sessions",
    alignment: "left",
    priority: 8,
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

  // Restore a persisted debug-info opt-out the same way (W1/#3294: on by
  // default; only an explicit opt-out changes anything, and it must land
  // before the first compile so the first bytes already honour it).
  store.getState().setDebugInfoEnabled(
    loadDebugSettings(window.localStorage).emitDebugInfo,
  );

  // Breakpoints persist per project (W4/#3297, ruled 2026-08-29) under the
  // same per-project scope the editor-tab snapshot uses. No scope (an
  // embedder without session persistence) → session-local breakpoints.
  if (editorScope !== undefined) {
    store.getState().applyPersistedBreakpoints(
      loadBreakpoints(window.localStorage, editorScope),
    );
    store.getState().setBreakpointsSink((list) =>
      saveBreakpoints(window.localStorage, editorScope, list),
    );
  }

  // Bind handles, kick the initial compile, and open the entry file (the
  // groups-store subscription above keeps focus tracking in sync as the
  // document component mounts).
  store.getState().initialize(project, documents);

  // Breakpoint dots re-render on anchor changes AND on checksum changes
  // (bound⇄unbound is a function of degraded-ness, W4/#3297); the
  // execution highlight re-renders whenever the runtime position,
  // paused-ness, or status moves (W6/#3299). The subscription lives in
  // `debug-refresh-subscription.ts` — its re-entrancy discipline is
  // load-bearing and tested there.
  subscribeDebugRefresh(store, {
    refreshBreakpoints: () => documents.refreshBreakpoints(),
    refreshExecutionHighlight: () => documents.refreshExecutionHighlight(),
    revealProgram: (containerIdx, offset) =>
      commands.dispatch(EDITOR_REVEAL_COMMAND_ID, {
        kind: "program",
        address: encodeProgramAddress(containerIdx, offset),
      }),
  });

  // Restore the persisted editor settings (Settings → Editor). After initialize,
  // so the actions reach `documents`; new views read them from slotOptions, open
  // ones get the live switch.
  {
    const editor = loadEditorSettings(window.localStorage);
    store.getState().setFormGlyph(editor.formGlyph);
    store.getState().setShowGutters(editor.showGutters);
    store.getState().setAutoOpenForm(editor.autoOpenForm);
    store.getState().setEditorFontSize(editor.fontSize);
    store.getState().setAppFontSize(editor.appFontSize);
  }
  // Problems panel view preferences (ruled 2026-08-25: grouped by default,
  // and the toggles persist). The filter text deliberately does not.
  {
    store.getState().applyProblemsPrefs(loadProblemsPrefs(window.localStorage));
    store
      .getState()
      .setProblemsPrefsSink((prefs) => saveProblemsPrefs(window.localStorage, prefs));
  }
  // TODOs panel: grouping persists, the tag selection does not. A tag is a
  // property of THIS project's notes — restoring `(audio)` into a project
  // that has no such tag would filter the panel empty with no visible
  // cause, the same reason the filter text is not persisted either.
  {
    store.getState().applyTodosPrefs(loadTodosPrefs(window.localStorage));
    store.getState().setTodosPrefsSink((prefs) => saveTodosPrefs(window.localStorage, prefs));
  }
  // `project.getEntryFile()`, not the raw `entryFile` option (issue #2331,
  // ruled 2026-08-07 "`[project] entry` beats `mountStudio`'s `entryFile`"):
  // `project.initialize()` above already ran `brink.toml` discovery, so by
  // this point `ProjectSession` may have superseded the constructor
  // argument with a config-named entry — reading it back here is how the
  // ruling actually reaches the initial tab.
  if (restoredEditor === null) {
    store.getState().openTarget({ kind: "file", path: project.getEntryFile() }, true);

    // Default layout (spec §4): the Inky two-up — entry file left, player in
    // a right split, focus back on the editor. This is what a project with
    // nothing remembered opens as.
    openPlayerSplit(editorGroups);
  } else {
    // A restored session brings its own tabs and splits, so neither the
    // entry-file open nor the two-up runs — both would fight what the author
    // last had on screen.
    //
    // Cursor and scroll ride `restoreViewState`, which queues against the
    // view and applies when it mounts, so replaying every open tab here is
    // correct even though none of them has mounted yet. It restores the
    // selection and scroll WITHOUT focusing, which is why replaying all of
    // them does not fight over focus.
    // Two different keys meet here. The snapshot is keyed by the shell's
    // `documentKey(ref)`; `DocumentSessions` files its views under the ref's
    // bare `docId` (what `InkFileDocument` hands `mountView`). Walking the
    // restored tabs gives both without parsing either.
    //
    // The map is per DOCUMENT, not per (document, group), so a document open
    // in two groups restores the same cursor and scroll to both rather than
    // each pane independently — `restoreViewState` supports the finer
    // addressing if that is ever worth persisting.
    for (const group of restoredEditor.groups) {
      for (const tab of group.tabs) {
        const viewState = restoredEditor.viewStates[documentKey(tab.ref)];
        if (viewState !== undefined) documentsRef?.restoreViewState(tab.ref.docId, viewState);
      }
    }
  }

  // Persist from here on: debounced writes of the structure, with each open
  // tab's cursor + scroll read at write time so a snapshot is never stale.
  const detachEditorPersistence =
    editorScope === undefined
      ? null
      : attachEditorPersistence(editorGroups, window.localStorage, {
          scope: editorScope,
          viewStates: () => {
            const out: Record<string, { anchor: number; head: number; scrollTop: number }> = {};
            const sessions = documentsRef;
            if (sessions === null) return out;
            for (const group of editorGroups.getState().groups) {
              for (const tab of group.tabs) {
                const key = documentKey(tab.ref);
                if (key in out) continue;
                // Read by `docId` (DocumentSessions' own key), store by the
                // shell's `documentKey` so `reconcileEditorSnapshot` can
                // prune this map against the tabs it keeps.
                const state = sessions.viewState(tab.ref.docId);
                if (state !== null) out[key] = state;
              }
            }
            return out;
          },
        });

  perfMark("studio.renderStart");
  if (perfOn) {
    // First frame after the initial render commit — the end of the startup
    // timeline (project-open → wasm → initialize → first paint).
    requestAnimationFrame(() => perfMark("studio.firstFrame"));
  }
  const root = createRoot(container);
  const appTree = (
    <Root
      store={store}
      project={project}
      documents={documents}
      commands={commands}
      toolWindows={toolWindows}
      statusBarItems={statusBarItems}
      documentTypes={documentTypes}
      editorGroups={editorGroups}
      layout={shellLayout}
      notifications={notifications}
      api={api}
    />
  );
  root.render(
    perfOn ? (
      // React commit durations feed the probe. Note the prod caveat:
      // react-dom's production bundle no-ops <Profiler> onRender unless
      // the host aliases the profiling build — the wrapper itself is
      // harmless there, and `react.commit.*` spans simply don't appear.
      <Profiler
        id="studio"
        onRender={(_id, phase, actualDuration, _base, startTime) =>
          perfRecord(`react.commit.${phase}`, startTime, actualDuration)
        }
      >
        {appTree}
      </Profiler>
    ) : (
      appTree
    ),
  );

  return {
    api,
    // `project.initialize()` (above) already ran `brink.toml` discovery, so
    // this reads the FINAL resolved entry — see `StudioHandle.entryFile`'s
    // doc comment.
    entryFile: project.getEntryFile(),
    // Unmounting runs Root's cleanup effect: dispose session + views + project.
    // Editor views unmount (child effects) before Root's cleanup runs, so the
    // egress flush must happen first, while the views still exist: push every
    // mounted view's text, then deliver pending host notifications (#154).
    unmount: () => {
      // Flush the last editor-state write before the views go away — its
      // `viewStates` callback reads them, and after teardown there is
      // nothing left to read.
      detachEditorPersistence?.();
      documents.flushAll();
      project.flushFileChanges();
      root.unmount();
    },
  };
}
