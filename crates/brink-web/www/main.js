import init, { compile, StoryRunner, semantic_tokens, token_type_names } from './pkg/brink_web.js';
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

  const player = createPlayer(document.getElementById('player'));

  const editor = createEditor(document.getElementById('editor'), {
    initialDoc: DEFAULT_INK,
    onCompile(source) {
      const json = compile(source);
      const result = JSON.parse(json);

      if (result.ok && result.story_bytes) {
        const bytes = new Uint8Array(result.story_bytes);
        player.loadStory(bytes);
      }

      return result;
    },
    semanticTokens(source) {
      return JSON.parse(semantic_tokens(source));
    },
    tokenTypeNames() {
      return JSON.parse(token_type_names());
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
