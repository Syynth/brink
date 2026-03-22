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

    let lines;
    try {
      const json = runner.continue_story();
      lines = JSON.parse(json);
    } catch (e) {
      const err = document.createElement('div');
      err.className = 'error';
      err.textContent = 'Runtime error: ' + e.message;
      storyDiv.appendChild(err);
      return;
    }

    for (const line of lines) {
      const text = line.text.replace(/\n$/, '');
      if (text) {
        const textLines = text.split('\n');
        for (const tl of textLines) {
          if (tl.trim() === '') continue;
          const p = document.createElement('p');
          p.textContent = tl;
          storyDiv.appendChild(p);
        }
      }

      if (line.type === 'choices' && line.choices && line.choices.length > 0) {
        for (const choice of line.choices) {
          const btn = document.createElement('button');
          btn.textContent = choice.text;
          btn.addEventListener('click', () => {
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
      } else if (line.type === 'end') {
        const end = document.createElement('div');
        end.className = 'end-marker';
        end.textContent = '— End —';
        storyDiv.appendChild(end);
      }
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
