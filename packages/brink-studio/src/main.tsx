import { createRoot } from "react-dom/client";
import { useRef, useEffect, useCallback } from "react";
import { initWasm, StoryRunnerHandle } from "@brink/wasm";
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
import { App, StoreProvider } from "@brink/studio-ui";
import { EditorView } from "@codemirror/view";
import { forwardRef, useImperativeHandle } from "react";
import type { BrinkStudioOptions } from "@brink/ink-editor";
import ratchetsLair from "./stories/ratchets-lair.ink.txt?raw";
import candlelitDaughters from "./stories/candlelit-daughters.ink.txt?raw";

const MAIN_INK = `INCLUDE ratchets-lair.ink
INCLUDE candlelit-daughters.ink

Which story would you like to play?

* [The Quest for Ratchet's Lair]
  -> intro
* [Codetta: The Candlelit Daughters]
  -> introduction
`;

// The old inline story has been moved to stories/ratchets-lair.ink.txt

// ── Root component ─────────────────────────────────────────────

interface RootProps {
  store: StudioStore;
  project: ProjectSession;
  studioOptions: BrinkStudioOptions;
}

function Root({ store, project, studioOptions }: RootProps) {
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
        // onCompileResult reads from refs, so it's fine to close over it indirectly
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

  // Create manager once
  if (!managerRef.current) {
    managerRef.current = new EditorStateManager(project, fullOptions.current);
  }

  const manager = managerRef.current;
  const initialState = manager.getState(project.getActiveFile());

  // Initialize store with refs after first render
  useEffect(() => {
    const editor = editorRef.current;
    if (editor && manager) {
      store.getState().initialize(project, manager, editor);
      manager.setView(editor.getView());
      // Expose for e2e tests
      (window as any).__brinkView = editor.getView();
    }
  }, [store, project, manager]);

  return (
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
  );
}

// ── Bootstrap ──────────────────────────────────────────────────

async function main(): Promise<void> {
  await initWasm();

  const loading = document.getElementById("loading");
  if (loading) loading.remove();

  // Initialize project BEFORE rendering so the wasm session has files loaded
  const provider = new InMemoryFileProvider({
    "main.ink": MAIN_INK,
    "ratchets-lair.ink": ratchetsLair,
    "candlelit-daughters.ink": candlelitDaughters,
  });
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();

  const studioOptions = project.createStudioOptions();
  const store = createStudioStore();

  const appRoot = document.getElementById("app");
  if (!appRoot) throw new Error("Missing #app container");

  const root = createRoot(appRoot);
  root.render(<Root store={store} project={project} studioOptions={studioOptions} />);
}

main();
