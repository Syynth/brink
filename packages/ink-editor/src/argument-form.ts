/**
 * The argument Form (argument-widget spec §1.1–1.2). A studio-composed form with
 * one control per call argument — chosen by the argument's type: a plain text
 * input, the built-in `color` picker, a host-declared **value-list** dropdown
 * (#174), or a **host widget** (single-slot or an arg-group like the map-point
 * picker) whose editor embeds inline. Opened in the studio popover chrome; on
 * Apply it hands the caller one literal per parameter (in index order), which the
 * caller writes (replace a call's args, or insert a completed call). The Form
 * never formats the call wrapper itself.
 *
 * The Form holds a **live draft** literal per parameter. Drafts are read both on
 * Apply and when opening a host editor, so inter-arg context resolves from the
 * *current* form state (pick a map in the dropdown, then the point picker renders
 * that map) before anything is written to the document.
 *
 * Reachable from the in-editor call glyph / hover-card / keybind AND the Host
 * Functions panel — all call `openArgumentForm`, differing only in what they do
 * with the result.
 */

import { openPopover, type PopoverHandle } from "./widget-popover.js";
import { mountColorPicker, hexToRgb, rgbToHex } from "./color-picker-ui.js";
import type {
  ValueItem,
  ArgumentWidget,
  ArgumentWidgetContext,
  ArgumentWidgetEditorHost,
} from "@brink/wasm-types";

/** One non-grouped parameter. The control is chosen by which of `widgetKind` /
 *  `values` / `hostWidget` is set, else a text input. */
export interface FormField {
  paramName: string;
  /** Position in the call's argument list. Defaults to the field's array index
   *  (callers with arg-groups must set this explicitly). */
  paramIndex?: number;
  /** Semantic-type name, shown in the label (e.g. `hex_color`). */
  typeName?: string;
  /** Built-in widget kind (`color`, …) → an embedded picker. */
  widgetKind?: string;
  /** Value-list options (#174) → a dropdown of `label`s inserting `value`s. */
  values?: ValueItem[];
  /** A host single-slot widget → its editor embeds inline. */
  hostWidget?: ArgumentWidget;
  /** Current value — the RAW literal text (quotes included for strings). */
  initial?: string;
}

/** An arg-group widget spanning several parameters (spec §2) — one combined
 *  control whose host editor embeds inline. */
export interface FormGroup {
  paramIndices: number[];
  paramNames: string[];
  /** The widget's semantic type / id (matches `hostWidget.type`). */
  typeName: string;
  hostWidget: ArgumentWidget;
  surface?: "popover" | "modal";
  /** Per-member current value — RAW literal text, one per member. */
  initialValues: string[];
  /** key → the sibling param index supplying it — resolved from live drafts. */
  contextParams?: Record<string, number>;
}

export interface ArgumentFormOptions {
  /** Heading, e.g. the call signature. */
  title?: string;
  /** The EXTERNAL/callee name, passed to host widgets as `ctx.external`. */
  external?: string;
  /** Non-grouped parameters. */
  fields: FormField[];
  /** Host arg-group widgets (spec §2). */
  groups?: FormGroup[];
  /** The Apply button label (e.g. "Insert" from the panel, "Apply" in-editor). */
  applyLabel?: string;
  /** One literal per parameter, in index order — ready to join with `, `. */
  onApply: (literals: string[]) => void;
  onCancel: () => void;
}

function stripQuotes(s: string): string {
  return s.length >= 2 && s.startsWith('"') && s.endsWith('"') ? s.slice(1, -1) : s;
}

function toDisplayHex(value: string): string {
  const rgb = hexToRgb(stripQuotes(value));
  return rgb ? rgbToHex(rgb) : "#000000";
}

