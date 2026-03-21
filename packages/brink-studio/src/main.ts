import { EditorView } from "@codemirror/view";
import { initWasm, StoryRunnerHandle } from "./wasm.js";
import { createBrinkEditor } from "./editor/index.js";
import { createBrinkPlayer } from "./player/index.js";
import { createBinder } from "./binder/index.js";
import { InMemoryFileProvider } from "./provider.js";
import { ProjectSession } from "./project-session.js";
import { EditorStateManager } from "./editor/state-manager.js";
import { createFileTabBar } from "./editor/file-tabs.js";

const DEFAULT_INK = `// A short screenplay-style demo.
// Try Tab on a blank line below another blank line to start a character.

VAR tension = 0

-> opening

=== opening ===
The lights dim. A single bulb swings above a metal table.

@DETECTIVE:<>
(leaning forward)<>
Where were you last night?

@SUSPECT:<>
(quietly)<>
I was at home. Alone.

@DETECTIVE:<>
Alone. Convenient.

~ tension = tension + 1

* [Press harder]
  -> interrogation.pressure
* [Change the subject]
  -> interrogation.redirect
* [Show the evidence]
  -> evidence

=== interrogation ===

= pressure
@DETECTIVE:<>
(slamming the table)<>
We have witnesses who say otherwise.

@SUSPECT:<>
Then your witnesses are liars.

~ tension = tension + 1

* {tension >= 2} [Suspect is cracking — show the photo]
  -> evidence
* [Back off for now]
  -> interrogation.redirect

= redirect
@DETECTIVE:<>
(standing, walking to the window)<>
Nice night out there. You like the harbour?

@SUSPECT:<>
(shifting uncomfortably)<>
What does that have to do with anything?

* [Go back to the pressure]
  -> interrogation.pressure
* [Wrap it up]
  -> ending

=== evidence ===
The detective slides a photograph across the table.

@DETECTIVE:<>
Recognise this?

@SUSPECT:<>
(long pause)<>
...where did you get that?

@DETECTIVE:<>
(smiling)<>
I ask the questions.

-> ending

=== ending ===
The tape recorder clicks off. The detective stands.

@DETECTIVE:<>
That will be all. For now.

The suspect is led from the room.
-> END
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

  // Expose for e2e tests
  (window as any).__brinkView = editor.view;

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
