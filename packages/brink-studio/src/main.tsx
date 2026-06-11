import { createRoot } from "react-dom/client";
import { useEffect } from "react";
import { initWasm } from "@brink/wasm";
import type { CompileResult, FileOutline } from "@brink/wasm-types";
import {
  DocumentSessions,
  ProjectSession,
  InMemoryFileProvider,
} from "@brink/ink-editor";
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
  resolveQualifiedSymbol,
  type EditorGroupsState,
  type EditorGroupsStore,
  type Location as ShellLocation,
  type SourceLocation,
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
  PlayerPane,
  ProblemsBadge,
  ProblemsView,
  ProgramView,
  StateView,
  StorySegment,
  StoreProvider,
  inkFileRef,
  registerCompiledOutputCommand,
} from "@brink/studio-ui";
import { registerStoryCommands } from "./story-commands.js";
import toppledTemple from "./stories/toppled-temple.ink.txt?raw";

const MAIN_INK = `INCLUDE toppled-temple.ink

-> intro
`;

// Deterministic single-file project for e2e, loaded via `?fixture=screenplay`.
// This decouples the binder/decorations/stitches specs from the demo default
// above (which is multi-file and has no top-level knots). Not used in normal
// app usage — only when the query param is present.
const SCREENPLAY_FIXTURE = `// A short screenplay-style demo.
-> opening

=== opening ===
The lights dim.
A figure steps into the light.
-> evidence

=== interrogation ===
= evidence
"Where were you that night?"
-> END
`;

// ── Tool-window icons ──────────────────────────────────────────
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

const PLAYER_ICON = (
  <svg {...iconProps}>
    <path d="M5 3.5v9l8-4.5z" fill="currentColor" stroke="none" />
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
        <App />
      </StoreProvider>
    </ShellProvider>
  );
}

// ── Bootstrap ──────────────────────────────────────────────────

// HMR guard (dev only). Under Vite HMR an update that reaches this entry
// re-executes the whole module, so without a guard each edit stacks another
// createRoot() on #app and orphans the previous wasm EditorSession. The old
// instance's dispose hook unmounts its root *before* the new instance mounts
// (Root's unmount effect already disposes the player and frees the wasm
// session — HMR just never triggered it). The generation counter lets a
// superseded main() — disposed while still awaiting init — bail out instead
// of mounting a second root.
interface HotData {
  generation?: number;
  teardown?: () => void;
}
const hotData = import.meta.hot?.data as HotData | undefined;
const generation = hotData ? (hotData.generation = (hotData.generation ?? 0) + 1) : 0;

function superseded(): boolean {
  return hotData !== undefined && hotData.generation !== generation;
}

async function main(): Promise<void> {
  await initWasm();
  if (superseded()) return;

  const loading = document.getElementById("loading");
  if (loading) loading.remove();

  // Initialize project BEFORE rendering so the wasm session has files loaded.
  // `?fixture=screenplay` loads a deterministic single-file project for e2e.
  const fixture = new URLSearchParams(window.location.search).get("fixture");
  const files: Record<string, string> =
    fixture === "screenplay"
      ? { "main.ink": SCREENPLAY_FIXTURE }
      : {
          "main.ink": MAIN_INK,
          "toppled-temple.ink": toppledTemple,
        };
  const provider = new InMemoryFileProvider(files);
  const entryFile = "main.ink";
  const project = new ProjectSession({ provider, entryFile });
  await project.initialize();
  if (superseded()) {
    project.destroy();
    return;
  }

  const store = createStudioStore();

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
  });

  // The store's document opener (Binder rows, addFile): note the target so
  // symbol mounts can fall back to the outline range, then open through the
  // shell's groups store (which applies the §7.8 reveal policy).
  store.getState().setDocumentOpener((target, pinned) => {
    documents.noteTarget(target);
    editorGroups.getState().openDocument(inkFileRef(target), { pinned });
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
  // user-visible Mod-N ordering: Binder Mod-1, Player Mod-2, State Mod-3,
  // Program Mod-4, Problems Mod-5, Output Mod-6. The shell never imports
  // these components — they are registered into it here, at the app boundary.
  //
  // Bottom-dock sharing: Program Explorer and Problems both default to
  // bottom/start (spec §4) — a section holds multiple windows, one open at a
  // time, the strip tabs between them. Output takes bottom/end.
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
    id: "player",
    title: "Player",
    icon: PLAYER_ICON,
    defaultPlacement: { dock: "right", section: "start" },
    defaultOpen: true,
    component: PlayerPane,
  });
  toolWindows.register({
    id: "state",
    title: "State View",
    icon: STATE_ICON,
    defaultPlacement: { dock: "right", section: "end" },
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

  // Bind handles, kick the initial compile, and open the entry file (the
  // groups-store subscription above keeps focus tracking in sync as the
  // document component mounts).
  store.getState().initialize(project, documents);
  store.getState().openTarget({ kind: "file", path: entryFile }, true);

  const appRoot = document.getElementById("app");
  if (!appRoot) throw new Error("Missing #app container");

  const root = createRoot(appRoot);
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
    />,
  );

  if (hotData) {
    // Unmounting runs Root's cleanup effect: dispose session + views + project.
    hotData.teardown = () => root.unmount();
  }
}

main();

if (import.meta.hot) {
  import.meta.hot.dispose((data: HotData) => {
    data.teardown?.();
    data.teardown = undefined;
  });
}