/** Open the form anchored to `anchor`. Returns a teardown that closes it. */
export function openArgumentForm(anchor: HTMLElement, opts: ArgumentFormOptions): () => void {
  let popover: PopoverHandle | null = null;
  let applied = false;

  const groups = opts.groups ?? [];
  const fieldIndex = (f: FormField, i: number): number => f.paramIndex ?? i;

  // Total parameter count across fields + group members.
  let paramCount = 0;
  opts.fields.forEach((f, i) => (paramCount = Math.max(paramCount, fieldIndex(f, i) + 1)));
  for (const g of groups) for (const pi of g.paramIndices) paramCount = Math.max(paramCount, pi + 1);

  // Live draft literal per parameter — read by Apply and by host-editor context.
  const drafts: string[] = new Array<string>(paramCount).fill("");
  // Teardowns for any embedded host editors, run on form close.
  const teardowns = new Set<() => void>();

  const render = (container: HTMLElement): void => {
    const root = document.createElement("div");
    root.className = "brink-arg-form";

    if (opts.title) {
      const h = document.createElement("div");
      h.className = "brink-arg-form-title";
      h.textContent = opts.title;
      root.appendChild(h);
    }

    // Render units (fields + groups) in parameter order.
    const units: { index: number; build: (row: HTMLElement) => void }[] = [];
    opts.fields.forEach((f, i) => {
      const pi = fieldIndex(f, i);
      units.push({ index: pi, build: (row) => buildField(row, f, pi, drafts, teardowns, opts) });
    });
    for (const g of groups) {
      units.push({
        index: Math.min(...g.paramIndices),
        build: (row) => buildGroupField(row, g, drafts, teardowns, opts),
      });
    }
    units.sort((a, b) => a.index - b.index);

    for (const unit of units) {
      const row = document.createElement("div");
      row.className = "brink-arg-form-row";
      unit.build(row);
      root.appendChild(row);
    }

    const buttons = document.createElement("div");
    buttons.className = "brink-arg-form-buttons";
    const cancel = document.createElement("button");
    cancel.type = "button";
    cancel.className = "brink-arg-form-btn";
    cancel.textContent = "Cancel";
    cancel.addEventListener("click", () => popover?.close());
    const apply = document.createElement("button");
    apply.type = "button";
    apply.className = "brink-arg-form-btn brink-arg-form-btn-primary";
    apply.textContent = opts.applyLabel ?? "Apply";
    apply.addEventListener("click", () => {
      applied = true;
      opts.onApply(drafts.slice());
      popover?.close();
    });
    buttons.append(cancel, apply);
    root.appendChild(buttons);

    container.appendChild(root);
  };

  popover = openPopover(anchor, render, () => {
    for (const t of teardowns) t();
    if (!applied) opts.onCancel();
  });

  return () => popover?.close();
}

/** A label cell — `param: type` (or just `param`). */
function labelCell(field: { paramName: string; typeName?: string }): HTMLElement {
  const label = document.createElement("span");
  label.className = "brink-arg-form-label";
  label.textContent = field.typeName ? `${field.paramName}: ${field.typeName}` : field.paramName;
  return label;
}

/** Dispatch a non-grouped parameter to its control. */
function buildField(
  row: HTMLElement,
  field: FormField,
  pi: number,
  drafts: string[],
  teardowns: Set<() => void>,
  opts: ArgumentFormOptions,
): void {
  row.appendChild(labelCell(field));
  if (field.widgetKind === "color") {
    buildColorField(row, field, pi, drafts);
  } else if (field.values && field.values.length > 0) {
    buildEnumField(row, field, pi, drafts);
  } else if (field.hostWidget) {
    buildHostField(row, field, pi, drafts, teardowns, opts);
  } else {
    buildTextField(row, field, pi, drafts);
  }
}

/** A plain text field — the author types the raw arg expression. Empty falls
 *  back to the param name (a placeholder, like the old skeleton insert). */
function buildTextField(row: HTMLElement, field: FormField, pi: number, drafts: string[]): void {
  const input = document.createElement("input");
  input.type = "text";
  input.className = "brink-arg-form-input";
  input.spellcheck = false;
  input.value = field.initial ?? "";
  input.placeholder = field.typeName ?? "value";
  drafts[pi] = input.value.trim() || field.paramName;
  input.addEventListener("input", () => {
    drafts[pi] = input.value.trim() || field.paramName;
  });
  row.appendChild(input);
}

/** A color field: the picker widget embedded inline (its own hex input +
 *  presets are part of it). The draft is a `"#RRGGBB"` literal. */
function buildColorField(row: HTMLElement, field: FormField, pi: number, drafts: string[]): void {
  const hex = field.initial ? toDisplayHex(field.initial) : "#FF8800";
  drafts[pi] = `"${hex}"`;
  const wrap = document.createElement("div");
  wrap.className = "brink-arg-form-picker";
  mountColorPicker(wrap, hex, (h) => {
    drafts[pi] = `"${h}"`;
  });
  row.appendChild(wrap);
}

/** A value-list dropdown (#174): display labels, insert literals verbatim. An
 *  existing value not in the list is preserved as a selectable option. */
