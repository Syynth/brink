import {
  initWasm,
  compile,
  getTokenTypeNames,
  EditorSessionHandle,
  StoryRunnerHandle,
} from "./wasm.js";
import { createBrinkEditor } from "./editor/index.js";
import { createBrinkPlayer } from "./player/index.js";

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

  // Create stateful editor session for HIR-backed IDE features
  const session = new EditorSessionHandle();

  const editor = createBrinkEditor(editorContainer, {
    initialContent: DEFAULT_INK,
    compile,
    getSemanticTokens: (source: string) => {
      session.updateSource(source);
      return session.getSemanticTokens();
    },
    getTokenTypeNames,
    session,
    getCompletions: (_source: string, offset: number) => session.getCompletions(offset),
    getHover: (_source: string, offset: number) => session.getHover(offset),
    gotoDefinition: (_source: string, offset: number) => session.gotoDefinition(offset),
    findReferences: (_source: string, offset: number) => session.findReferences(offset),
    prepareRename: (_source: string, offset: number) => session.prepareRename(offset),
    doRename: (_source: string, offset: number, newName: string) => session.doRename(offset, newName),
    getCodeActions: (_source: string, offset: number) => session.getCodeActions(offset),
    getInlayHints: (_source: string, start: number, end: number) => session.getInlayHints(start, end),
    getSignatureHelp: (_source: string, offset: number) => session.getSignatureHelp(offset),
    getFoldingRanges: () => session.getFoldingRanges(),
    onCompile(result) {
      if (result.ok && result.story_bytes) {
        player.loadStory(new Uint8Array(result.story_bytes));
      }
    },
  });

  document.getElementById("btn-run")?.addEventListener("click", () => {
    editor.triggerCompile();
  });

  document.getElementById("btn-restart")?.addEventListener("click", () => {
    player.reset();
  });
}

main();
