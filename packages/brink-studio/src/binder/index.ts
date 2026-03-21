/**
 * Binder — project navigator tree view.
 *
 * Tree structure: files → knots → stitches.
 * Variables, lists, and externals are filtered out.
 * Chevron on the left, icon + label to the right.
 * Indent guides as vertical lines.
 */

import type { EditorStateManager } from "../editor/state-manager.js";
import type { DocumentSymbol } from "../wasm.js";

// ── Public types ───────────────────────────────────────────────────

export interface BinderOptions {
  manager: EditorStateManager;
  onFileCreated?: (path: string) => void;
}

export interface BinderHandle {
  refresh(): void;
  destroy(): void;
  readonly element: HTMLElement;
}

// ── Icons ─────────────────────────────────────────────────────────

const ICON_FILE = "📄";
const ICON_KNOT = "◆";
const ICON_STITCH = "◇";

function iconClass(kind: string): string {
  switch (kind) {
    case "file": return "brink-binder-icon-file";
    case "knot": return "brink-binder-icon-knot";
    case "stitch": return "brink-binder-icon-stitch";
    default: return "";
  }
}

function iconChar(kind: string): string {
  switch (kind) {
    case "file": return ICON_FILE;
    case "knot": return ICON_KNOT;
    case "stitch": return ICON_STITCH;
    default: return "·";
  }
}

// ── Implementation ─────────────────────────────────────────────────

export function createBinder(options: BinderOptions): BinderHandle {
  const { manager } = options;

  const root = document.createElement("div");
  root.className = "brink-binder";

  /** Tracks collapsed state by key (file path or "path::knotName"). */
  const collapsed = new Set<string>();

  let inputActive = false;
  let clickTimer: ReturnType<typeof setTimeout> | null = null;

  function notifyTabChanged(): void {
    manager.getView().dom.dispatchEvent(new CustomEvent("brink-tab-changed"));
  }

  function resolveTarget(
    target: import("../editor/state-manager.js").TabTarget,
  ): import("../editor/state-manager.js").TabTarget {
    if (target.kind !== "symbol") return target;
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

  // ── Row builder ───────────────────────────────────────────────

  function buildRow(opts: {
    depth: number;
    kind: string;
    label: string;
    expandable: boolean;
    expandKey: string;
    isActive: boolean;
  }): { row: HTMLElement; chevron: HTMLElement } {
    const row = document.createElement("div");
    row.className = "brink-binder-row"
      + (opts.kind === "file" ? " brink-binder-file-row" : "")
      + (opts.kind === "knot" ? " brink-binder-knot" : "")
      + (opts.kind === "stitch" ? " brink-binder-stitch" : "");

    if (opts.isActive) {
      row.classList.add("brink-binder-active");
    }

    // Indent guides
    const guides = document.createElement("div");
    guides.className = "brink-binder-guides";
    for (let i = 0; i < opts.depth; i++) {
      const guide = document.createElement("div");
      guide.className = "brink-binder-guide";
      guides.appendChild(guide);
    }
    row.appendChild(guides);

    // Chevron
    const chevron = document.createElement("div");
    chevron.className = "brink-binder-chevron";
    if (opts.expandable) {
      chevron.textContent = "▶";
      if (!collapsed.has(opts.expandKey)) {
        // expanded state — rotate to point down
      } else {
        chevron.classList.add("collapsed");
      }
      chevron.addEventListener("click", (e) => {
        e.stopPropagation();
        if (collapsed.has(opts.expandKey)) {
          collapsed.delete(opts.expandKey);
        } else {
          collapsed.add(opts.expandKey);
        }
        render();
      });
    } else {
      chevron.classList.add("leaf");
    }
    row.appendChild(chevron);

    // Icon
    const icon = document.createElement("span");
    icon.className = "brink-binder-icon " + iconClass(opts.kind);
    icon.textContent = iconChar(opts.kind);
    row.appendChild(icon);

    // Label
    const label = document.createElement("span");
    label.className = "brink-binder-label";
    label.textContent = opts.label;
    row.appendChild(label);

    return { row, chevron };
  }

  // ── Render ────────────────────────────────────────────────────

  function render(): void {
    root.innerHTML = "";

    const session = manager.getProject().getSession();
    const outline = session.getProjectOutline();
    const activeTab = manager.getActiveTab();

    for (const file of outline) {
      // Filter to only structural symbols (knots)
      const knots = file.symbols.filter((s) => s.kind === "knot");
      const hasChildren = knots.length > 0;
      const fileKey = file.path;

      const isFileActive = activeTab.target.kind === "file" && activeTab.target.path === file.path;

      const { row: fileRow } = buildRow({
        depth: 0,
        kind: "file",
        label: displayName(file.path),
        expandable: hasChildren,
        expandKey: fileKey,
        isActive: isFileActive,
      });

      attachClickHandlers(fileRow, { kind: "file", path: file.path });
      root.appendChild(fileRow);

      // Children (if expanded)
      if (!collapsed.has(fileKey)) {
        for (const knot of knots) {
          renderKnot(file.path, knot, activeTab);
        }
      }
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

  function renderKnot(
    path: string,
    knot: DocumentSymbol,
    activeTab: ReturnType<typeof manager.getActiveTab>,
  ): void {
    const stitches = knot.children.filter((c) => c.kind === "stitch");
    const hasStitches = stitches.length > 0;
    const knotKey = `${path}::${knot.name}`;

    const isKnotActive = activeTab.id === knotKey;

    const { row: knotRow } = buildRow({
      depth: 1,
      kind: "knot",
      label: knot.name,
      expandable: hasStitches,
      expandKey: knotKey,
      isActive: isKnotActive,
    });

    attachClickHandlers(knotRow, {
      kind: "symbol", path, name: knot.name,
      start: knot.full_start, end: knot.full_end,
    });
    root.appendChild(knotRow);

    // Stitches (if expanded)
    if (hasStitches && !collapsed.has(knotKey)) {
      for (const stitch of stitches) {
        renderStitch(path, stitch, activeTab);
      }
    }
  }

  function renderStitch(
    path: string,
    stitch: DocumentSymbol,
    activeTab: ReturnType<typeof manager.getActiveTab>,
  ): void {
    const stitchId = `${path}::${stitch.name}`;
    const isActive = activeTab.id === stitchId;

    const { row } = buildRow({
      depth: 2,
      kind: "stitch",
      label: stitch.name,
      expandable: false,
      expandKey: "",
      isActive: isActive,
    });

    attachClickHandlers(row, {
      kind: "symbol", path, name: stitch.name,
      start: stitch.full_start, end: stitch.full_end,
    });
    root.appendChild(row);
  }

  // ── New file input ────────────────────────────────────────────

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