function buildEnumField(row: HTMLElement, field: FormField, pi: number, drafts: string[]): void {
  const items = field.values ?? [];
  const select = document.createElement("select");
  select.className = "brink-arg-form-select";
  const initial = field.initial?.trim();
  const known = initial !== undefined && items.some((it) => it.value === initial);
  if (initial !== undefined && initial.length > 0 && !known) {
    const opt = document.createElement("option");
    opt.value = initial;
    opt.textContent = `${initial} (current)`;
    select.appendChild(opt);
  }
  for (const it of items) {
    const opt = document.createElement("option");
    opt.value = it.value;
    opt.textContent = it.detail ? `${it.label} — ${it.detail}` : it.label;
    select.appendChild(opt);
  }
  select.value = initial && (known || initial.length > 0) ? initial : (items[0]?.value ?? "");
  drafts[pi] = select.value;
  select.addEventListener("change", () => {
    drafts[pi] = select.value;
  });
  row.appendChild(select);
}

/** A host single-slot widget: a summary chip + Edit that embeds the host editor
 *  inline in the row. `resolve` updates the draft + summary; `cancel` collapses. */
function buildHostField(
  row: HTMLElement,
  field: FormField,
  pi: number,
  drafts: string[],
  teardowns: Set<() => void>,
  opts: ArgumentFormOptions,
): void {
  const widget = field.hostWidget!;
  drafts[pi] = field.initial ?? "";
  const ctx = (): ArgumentWidgetContext => ({
    type: widget.type,
    external: opts.external ?? "",
    paramNames: [field.paramName],
    values: [stripQuotes(drafts[pi])],
  });
  mountHostEditor(row, widget, ctx, teardowns, (vals) => {
    if (vals.length > 0) drafts[pi] = vals[0];
  });
}

/** An arg-group widget: one control over several params. Context is resolved
 *  from live sibling drafts via `contextParams` at the moment the editor opens. */
function buildGroupField(
  row: HTMLElement,
  group: FormGroup,
  drafts: string[],
  teardowns: Set<() => void>,
  opts: ArgumentFormOptions,
): void {
  group.paramIndices.forEach((pi, k) => (drafts[pi] = group.initialValues[k] ?? ""));
  row.appendChild(labelCell({ paramName: group.paramNames.join(", "), typeName: group.typeName }));
  const ctx = (): ArgumentWidgetContext => {
    const context: Record<string, string> = {};
    for (const [key, idx] of Object.entries(group.contextParams ?? {})) {
      if (drafts[idx] !== undefined) context[key] = stripQuotes(drafts[idx]);
    }
    return {
      type: group.hostWidget.type,
      external: opts.external ?? "",
      paramNames: group.paramNames,
      values: group.paramIndices.map((pi) => stripQuotes(drafts[pi])),
      context,
    };
  };
  mountHostEditor(row, group.hostWidget, ctx, teardowns, (vals) => {
    group.paramIndices.forEach((pi, k) => {
      if (vals[k] !== undefined) drafts[pi] = vals[k];
    });
  });
}

/** Shared chrome for an embedded host editor (single-slot or group): a summary
 *  chip + Edit toggling an inline editor container. `onResolve` writes drafts;
 *  the summary refreshes from the widget's `inline` label. */
function mountHostEditor(
  row: HTMLElement,
  widget: ArgumentWidget,
  ctx: () => ArgumentWidgetContext,
  teardowns: Set<() => void>,
  onResolve: (values: string[]) => void,
): void {
  const summary = document.createElement("span");
  summary.className = "brink-arg-form-host";
  const refresh = (): void => {
    const label = widget.inline?.(ctx());
    summary.textContent = label?.text ?? ctx().values.join(", ");
    summary.className = "brink-arg-form-host";
    if (label?.className) summary.classList.add(label.className);
  };
  refresh();

  const editor = document.createElement("div");
  editor.className = "brink-arg-form-editor";
  editor.hidden = true;
  let teardown: (() => void) | undefined;
  const close = (): void => {
    teardown?.();
    if (teardown) teardowns.delete(teardown);
    teardown = undefined;
    editor.replaceChildren();
    editor.hidden = true;
    refresh();
  };
  const edit = document.createElement("button");
  edit.type = "button";
  edit.className = "brink-arg-form-edit";
  edit.textContent = "Edit…";
  edit.addEventListener("click", () => {
    if (!editor.hidden) {
      close();
      return;
    }
    editor.hidden = false;
    const host: ArgumentWidgetEditorHost = {
      resolve: (values) => {
        onResolve(values);
        close();
      },
      cancel: () => close(),
    };
    teardown = widget.editor.render(ctx(), host, editor);
    if (teardown) teardowns.add(teardown);
  });

  row.append(summary, edit, editor);
}
