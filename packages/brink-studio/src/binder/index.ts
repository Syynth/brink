import type { EditorSessionHandle, DocumentSymbol } from "../wasm.js";

// ── Public types ───────────────────────────────────────────────────

export interface BinderOptions {
  session: EditorSessionHandle;
  onNavigate: (path: string, offset?: number) => void;
}

export interface BinderHandle {
  /** Re-query the project outline and re-render the tree. */
  refresh(): void;
  /** Highlight the active item by path and optional byte offset. */
  setActive(path: string, offset?: number): void;
  /** Tear down the binder and remove its DOM element. */
  destroy(): void;
  /** The root DOM element of the binder panel. */
  readonly element: HTMLElement;
}

// ── Implementation ─────────────────────────────────────────────────

export function createBinder(options: BinderOptions): BinderHandle {
  const { session, onNavigate } = options;

  const root = document.createElement("div");
  root.className = "brink-binder";

  let activePath: string | undefined;
  let activeOffset: number | undefined;

  function render(): void {
    root.innerHTML = "";
    const outline = session.getProjectOutline();

    for (const file of outline) {
      const fileNode = document.createElement("div");
      fileNode.className = "brink-binder-file";

      const fileLabel = document.createElement("div");
      fileLabel.className = "brink-binder-label brink-binder-file-label";
      fileLabel.textContent = file.path;
      fileLabel.addEventListener("click", () => {
        onNavigate(file.path);
      });
      fileNode.appendChild(fileLabel);

      for (const symbol of file.symbols) {
        renderSymbol(fileNode, file.path, symbol, 1);
      }

      root.appendChild(fileNode);
    }

    applyActive();
  }

  function renderSymbol(
    parent: HTMLElement,
    path: string,
    symbol: DocumentSymbol,
    depth: number,
  ): void {
    const kind = symbol.kind === "knot" ? "knot" : "stitch";
    const item = document.createElement("div");
    item.className = `brink-binder-${kind}`;
    item.style.paddingLeft = `${depth * 12}px`;
    item.dataset.path = path;
    item.dataset.start = String(symbol.start);

    const label = document.createElement("div");
    label.className = "brink-binder-label";
    label.textContent = symbol.name;
    label.addEventListener("click", () => {
      onNavigate(path, symbol.start);
    });
    item.appendChild(label);

    parent.appendChild(item);

    for (const child of symbol.children) {
      renderSymbol(parent, path, child, depth + 1);
    }
  }

  function applyActive(): void {
    const items = root.querySelectorAll("[data-path]");
    for (const item of items) {
      item.classList.remove("brink-binder-active");
    }

    if (activePath == null) return;

    let best: Element | null = null;
    let bestStart = -1;

    for (const item of items) {
      const el = item as HTMLElement;
      if (el.dataset.path !== activePath) continue;
      const start = Number(el.dataset.start);

      if (activeOffset != null) {
        if (start <= activeOffset && start > bestStart) {
          best = el;
          bestStart = start;
        }
      } else if (best == null) {
        best = el;
      }
    }

    if (best) {
      best.classList.add("brink-binder-active");
    }
  }

  // Initial render
  render();

  return {
    refresh(): void {
      render();
    },
    setActive(path: string, offset?: number): void {
      activePath = path;
      activeOffset = offset;
      applyActive();
    },
    destroy(): void {
      root.remove();
    },
    get element(): HTMLElement {
      return root;
    },
  };
}
