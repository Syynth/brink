/**
 * FileTabBar — horizontal tab strip for switching between open files.
 *
 * Renders inside the editor chrome, above the CodeMirror view.
 * Delegates file switching and creation to EditorStateManager.
 */

import type { EditorStateManager } from "./state-manager.js";

export interface FileTabBarOptions {
  manager: EditorStateManager;
  /** Called after a file switch completes. */
  onSwitch?: (path: string) => void;
}

export interface FileTabBarHandle {
  /** The root DOM element — mount this into the editor pane. */
  readonly element: HTMLElement;
  /** Re-render the tab strip (e.g. after external file changes). */
  refresh(): void;
  /** Tear down. */
  destroy(): void;
}

export function createFileTabBar(options: FileTabBarOptions): FileTabBarHandle {
  const { manager } = options;

  const root = document.createElement("div");
  root.className = "brink-file-tabs";

  let inputActive = false;

  function displayName(path: string): string {
    const slash = path.lastIndexOf("/");
    return slash >= 0 ? path.substring(slash + 1) : path;
  }

  function render(): void {
    // Clear existing tabs (but keep input if active)
    const existingInput = root.querySelector(".brink-tab-input-wrapper");
    root.innerHTML = "";

    const files = manager.files();
    const active = manager.active();

    for (const file of files) {
      const tab = document.createElement("div");
      tab.className = "brink-tab" + (file === active ? " active" : "");
      tab.title = file;

      const label = document.createElement("span");
      label.className = "brink-tab-label";
      label.textContent = displayName(file);
      tab.appendChild(label);

      // Close button (don't show if only one file)
      if (files.length > 1) {
        const close = document.createElement("span");
        close.className = "brink-tab-close";
        close.textContent = "\u00d7";
        close.title = "Close";
        close.addEventListener("click", (e) => {
          e.stopPropagation();
          void manager.closeFile(file).then((closed) => {
            if (closed) {
              render();
              options.onSwitch?.(manager.active());
            }
          });
        });
        tab.appendChild(close);
      }

      tab.addEventListener("click", () => {
        if (file === active) return;
        void manager.switchTo(file).then(() => {
          render();
          options.onSwitch?.(file);
        });
      });

      root.appendChild(tab);
    }

    // "+" button
    const addBtn = document.createElement("div");
    addBtn.className = "brink-tab-new";
    addBtn.textContent = "+";
    addBtn.title = "New file";
    addBtn.addEventListener("click", () => {
      if (inputActive) return;
      showNewFileInput();
    });
    root.appendChild(addBtn);

    // Restore input if it was active
    if (existingInput && inputActive) {
      root.appendChild(existingInput);
    }
  }

  function showNewFileInput(): void {
    inputActive = true;

    const wrapper = document.createElement("div");
    wrapper.className = "brink-tab-input-wrapper";

    const input = document.createElement("input");
    input.className = "brink-tab-input";
    input.type = "text";
    input.placeholder = "filename.ink";
    input.size = 16;

    function cancel(): void {
      inputActive = false;
      wrapper.remove();
    }

    function confirm(): void {
      let name = input.value.trim();
      if (!name) {
        cancel();
        return;
      }
      // Add .ink extension if none provided
      if (!name.includes(".")) {
        name += ".ink";
      }
      // Check for duplicates
      if (manager.files().includes(name)) {
        input.classList.add("error");
        return;
      }
      inputActive = false;
      wrapper.remove();
      void manager.addFile(name).then(() =>
        manager.switchTo(name).then(() => {
          render();
          options.onSwitch?.(name);
        }),
      );
    }

    input.addEventListener("keydown", (e) => {
      if (e.key === "Enter") {
        e.preventDefault();
        confirm();
      } else if (e.key === "Escape") {
        e.preventDefault();
        cancel();
      }
    });
    input.addEventListener("blur", cancel);

    wrapper.appendChild(input);
    root.appendChild(wrapper);
    input.focus();
  }

  render();

  return {
    get element() {
      return root;
    },
    refresh: render,
    destroy() {
      root.remove();
    },
  };
}
