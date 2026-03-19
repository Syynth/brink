import { initWasm, StoryRunnerHandle } from "./wasm.js";
import { createBrinkEditor } from "./editor/index.js";
import { createBrinkPlayer } from "./player/index.js";
import { createBinder } from "./binder/index.js";
import { InMemoryFileProvider } from "./provider.js";
import { ProjectSession } from "./project-session.js";

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

  if (!playerContainer || !editorContainer) {
    throw new Error("Missing #player or #editor containers");
  }

  const player = createBrinkPlayer(
    playerContainer,
    (bytes) => new StoryRunnerHandle(bytes),
  );

  const provider = new InMemoryFileProvider({ "main.ink": DEFAULT_INK });
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();

  // Mount binder panel if container exists
  const binderContainer = document.getElementById("binder");
  let binder: ReturnType<typeof createBinder> | undefined;

  const editor = createBrinkEditor(editorContainer, {
    ...project.createEditorOptions(),
    onCompile(result) {
      if (result.ok && result.story_bytes) {
        player.loadStory(new Uint8Array(result.story_bytes));
      }
      binder?.refresh();
    },
  });

  if (binderContainer) {
    binder = createBinder({
      session: project.getSession(),
      onNavigate: (_path, offset) => {
        if (offset != null) {
          editor.view.dispatch({
            selection: { anchor: offset },
            scrollIntoView: true,
          });
        }
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
