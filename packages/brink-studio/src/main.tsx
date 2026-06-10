import { createRoot } from "react-dom/client";
import { useRef, useEffect, useCallback } from "react";
import { initWasm } from "@brink/wasm";
import type { CompileResult, FileOutline, Location } from "@brink/wasm-types";
import {
  InkEditor,
  type InkEditorHandle,
  type KeyHint,
  type LineInfo,
  EditorStateManager,
  ProjectSession,
  InMemoryFileProvider,
} from "@brink/ink-editor";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import { CommandRegistry, ShellProvider, ToolWindowRegistry } from "@brink/studio-shell";
import {
  App,
  Binder,
  PlayerPane,
  ProgramView,
  StateView,
  StoreProvider,
} from "@brink/studio-ui";
import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import {
  elementTypeField,
  getHintsForElement,
  lineHasContent,
  buildContext,
} from "@brink/ink-editor";
import type { BrinkStudioOptions } from "@brink/ink-editor";
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

// ── Root component ─────────────────────────────────────────────

interface RootProps {
  store: StudioStore;
  project: ProjectSession;
  studioOptions: BrinkStudioOptions;
  updateListener: Extension;
  commands: CommandRegistry;
  toolWindows: ToolWindowRegistry;
}

function Root({ store, project, studioOptions, updateListener, commands, toolWindows }: RootProps) {
  const editorRef = useRef<InkEditorHandle>(null);
  const managerRef = useRef<EditorStateManager | null>(null);

  // Callbacks for InkEditor → Store
  const onCursorChange = useCallback((line: number, col: number) => {
    store.getState().setCursor(line, col);
  }, [store]);

  const onLineInfoChange = useCallback((info: LineInfo | null, hints: KeyHint[]) => {
    store.getState().setLineInfo(info, hints);
  }, [store]);

  const onCompileResult = useCallback((result: CompileResult) => {
    const state = store.getState();
    const session = project.getSession();
    const outline: FileOutline[] = session.getProjectOutline();

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

    state.setCompileResult(outline, { errors, warnings }, storyBytes);

    if (storyBytes) {
      state.loadStory(storyBytes);
    }
  }, [store, project]);

  const onDocEdited = useCallback(() => {
    store.getState().pinActiveTab();
  }, [store]);

  // Build full studio options with navigation wired to the store
  const fullOptions = useRef<BrinkStudioOptions | null>(null);
  if (!fullOptions.current) {
    fullOptions.current = {
      ...studioOptions,
      onCompile(result: CompileResult) {
        const state = store.getState();
        const session = project.getSession();
        const outline: FileOutline[] = session.getProjectOutline();

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

        state.setCompileResult(outline, { errors, warnings }, storyBytes);
        if (storyBytes) {
          state.loadStory(storyBytes);
        }
      },
      onNavigateToFile(location: Location) {
        const manager = managerRef.current;
        if (!manager) return;
        void manager.openTab({ kind: "file" as const, path: location.file }, true).then(() => {
          const tabs = [...manager.getTabs()];
          const activeTab = manager.getActiveTab();
          store.setState({ tabs, activeTabId: activeTab.id });
          const view = manager.getView();
          view.dispatch({
            selection: { anchor: location.start },
            effects: EditorView.scrollIntoView(location.start, { y: "center" }),
          });
        });
      },
    };
  }

  // Create manager once — pass the updateListener so every state
  // it creates (including for tab switches) has the React callbacks.
  if (!managerRef.current) {
    managerRef.current = new EditorStateManager(
      project,
      fullOptions.current,
      [updateListener],
    );
  }

  const manager = managerRef.current;
  const initialState = manager.getState(project.getActiveFile());

  // Initialize store with refs after first render
  useEffect(() => {
    const editor = editorRef.current;
    if (editor && manager) {
      store.getState().initialize(project, manager, editor);
      manager.setView(editor.getView());
      (window as any).__brinkView = editor.getView();
    }
  }, [store, project, manager]);

  // Tear down the wasm session + story runner when the app unmounts. The
  // standalone playground never unmounts, but the embeddable/host case does —
  // this keeps the lifecycle owned instead of leaking the cached parse/HIR.
  useEffect(
    () => () => {
      store.getState().disposePlayer();
      project.destroy();
    },
    [store, project],
  );

  return (
    <ShellProvider commands={commands} toolWindows={toolWindows}>
      <StoreProvider store={store}>
        <App
          editorSlot={
            <InkEditor
              ref={editorRef}
              studioOptions={fullOptions.current}
              initialState={initialState}
              onCursorChange={onCursorChange}
              onLineInfoChange={onLineInfoChange}
              onCompileResult={onCompileResult}
              onDocEdited={onDocEdited}
            />
          }
        />
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
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();
  if (superseded()) {
    project.destroy();
    return;
  }

  const studioOptions = project.createStudioOptions();
  const store = createStudioStore();

  // Shell command registry (spec §6). ShellProvider owns the keymap and the
  // global key handler, and generates the `view.toggle.<id>` commands
  // (Mod-1…9 by registration order) from the tool-window registry below.
  const commands = new CommandRegistry();
  commands.register({
    id: "story.restart",
    title: "Story: Restart",
    when: () => store.getState().storyBytes !== null,
    run: () => store.getState().resetStory(),
  });
  // Exposed for e2e/manual verification, like __brinkView.
  (window as unknown as Record<string, unknown>).__brinkCommands = commands;

  // Tool-window registry (spec §7.1, §4). Registration order is the stable,
  // user-visible Mod-N ordering: Binder Mod-1, Player Mod-2, State Mod-3,
  // Program Mod-4. The shell never imports these components — they are
  // registered into it here, at the app boundary.
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

  // Create the updateListener eagerly so it can be shared between
  // InkEditor (for the initial state) and EditorStateManager (for
  // tab-switch states). It reads callbacks from the store, so it
  // doesn't need to be recreated when callbacks change.
  const updateListener = EditorView.updateListener.of((update) => {
    const state = store.getState();

    if (update.docChanged) {
      state.pinActiveTab();
    }

    if (update.docChanged || update.selectionSet) {
      const { state: editorState } = update.view;
      const pos = editorState.selection.main.head;
      const line = editorState.doc.lineAt(pos);
      const col = pos - line.from;

      state.setCursor(line.number, col + 1);

      const infos = editorState.field(elementTypeField);
      const info = infos[line.number - 1] ?? null;

      let hints: { key: string; hint: string }[] = [];
      if (info) {
        const hasContent = lineHasContent(line.text, info);
        const lineCtx = buildContext(infos, line.number - 1);
        hints = getHintsForElement(info, hasContent, lineCtx);
      }

      state.setLineInfo(info, hints);
    }
  });

  const appRoot = document.getElementById("app");
  if (!appRoot) throw new Error("Missing #app container");

  const root = createRoot(appRoot);
  root.render(
    <Root
      store={store}
      project={project}
      studioOptions={studioOptions}
      updateListener={updateListener}
      commands={commands}
      toolWindows={toolWindows}
    />,
  );

  if (hotData) {
    // Unmounting runs Root's cleanup effect: disposePlayer + project.destroy.
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
