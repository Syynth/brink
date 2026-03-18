import { type Extension } from "@codemirror/state";
import { EditorView, showPanel, type Panel } from "@codemirror/view";
import { forEachDiagnostic } from "@codemirror/lint";
import { elementTypeField, ElementType } from "./element-type.js";

const ELEMENT_LABELS: Record<ElementType, string> = {
  [ElementType.KnotHeader]: "Knot",
  [ElementType.StitchHeader]: "Stitch",
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
  [ElementType.Blank]: "",
};

function renderStatus(view: EditorView, els: { element: HTMLSpanElement; cursor: HTMLSpanElement; diag: HTMLSpanElement }) {
  const { state } = view;
  const pos = state.selection.main.head;
  const line = state.doc.lineAt(pos);
  const col = pos - line.from;

  // Element type
  const infos = state.field(elementTypeField);
  const info = infos[line.number - 1];
  if (info && info.type !== ElementType.Blank) {
    let label = ELEMENT_LABELS[info.type];
    if ((info.type === ElementType.Choice || info.type === ElementType.Gather) && info.depth > 0) {
      label += ` \u00b7 depth ${info.depth}`;
    }
    if (info.type === ElementType.Choice && info.sticky) {
      label += " (sticky)";
    }
    els.element.textContent = label;
  } else {
    els.element.textContent = "";
  }

  // Cursor position
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

function createStatusPanel(view: EditorView): Panel {
  const dom = document.createElement("div");
  dom.className = "brink-statusbar";

  const elementSpan = document.createElement("span");
  elementSpan.className = "brink-status-element";

  const cursorSpan = document.createElement("span");
  cursorSpan.className = "brink-status-cursor";

  const diagSpan = document.createElement("span");
  diagSpan.className = "brink-status-diag";

  const left = document.createElement("div");
  left.className = "brink-status-left";
  left.appendChild(elementSpan);

  const right = document.createElement("div");
  right.className = "brink-status-right";
  right.appendChild(diagSpan);
  right.appendChild(cursorSpan);

  dom.appendChild(left);
  dom.appendChild(right);

  const els = { element: elementSpan, cursor: cursorSpan, diag: diagSpan };
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
