/**
 * Binder — project navigator tree view.
 *
 * Shows files with expand/collapse, knots indented under files,
 * stitches under knots. Single-click opens an unpinned tab,
 * double-click opens a pinned tab.
 */

import type { EditorStateManager } from "../editor/state-manager.js";
import type { DocumentSymbol } from "../wasm.js";

// ── Public types ───────────────────────────────────────────────────

export interface BinderOptions {
  manager: EditorStateManager;
  onFileCreated?: (path: string) => void;
}

export interface BinderHandle {
  /** Re-query the project outline and re-render the tree. */
  refresh(): void;
  /** Tear down the binder and remove its DOM element. */
  destroy(): void;
  /** The root DOM element of the binder panel. */
  readonly element: HTMLElement;
}

// ── Implementation ─────────────────────────────────────────────────

export function createBinder(options: BinderOptions): BinderHandle {
  const { manager } = options;

  const root = document.createElement("div");
  root.className = "brink-binder";

  /** Tracks which file paths are collapsed. Default: all expanded. */
  const collapsed = new Set<string>();

  let inputActive = false;
  let clickTimer: ReturnType<typeof setTimeout> | null = null;

  /** Emit tab-changed event so tab bar and binder refresh. */
  function notifyTabChanged(): void {
    manager.getView().dom.dispatchEvent(new CustomEvent("brink-tab-changed"));
  }

  /**
   * Resolve a symbol's current byte range from the live outline.
   * Flushes the editor first so the outline reflects the latest edits
   * (the compile callback is debounced at 500ms, so offsets may be stale).
   */
  function resolveTarget(
    target: import("../editor/state-manager.js").TabTarget,
  ): import("../editor/state-manager.js").TabTarget {
    if (target.kind !== "symbol") return target;
    // Flush current editor content → triggers re-analysis
    manager.snapshot();
    const session = manager.getProject().getSession();
    const view = manager.getView();
    session.updateSource(view.state.doc.toString());

    const outline = session.getProjectOutline();
    const file = outline.find((f) => f.path === target.path);
    if (!file) return target;
    for (const sym of file.symbols) {
      if (sym.name === target.name) {
        return { ...target, start: sym.full_start, end: sym.full_end };
      }
      for (const child of sym.children) {
        if (child.name === target.name) {
          return { ...target, start: child.full_start, end: child.full_end };
        }
      }
    }
    return target;
  }

  /** Handle single/double-click discrimination. */
  function attachClickHandlers(
    el: HTMLElement,
    target: import("../editor/state-manager.js").TabTarget,
  ): void {
    el.addEventListener("click", () => {
      if (clickTimer) clearTimeout(clickTimer);
      clickTimer = setTimeout(() => {
        clickTimer = null;
        void manager.openTab(resolveTarget(target), false).then(notifyTabChanged);
      }, 200);
    });
    el.addEventListener("dblclick", (e) => {
      e.preventDefault();
      if (clickTimer) { clearTimeout(clickTimer); clickTimer = null; }
      void manager.openTab(resolveTarget(target), true).then(notifyTabChanged);
    });
  }

  function displayName(path: string): string {
    const slash = path.lastIndexOf("/");
    return slash >= 0 ? path.substring(slash + 1) : path;
  }

  function render(): void {
    root.innerHTML = "";

    const session = manager.getProject().getSession();
    const outline = session.getProjectOutline();
    const activeTab = manager.getActiveTab();

    for (const file of outline) {
      const fileNode = document.createElement("div");
      fileNode.className = "brink-binder-file";

      // File row
      const fileRow = document.createElement("div");
      fileRow.className = "brink-binder-row brink-binder-file-row";

      // Expand/collapse arrow
      const hasChildren = file.symbols.length > 0;
      const arrow = document.createElement("span");
      arrow.className = "brink-binder-arrow";
      if (hasChildren) {
        arrow.textContent = collapsed.has(file.path) ? "\u25b6" : "\u25bc";
        arrow.addEventListener("click", (e) => {
          e.stopPropagation();
          if (collapsed.has(file.path)) {
            collapsed.delete(file.path);
          } else {
            collapsed.add(file.path);
          }
          render();
        });
      } else {
        arrow.textContent = " ";
      }
      fileRow.appendChild(arrow);

      const fileLabel = document.createElement("span");
      fileLabel.className = "brink-binder-label brink-binder-file-label";
      fileLabel.textContent = displayName(file.path);
      fileRow.appendChild(fileLabel);

      // Highlight active
      if (activeTab.target.kind === "file" && activeTab.target.path === file.path) {
        fileRow.classList.add("brink-binder-active");
      }

      attachClickHandlers(fileRow, { kind: "file", path: file.path });

      fileNode.appendChild(fileRow);

      // Children (if expanded)
      if (!collapsed.has(file.path)) {
        for (const symbol of file.symbols) {
          renderSymbol(fileNode, file.path, symbol, 1, activeTab);
        }
      }

      root.appendChild(fileNode);
    }

    // "+ New file" button
    const newBtn = document.createElement("div");
    newBtn.className = "brink-binder-row brink-binder-new";
    newBtn.textContent = "+ New file";
    newBtn.addEventListener("click", () => {
      if (inputActive) return;
      showNewFileInput();
    });
    root.appendChild(newBtn);
  }

  function renderSymbol(
    parent: HTMLElement,
    path: string,
    symbol: DocumentSymbol,
    depth: number,
    activeTab: ReturnType<typeof manager.getActiveTab>,
  ): void {
    const kind = symbol.kind === "knot" ? "knot" : "stitch";
    const row = document.createElement("div");
    row.className = `brink-binder-row brink-binder-${kind}`;
    row.style.paddingLeft = `${8 + depth * 14}px`;

    const label = document.createElement("span");
    label.className = "brink-binder-label";
    label.textContent = symbol.name;
    row.appendChild(label);

    // Highlight active symbol
    const symbolId = `${path}::${symbol.name}`;
    if (activeTab.id === symbolId) {
      row.classList.add("brink-binder-active");
    }

    console.log(`[binder] symbol="${symbol.name}" kind=${symbol.kind} full_start=${symbol.full_start} full_end=${symbol.full_end}`);
    attachClickHandlers(row, { kind: "symbol", path, name: symbol.name, start: symbol.full_start, end: symbol.full_end });

    parent.appendChild(row);

    for (const child of symbol.children) {
      renderSymbol(parent, path, child, depth + 1, activeTab);
    }
  }

  function showNewFileInput(): void {
    inputActive = true;

    const wrapper = document.createElement("div");
    wrapper.className = "brink-binder-input-wrapper";

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
        options.onFileCreated?.(name);
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

  // Initial render
  render();

  return {
    refresh(): void {
      render();
    },
    destroy(): void {
      root.remove();
    },
    get element(): HTMLElement {
      return root;
    },
  };
}
