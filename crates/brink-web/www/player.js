import { StoryRunner } from './pkg/brink_web.js';

export function createPlayer(container) {
  let runner = null;

  const storyDiv = document.createElement('div');
  storyDiv.className = 'story-text';
  container.appendChild(storyDiv);

  const choicesDiv = document.createElement('div');
  choicesDiv.className = 'choices';
  container.appendChild(choicesDiv);

  function clear() {
    storyDiv.innerHTML = '';
    choicesDiv.innerHTML = '';
  }

  function advance() {
    if (!runner) return;

    choicesDiv.innerHTML = '';

    let result;
    try {
      const json = runner.continue_story();
      result = JSON.parse(json);
    } catch (e) {
      const err = document.createElement('div');
      err.className = 'error';
      err.textContent = 'Runtime error: ' + e.message;
      storyDiv.appendChild(err);
      return;
    }

    // Render text lines
    if (result.text) {
      const lines = result.text.split('\n');
      for (const line of lines) {
        if (line.trim() === '') continue;
        const p = document.createElement('p');
        p.textContent = line;
        storyDiv.appendChild(p);
      }
    }

    if (result.status === 'continue') {
      // More text available, keep reading
      advance();
      return;
    }

    if (result.status === 'choices' && result.choices.length > 0) {
      for (const choice of result.choices) {
        const btn = document.createElement('button');
        btn.textContent = choice.text;
        btn.addEventListener('click', () => {
          // Show chosen text
          const p = document.createElement('p');
          p.textContent = '> ' + choice.text;
          p.style.color = 'var(--accent)';
          storyDiv.appendChild(p);

          try {
            runner.choose(choice.index);
          } catch (e) {
            const err = document.createElement('div');
            err.className = 'error';
            err.textContent = 'Choose error: ' + e.message;
            storyDiv.appendChild(err);
            return;
          }
          advance();
        });
        choicesDiv.appendChild(btn);
      }
    } else if (result.status === 'ended') {
      const end = document.createElement('div');
      end.className = 'end-marker';
      end.textContent = '— End —';
      storyDiv.appendChild(end);
    }

    // Auto-scroll to bottom
    container.scrollTop = container.scrollHeight;
  }

  return {
    loadStory(bytes) {
      clear();
      try {
        runner = new StoryRunner(bytes);
      } catch (e) {
        const err = document.createElement('div');
        err.className = 'error';
        err.textContent = 'Load error: ' + e.message;
        storyDiv.appendChild(err);
        return;
      }
      advance();
    },
    reset() {
      if (!runner) return;
      clear();
      runner.reset();
      advance();
    },
  };
}
