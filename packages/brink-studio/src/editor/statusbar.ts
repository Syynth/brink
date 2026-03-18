import { type Extension } from "@codemirror/state";
import { EditorView, showPanel, type Panel } from "@codemirror/view";
import { forEachDiagnostic } from "@codemirror/lint";
import { elementTypeField, ElementType, type LineInfo } from "./element-type.js";
import { getHintsForElement, lineHasContent, parseSigilPrefix, buildContext } from "./transitions.js";

// ── Element labels ─────────────────────────────────────────────────

const ELEMENT_LABELS: Record<ElementType, string> = {
  [ElementType.KnotHeader]: "Knot Header",
  [ElementType.StitchHeader]: "Stitch Header",
  [ElementType.NarrativeText]: "Narrative",
  [ElementType.Choice]: "Choice",
  [ElementType.Gather]: "Gather",
  [ElementType.Divert]: "Divert",
  [ElementType.Logic]: "Logic",
  [ElementType.VarDecl]: "Variable",
  [ElementType.Comment]: "Comment",
  [ElementType.Include]: "Include",
  [ElementType.External]: "External",
  [ElementType.Tag]: "Tag",
  [ElementType.Blank]: "Blank",
};

function elementLabel(info: LineInfo): string {
  let label = ELEMENT_LABELS[info.type];
  if ((info.type === ElementType.Choice || info.type === ElementType.Gather) && info.depth > 1) {
    label += ` \u00b7 ${info.depth}`;
  }
  if (info.type === ElementType.Choice && info.sticky) {
    label += " (+)";
  }
  return label;
}

// ── Dropdown conversion ────────────────────────────────────────────

const CONVERTIBLE_TYPES: { label: string; sigil: string }[] = [
  { label: "Narrative", sigil: "" },
  { label: "Choice (*)", sigil: "* " },
  { label: "Choice (+)", sigil: "+ " },
  { label: "Gather", sigil: "- " },
  { label: "Divert", sigil: "-> " },
  { label: "Logic", sigil: "~ " },
  { label: "Comment", sigil: "// " },
  { label: "Tag", sigil: "# " },
  { label: "Knot Header", sigil: "=== " },
  { label: "Stitch Header", sigil: "= " },
];

function getLineSigilRange(text: string): { start: number; end: number } {
  const trimmed = text.trimStart();
  const ws = text.length - trimmed.length;

  const patterns = [
    /^={3,}\s*/,
    /^={2}\s+\w[^=]*={2,}\s*/,
    /^=\s+/,
    /^([*+]\s*)+/,
    /^(-\s*)+(?!>)/,
    /^->\s*/,
    /^~\s*/,
    /^\/\/\s*/,
    /^\/\*\s*/,
    /^#\s*/,
    /^(VAR|CONST|LIST)\s+/,
    /^INCLUDE\s+/,
    /^EXTERNAL\s+/,
  ];

  for (const p of patterns) {
    const m = trimmed.match(p);
    if (m) return { start: ws, end: ws + m[0].length };
  }

  return { start: ws, end: ws };
}

function convertLineToType(view: EditorView, sigil: string) {
  const pos = view.state.selection.main.head;
  const line = view.state.doc.lineAt(pos);
  const { start, end } = getLineSigilRange(line.text);

  view.dispatch({
    changes: { from: line.from + start, to: line.from + end, insert: sigil },
    selection: { anchor: line.from + start + sigil.length },
  });
  view.focus();
}

// ── Rendering ──────────────────────────────────────────────────────

interface StatusEls {
  keyhint: HTMLSpanElement;
  element: HTMLButtonElement;
  cursor: HTMLSpanElement;
  diag: HTMLSpanElement;
}

function renderStatus(view: EditorView, els: StatusEls) {
  const { state } = view;
  const pos = state.selection.main.head;
  const line = state.doc.lineAt(pos);
  const col = pos - line.from;
  const infos = state.field(elementTypeField);
  const info = infos[line.number - 1];

  // Element label
  els.element.textContent = info ? elementLabel(info) : "Blank";

  // Keybind hints from transition table
  if (info) {
    const hasContent = lineHasContent(line.text, info);
    const lineCtx = buildContext(infos, line.number - 1);
    const hints = getHintsForElement(info, hasContent, lineCtx);
    els.keyhint.textContent = hints.map((h) => `${h.key}: ${h.hint}`).join("  \u00b7  ");
  } else {
    els.keyhint.textContent = "";
  }

  // Cursor
  els.cursor.textContent = `${line.number}:${col + 1}`;

  // Diagnostics
  let errors = 0;
  let warnings = 0;
  forEachDiagnostic(state, (d) => {
    if (d.severity === "error") errors++;
    else if (d.severity === "warning") warnings++;
  });
  if (errors > 0 || warnings > 0) {
    const parts: string[] = [];
    if (errors > 0) parts.push(`${errors} error${errors > 1 ? "s" : ""}`);
    if (warnings > 0) parts.push(`${warnings} warning${warnings > 1 ? "s" : ""}`);
    els.diag.textContent = parts.join(", ");
    els.diag.className = "brink-status-diag" + (errors > 0 ? " has-errors" : " has-warnings");
  } else {
    els.diag.textContent = "";
    els.diag.className = "brink-status-diag";
  }
}

// ── Panel ──────────────────────────────────────────────────────────

function createStatusPanel(view: EditorView): Panel {
  const dom = document.createElement("div");
  dom.className = "brink-statusbar";

  const keyhintSpan = document.createElement("span");
  keyhintSpan.className = "brink-status-keyhint";

  const elementBtn = document.createElement("button");
  elementBtn.className = "brink-status-element-btn";

  const cursorSpan = document.createElement("span");
  cursorSpan.className = "brink-status-cursor";

  const diagSpan = document.createElement("span");
  diagSpan.className = "brink-status-diag";

  // Dropdown
  const dropdown = document.createElement("div");
  dropdown.className = "brink-element-dropdown";
  dropdown.style.display = "none";

  for (const item of CONVERTIBLE_TYPES) {
    const option = document.createElement("button");
    option.className = "brink-element-dropdown-item";
    option.textContent = item.label;
    option.addEventListener("mousedown", (e) => {
      e.preventDefault();
      dropdown.style.display = "none";
      convertLineToType(view, item.sigil);
    });
    dropdown.appendChild(option);
  }

  elementBtn.addEventListener("click", (e) => {
    e.stopPropagation();
    const showing = dropdown.style.display !== "none";
    dropdown.style.display = showing ? "none" : "";
    if (!showing) {
      const rect = elementBtn.getBoundingClientRect();
      dropdown.style.left = `${rect.left}px`;
      dropdown.style.bottom = `${window.innerHeight - rect.top + 4}px`;
    }
  });

  document.addEventListener("click", () => {
    dropdown.style.display = "none";
  });

  const left = document.createElement("div");
  left.className = "brink-status-left";
  left.appendChild(keyhintSpan);

  const right = document.createElement("div");
  right.className = "brink-status-right";
  right.appendChild(diagSpan);
  right.appendChild(elementBtn);
  right.appendChild(cursorSpan);

  dom.appendChild(left);
  dom.appendChild(right);
  document.body.appendChild(dropdown);

  const els = { keyhint: keyhintSpan, element: elementBtn, cursor: cursorSpan, diag: diagSpan };
  renderStatus(view, els);

  return {
    dom,
    update(viewUpdate) {
      renderStatus(viewUpdate.view, els);
    },
  };
}

export function statusBarExtension(): Extension {
  return showPanel.of((view) => createStatusPanel(view));
}
