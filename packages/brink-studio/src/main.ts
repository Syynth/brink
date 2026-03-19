import { EditorView } from "@codemirror/view";
import { initWasm, StoryRunnerHandle } from "./wasm.js";
import { createBrinkEditor } from "./editor/index.js";
import { createBrinkPlayer } from "./player/index.js";
import { createBinder } from "./binder/index.js";
import { InMemoryFileProvider } from "./provider.js";
import { ProjectSession } from "./project-session.js";
import { EditorStateManager } from "./editor/state-manager.js";
import { createFileTabBar } from "./editor/file-tabs.js";

const DEFAULT_INK = `// Welcome to brink studio!
// Edit this ink story and watch it run.

-> start

=== start ===
Hello, world!
What would you like to do?

* [Tell me a story]
  -> story
* [Goodbye]
  Farewell!
  -> END

=== story ===
Once upon a time, there was a little compiler.
It worked very hard to understand stories.
* [And then?]
  And then it got everything right!
  -> END
* [Go back]
  -> start
`;

async function main(): Promise<void> {
  await initWasm();

  const loading = document.getElementById("loading");
  if (loading) loading.remove();

  const playerContainer = document.getElementById("player");
  const editorContainer = document.getElementById("editor");
  const editorPane = document.getElementById("editor-pane");

  if (!playerContainer || !editorContainer || !editorPane) {
    throw new Error("Missing #player, #editor, or #editor-pane containers");
  }

  const player = createBrinkPlayer(
    playerContainer,
    (bytes) => new StoryRunnerHandle(bytes),
  );

  // Set up project session
  const provider = new InMemoryFileProvider({ "main.ink": DEFAULT_INK });
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();

  // Mount binder panel if container exists
  const binderContainer = document.getElementById("binder-pane");
  let binder: ReturnType<typeof createBinder> | undefined;

  // Build studio options with callbacks
  const studioOptions = {
    ...project.createStudioOptions(),
    onCompile(result: import("./wasm.js").CompileResult) {
      if (result.ok && result.story_bytes) {
        player.loadStory(new Uint8Array(result.story_bytes));
      }
      binder?.refresh();
      tabs.refresh();
    },
    onNavigateToFile(location: import("./wasm.js").Location) {
      void manager.openTab({ kind: "file" as const, path: location.file }, true).then(() => {
        tabs.refresh();
        binder?.refresh();
        // Scroll to the definition after the state swap
        const view = manager.getView();
        view.dispatch({
          selection: { anchor: location.start },
          effects: EditorView.scrollIntoView(location.start, { y: "center" }),
        });
      });
    },
  };

  // Create state manager and editor
  const manager = new EditorStateManager(project, studioOptions);
  const initialState = manager.getState(project.getActiveFile());

  const editor = createBrinkEditor(editorContainer, {
    ...studioOptions,
    initialContent: "",
    initialState,
  });

  manager.setView(editor.view);

  // Listen for tab changes dispatched by the binder
  editor.view.dom.addEventListener("brink-tab-changed", () => {
    tabs.refresh();
    binder?.refresh();
  });

  // Mount file tab bar above the editor
  const tabs = createFileTabBar({
    manager,
    onSwitch() {
      binder?.refresh();
    },
  });
  editorPane.insertBefore(tabs.element, editorContainer);

  if (binderContainer) {
    // Replace placeholder content
    const placeholder = binderContainer.querySelector(".placeholder");
    if (placeholder) placeholder.remove();

    binder = createBinder({
      manager,
      onFileCreated: () => {
        tabs.refresh();
      },
    });

    binderContainer.appendChild(binder.element);
  }

  document.getElementById("btn-run")?.addEventListener("click", () => {
    editor.triggerCompile();
  });

  document.getElementById("btn-restart")?.addEventListener("click", () => {
    player.reset();
  });
}

main();
