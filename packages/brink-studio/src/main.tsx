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
import { App, StoreProvider } from "@brink/studio-ui";
import { EditorView } from "@codemirror/view";
import type { Extension } from "@codemirror/state";
import {
  elementTypeField,
  getHintsForElement,
  lineHasContent,
  buildContext,
} from "@brink/ink-editor";
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

// ── Root component ─────────────────────────────────────────────

interface RootProps {
  store: StudioStore;
  project: ProjectSession;
  studioOptions: BrinkStudioOptions;
  updateListener: Extension;
}

function Root({ store, project, studioOptions, updateListener }: RootProps) {
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
    />,
  );
}

main();
