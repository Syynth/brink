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

/** Shared mutable state threaded through the field builders. */
interface FormRuntime {
  /** Live draft literal per parameter. */
  drafts: string[];
  /** Teardowns for embedded host editors, run on form close. */
  teardowns: Set<() => void>;
  /** Open host editors' redraw callbacks, keyed internally by context indices. */
  redrawers: Set<(changedIndex: number) => void>;
  /** Announce that `drafts[changedIndex]` changed (drives context re-render). */
  notify: (changedIndex: number) => void;
  /** The callee name, passed to host widgets as `ctx.external`. */
  external?: string;
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
  // Open host editors register here so an arg-group editor re-renders when a
  // sibling draft it reads as context changes (pick a map → the open point
  // picker re-titles itself), instead of needing an Edit off/on toggle.
  const redrawers = new Set<(changedIndex: number) => void>();
  const rt: FormRuntime = {
    drafts,
    teardowns,
    redrawers,
    external: opts.external,
    notify: (changedIndex) => {
      for (const r of redrawers) r(changedIndex);
    },
  };

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
      units.push({ index: pi, build: (row) => buildField(row, f, pi, rt) });
    });
    for (const g of groups) {
      units.push({
        index: Math.min(...g.paramIndices),
        build: (row) => buildGroupField(row, g, rt),
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

/**
 * Dispatch a non-grouped parameter to its control. Precedence (#990):
 * **color → hostWidget → values → text.** A host widget outranks a plain
 * values dropdown — before this fix a semantic type with BOTH a host widget
 * AND `setHostValues` labels (a rich picker over the same domain, e.g. an
 * icon-grid item browser) could never get its widget in the Form, because
 * `values` was checked first and always won. This is an observable behavior
 * change for any consumer whose host widget + values combination silently
 * fell back to the dropdown — the widget now renders instead (changeset).
 */
function buildField(row: HTMLElement, field: FormField, pi: number, rt: FormRuntime): void {
  row.appendChild(labelCell(field));
  if (field.widgetKind === "color") {
    buildColorField(row, field, pi, rt);
  } else if (field.hostWidget) {
    buildHostField(row, field, pi, rt);
  } else if (field.values && field.values.length > 0) {
    buildEnumField(row, field, pi, rt);
  } else {
    buildTextField(row, field, pi, rt);
  }
}

/** A plain text field — the author types the raw arg expression. Empty falls
 *  back to the param name (a placeholder, like the old skeleton insert). */
function buildTextField(row: HTMLElement, field: FormField, pi: number, rt: FormRuntime): void {
  const input = document.createElement("input");
  input.type = "text";
  input.className = "brink-arg-form-input";
  input.spellcheck = false;
  input.value = field.initial ?? "";
  input.placeholder = field.typeName ?? "value";
  rt.drafts[pi] = input.value.trim() || field.paramName;
  input.addEventListener("input", () => {
    rt.drafts[pi] = input.value.trim() || field.paramName;
    rt.notify(pi);
  });
  row.appendChild(input);
}

/** A color field: the picker widget embedded inline (its own hex input +
 *  presets are part of it). The draft is a `"#RRGGBB"` literal. */
function buildColorField(row: HTMLElement, field: FormField, pi: number, rt: FormRuntime): void {
  const hex = field.initial ? toDisplayHex(field.initial) : "#FF8800";
  rt.drafts[pi] = `"${hex}"`;
  const wrap = document.createElement("div");
  wrap.className = "brink-arg-form-picker";
  mountColorPicker(wrap, hex, (h) => {
    rt.drafts[pi] = `"${h}"`;
    rt.notify(pi);
  });
  row.appendChild(wrap);
}

/** A value-list dropdown (#174): display labels, insert literals verbatim. An
 *  existing value not in the list is preserved as a selectable option. */
function buildEnumField(row: HTMLElement, field: FormField, pi: number, rt: FormRuntime): void {
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
  rt.drafts[pi] = select.value;
  select.addEventListener("change", () => {
    rt.drafts[pi] = select.value;
    rt.notify(pi);
  });
  row.appendChild(select);
}

/** A host single-slot widget. `editor.surface: "inline"` mounts the control
 *  directly in the row (#990, e.g. a bool toggle); otherwise a summary chip +
 *  Edit expands the editor in place (see `mountHostEditor`). `resolve` updates
 *  the draft (+ summary, when chipped); `cancel` collapses (chipped only). */
function buildHostField(row: HTMLElement, field: FormField, pi: number, rt: FormRuntime): void {
  const widget = field.hostWidget!;
  // Empty slots fall back to the param name (a placeholder identifier) so Apply
  // never emits an empty literal — matching the text field.
  rt.drafts[pi] = field.initial && field.initial.length > 0 ? field.initial : field.paramName;
  const ctx = (): ArgumentWidgetContext => ({
    type: widget.type,
    external: rt.external ?? "",
    paramNames: [field.paramName],
    values: [stripQuotes(rt.drafts[pi])],
  });
  // A single slot has no inter-arg context, so nothing re-renders it.
  mountHostEditor(row, widget, ctx, [], rt, (vals) => {
    if (vals.length > 0) {
      rt.drafts[pi] = vals[0];
      rt.notify(pi);
    }
  });
}

/** An arg-group widget: one control over several params. Context is resolved
 *  from live sibling drafts via `contextParams` at the moment the editor opens
 *  (or re-renders), so picking the map first drives the point picker. */
function buildGroupField(row: HTMLElement, group: FormGroup, rt: FormRuntime): void {
  group.paramIndices.forEach((pi, k) => {
    const init = group.initialValues[k];
    rt.drafts[pi] = init && init.length > 0 ? init : group.paramNames[k];
  });
  row.appendChild(labelCell({ paramName: group.paramNames.join(", "), typeName: group.typeName }));
  const contextIndices = Object.values(group.contextParams ?? {});
  const ctx = (): ArgumentWidgetContext => {
    const context: Record<string, string> = {};
    for (const [key, idx] of Object.entries(group.contextParams ?? {})) {
      if (rt.drafts[idx] !== undefined) context[key] = stripQuotes(rt.drafts[idx]);
    }
    return {
      type: group.hostWidget.type,
      external: rt.external ?? "",
      paramNames: group.paramNames,
      values: group.paramIndices.map((pi) => stripQuotes(rt.drafts[pi])),
      context,
    };
  };
  mountHostEditor(row, group.hostWidget, ctx, contextIndices, rt, (vals) => {
    group.paramIndices.forEach((pi, k) => {
      if (vals[k] !== undefined) rt.drafts[pi] = vals[k];
    });
  });
}

/**
 * `editor.surface: "inline"` (#990): mount the widget's control directly in
 * the row, where a text field would sit — no summary chip, no Edit toggle.
 * `resolve` writes the draft live (a toggle/stepper resolves on every
 * interaction, not once-then-close); `cancel` is a no-op — there is no
 * collapsed state to revert to. Re-renders if a sibling draft in
 * `contextIndices` changes, same as the popover/modal path.
 */
function mountInlineHostEditor(
  row: HTMLElement,
  widget: ArgumentWidget,
  ctx: () => ArgumentWidgetContext,
  contextIndices: number[],
  rt: FormRuntime,
  onResolve: (values: string[]) => void,
): void {
  const container = document.createElement("div");
  container.className = "brink-arg-form-inline";
  const host: ArgumentWidgetEditorHost = {
    resolve: (values) => onResolve(values),
    cancel: () => {},
  };
  let teardown: (() => void) | undefined;
  const renderEditor = (): void => {
    teardown?.();
    if (teardown) rt.teardowns.delete(teardown);
    container.replaceChildren();
    teardown = widget.editor.render(ctx(), host, container);
    if (teardown) rt.teardowns.add(teardown);
  };
  renderEditor();
  if (contextIndices.length > 0) {
    const redrawer = (changedIndex: number): void => {
      if (contextIndices.includes(changedIndex)) renderEditor();
    };
    rt.redrawers.add(redrawer);
  }
  row.appendChild(container);
}

/**
 * Shared chrome for an embedded host editor (single-slot or group). Dispatches
 * on the widget's `editor.surface` (#990):
 *
 * - **`"inline"`** — the control mounts directly in the row, in the text
 *   field's place: no summary chip, no Edit button, no expand/collapse. The
 *   right shape for a primitive control (a bool toggle, a number stepper)
 *   that IS the field, not an editor of it.
 * - **`"popover"` / `"modal"` / unset** — a summary chip + Edit toggling an
 *   expandable editor container (unchanged), for rich pickers where showing
 *   the full editor inline in every row would be too heavy.
 *
 * `onResolve` writes drafts; while a non-inline editor is open (or an inline
 * one is always mounted), it re-renders if a sibling draft in
 * `contextIndices` changes.
 */
function mountHostEditor(
  row: HTMLElement,
  widget: ArgumentWidget,
  ctx: () => ArgumentWidgetContext,
  contextIndices: number[],
  rt: FormRuntime,
  onResolve: (values: string[]) => void,
): void {
  if (widget.editor.surface === "inline") {
    mountInlineHostEditor(row, widget, ctx, contextIndices, rt, onResolve);
    return;
  }

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
  let redrawer: ((changedIndex: number) => void) | undefined;

  // (Re)render the host editor into the container with the current context.
  const renderEditor = (host: ArgumentWidgetEditorHost): void => {
    teardown?.();
    if (teardown) rt.teardowns.delete(teardown);
    editor.replaceChildren();
    teardown = widget.editor.render(ctx(), host, editor);
    if (teardown) rt.teardowns.add(teardown);
  };

  const close = (): void => {
    teardown?.();
    if (teardown) rt.teardowns.delete(teardown);
    teardown = undefined;
    if (redrawer) rt.redrawers.delete(redrawer);
    redrawer = undefined;
    editor.replaceChildren();
    editor.hidden = true;
    refresh();
  };

  const open = (): void => {
    editor.hidden = false;
    const host: ArgumentWidgetEditorHost = {
      resolve: (values) => {
        onResolve(values);
        close();
      },
      cancel: () => close(),
    };
    renderEditor(host);
    // While open, re-render when a sibling draft this editor reads as context
    // changes (e.g. the map dropdown), so it reflects the new context live.
    if (contextIndices.length > 0) {
      redrawer = (changedIndex) => {
        if (contextIndices.includes(changedIndex)) renderEditor(host);
      };
      rt.redrawers.add(redrawer);
    }
  };

  const edit = document.createElement("button");
  edit.type = "button";
  edit.className = "brink-arg-form-edit";
  edit.textContent = "Edit…";
  edit.addEventListener("click", () => {
    if (editor.hidden) open();
    else close();
  });

  row.append(summary, edit, editor);
}
