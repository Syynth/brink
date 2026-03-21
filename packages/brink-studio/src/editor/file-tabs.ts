/**
 * FileTabBar — horizontal tab strip for switching between open tabs.
 *
 * Supports pinned and unpinned tabs. Unpinned tabs render with italic
 * labels and no close button. Double-clicking an unpinned tab pins it.
 */

import type { EditorStateManager, TabInfo } from "./state-manager.js";

export interface FileTabBarOptions {
  manager: EditorStateManager;
  /** Called after a tab switch completes. */
  onSwitch?: () => void;
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

  // Listen for auto-pin events from the editor
  manager.getView().dom.addEventListener("brink-tab-pinned", () => render());

  function render(): void {
    const existingInput = root.querySelector(".brink-tab-input-wrapper");
    root.innerHTML = "";

    const tabs = manager.getTabs();
    const activeTab = manager.getActiveTab();

    for (const tab of tabs) {
      const el = document.createElement("div");
      el.className = "brink-tab"
        + (tab.id === activeTab.id ? " active" : "")
        + (tab.pinned ? "" : " unpinned");
      el.title = tab.target.kind === "file" ? tab.target.path : `${tab.target.name} in ${tab.target.path}`;

      const label = document.createElement("span");
      label.className = "brink-tab-label";
      label.textContent = tab.label;
      el.appendChild(label);

      // Close button — available when there's more than one tab
      if (tabs.length > 1) {
        const close = document.createElement("span");
        close.className = "brink-tab-close";
        close.textContent = "\u00d7";
        close.title = "Close";
        close.addEventListener("click", (e) => {
          e.stopPropagation();
          void manager.closeTab(tab.id).then((closed) => {
            if (closed) {
              render();
              options.onSwitch?.();
            }
          });
        });
        el.appendChild(close);
      }

      // Single-click: switch to tab
      el.addEventListener("click", () => {
        if (tab.id === activeTab.id) return;
        void manager.openTab(tab.target, tab.pinned).then(() => {
          render();
          options.onSwitch?.();
        });
      });

      // Double-click: pin unpinned tab
      el.addEventListener("dblclick", (e) => {
        e.preventDefault();
        if (!tab.pinned) {
          manager.pinTab(tab.id);
          render();
        }
      });

      root.appendChild(el);
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
      if (!name.includes(".")) {
        name += ".ink";
      }
      if (manager.files().includes(name)) {
        input.classList.add("error");
        return;
      }
      inputActive = false;
      wrapper.remove();
      void manager.addFile(name).then(() => {
        render();
        options.onSwitch?.();
      });
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
