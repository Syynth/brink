import init, { EditorSession, token_type_names } from './pkg/brink_web.js';
import { createEditor } from './editor.js';
import { createPlayer } from './player.js';

const DEFAULT_INK = `// Welcome to brink playground!
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

async function main() {
  await init();

  document.getElementById('loading').remove();

  // Stateful IDE session: the single source of truth for parse/analysis,
  // shared by highlighting and compilation.
  const session = new EditorSession();

  const player = createPlayer(document.getElementById('player'));

  const editor = createEditor(document.getElementById('editor'), {
    initialDoc: DEFAULT_INK,
    session,
    tokenTypeNames() {
      return JSON.parse(token_type_names());
    },
    onCompiled(result) {
      if (result.ok && result.story_bytes) {
        player.loadStory(new Uint8Array(result.story_bytes));
      }
    },
  });

  document.getElementById('btn-run').addEventListener('click', () => {
    editor.triggerCompile();
  });

  document.getElementById('btn-restart').addEventListener('click', () => {
    player.reset();
  });
}

main();
