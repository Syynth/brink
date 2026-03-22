import type { StoryRunnerHandle, Line } from "../wasm.js";

export interface BrinkPlayerHandle {
  loadStory(bytes: Uint8Array): void;
  reset(): void;
  destroy(): void;
}

export function createBrinkPlayer(
  container: HTMLElement,
  createRunner: (bytes: Uint8Array) => StoryRunnerHandle,
): BrinkPlayerHandle {
  let runner: StoryRunnerHandle | null = null;

  const storyDiv = document.createElement("div");
  storyDiv.className = "story-text";
  container.appendChild(storyDiv);

  const choicesDiv = document.createElement("div");
  choicesDiv.className = "choices";
  container.appendChild(choicesDiv);

  function clear(): void {
    storyDiv.innerHTML = "";
    choicesDiv.innerHTML = "";
  }

  function advance(): void {
    if (!runner) return;

    choicesDiv.innerHTML = "";

    let lines: Line[];
    try {
      lines = runner.continueStory();
    } catch (e) {
      const err = document.createElement("div");
      err.className = "error";
      err.textContent = "Runtime error: " + (e instanceof Error ? e.message : String(e));
      storyDiv.appendChild(err);
      return;
    }

    for (const line of lines) {
      const text = line.text.replace(/\n$/, "");
      if (text) {
        const textLines = text.split("\n");
        for (const tl of textLines) {
          if (tl.trim() === "") continue;
          const p = document.createElement("p");
          p.textContent = tl;
          storyDiv.appendChild(p);
        }
      }

      if (line.type === "choices" && line.choices && line.choices.length > 0) {
        for (const choice of line.choices) {
          const btn = document.createElement("button");
          btn.textContent = choice.text;
          btn.addEventListener("click", () => {
            const p = document.createElement("p");
            p.textContent = "> " + choice.text;
            p.style.color = "var(--brink-accent)";
            storyDiv.appendChild(p);

            try {
              runner!.choose(choice.index);
            } catch (e) {
              const err = document.createElement("div");
              err.className = "error";
              err.textContent = "Choose error: " + (e instanceof Error ? e.message : String(e));
              storyDiv.appendChild(err);
              return;
            }
            advance();
          });
          choicesDiv.appendChild(btn);
        }
      } else if (line.type === "end") {
        const end = document.createElement("div");
        end.className = "end-marker";
        end.textContent = "\u2014 End \u2014";
        storyDiv.appendChild(end);
      }
    }

    container.scrollTop = container.scrollHeight;
  }

  return {
    loadStory(bytes: Uint8Array) {
      if (runner) {
        runner.free();
        runner = null;
      }
      clear();
      try {
        runner = createRunner(bytes);
      } catch (e) {
        const err = document.createElement("div");
        err.className = "error";
        err.textContent = "Load error: " + (e instanceof Error ? e.message : String(e));
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

    destroy() {
      if (runner) {
        runner.free();
        runner = null;
      }
      clear();
    },
  };
}
