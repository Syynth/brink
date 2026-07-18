/**
 * Argument-widget editing (argument-widget spec stage 2). Driven by the
 * brink-ide `argument_widgets` query, this CM extension renders, per call:
 *
 *  - **Edit** — an inline widget (e.g. the `color` swatch) on a *Filled* slot;
 *    clicking opens the widget editor and rewrites the literal in place.
 *  - **Fill** — a ghost placeholder (`‹color›`) on the first *Empty* widget
 *    slot; clicking opens the editor and inserts a literal at that position.
 *
 * Both go through the shared widget registry, so the `color` built-in (and
 * future widgets / host widgets) get Edit + Fill for free. Supersedes the
 * `color_hints` path.
 */

import { RangeSetBuilder, StateEffect, StateField, type Extension } from "@codemirror/state";
import { pickedCompletion } from "@codemirror/autocomplete";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  keymap,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import type {
  CallWidgetSite,
  SlotWidget,
  GroupWidgetSite,
  ArgumentWidget,
  ArgumentWidgetContext,
  ArgumentWidgetEditorHost,
  ValueItem,
} from "@brink/wasm-types";
import { getBuiltinWidget, getHostWidget, type WidgetEditorHost } from "./widget-registry.js";
import { openArgumentForm, type FormField, type FormGroup } from "./argument-form.js";
import { openPopover } from "./widget-popover.js";
import { openModal } from "./widget-modal.js";
import { ensureStructuralStyles } from "./structural-styles.js";
import "./color-widget.js"; // side-effect: registers the built-in "color" widget

/**
 * How the *inline* call-level glyph is shown (spec §6.5). Independent of the
 * hover-card "edit arguments" action, which is always available (it costs no
 * in-text chrome):
 *  - `off`    — no inline glyph (the hover card + keybind + panel still launch the Form)
 *  - `hover`  — inline glyph, revealed when the line is hovered
 *  - `inline` — inline glyph, always visible
 */
export type FormGlyphMode = "off" | "hover" | "inline";

/** The inline-glyph mode when none is configured — `off`, since the always-on
 *  hover-card action already launches the Form without in-text chrome. */
export const DEFAULT_FORM_GLYPH_MODE: FormGlyphMode = "off";

// Live glyph mode: a StateField the decorations read, switched by an effect so
// the Settings toggle reconfigures the glyph without rebuilding the editor.
const setFormGlyphEffect = StateEffect.define<FormGlyphMode>();
const formGlyphField = StateField.define<FormGlyphMode>({
  create: () => DEFAULT_FORM_GLYPH_MODE,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setFormGlyphEffect)) return e.value;
    return value;
  },
});

/** Switch a view's inline-glyph mode live (the Settings toggle dispatches this). */
export function setFormGlyphMode(view: EditorView, mode: FormGlyphMode): void {
  view.dispatch({ effects: setFormGlyphEffect.of(mode) });
}

// Live auto-open flag: whether accepting a function completion opens the Form.
const setAutoOpenEffect = StateEffect.define<boolean>();
const autoOpenField = StateField.define<boolean>({
  create: () => false,
  update(value, tr) {
    for (const e of tr.effects) if (e.is(setAutoOpenEffect)) return e.value;
    return value;
  },
});

/** Toggle a view's completion-accept auto-open live (the Settings toggle). */
export function setFormAutoOpen(view: EditorView, on: boolean): void {
  view.dispatch({ effects: setAutoOpenEffect.of(on) });
}

/** The form-launcher icon — a small "fields in a box" mark (currentColor). */
export const FORM_GLYPH_ICON =
  '<svg viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1.4" ' +
  'stroke-linecap="round" aria-hidden="true">' +
  '<rect x="2.5" y="2.5" width="11" height="11" rx="2.5"/>' +
  '<path d="M5 6h6M5 9h4"/></svg>';

/**
 * Open the whole-call Form for `site`, anchored to `anchor`. On Apply, the
 * call's entire argument list is replaced (depth-counted paren range). Shared
 * by the in-editor glyph and the hover-card action.
 */
export function openCallForm(anchor: HTMLElement, site: CallWidgetSite, view: EditorView): void {
  // Seed drafts with the RAW literal text (quotes included) so unedited string
  // args round-trip; widget context strips quotes for display.
  const rawAt = (from: number, to: number): string => view.state.doc.sliceString(from, to);

  // Read a member's current value from its slot — the Form is driven by the
  // signature metadata (every declared group, always), seeding values from
  // whatever arguments exist, so a partial or over-full call still renders the
  // right widgets rather than degrading to plain text fields.
  const slotRaw = (idx: number): string => {
    const slot = site.slots[idx] as SlotWidget | undefined;
    return slot && slot.state.kind === "filled" ? rawAt(slot.state.start, slot.state.end) : "";
  };

  const groups: FormGroup[] = [];
  const grouped = new Set<number>();
  for (const group of site.declared_groups ?? []) {
    const widget = getHostWidget(group.type);
    if (widget === undefined) continue;
    for (const idx of group.param_indices) grouped.add(idx);
    groups.push({
      paramIndices: group.param_indices,
      paramNames: group.param_names,
      typeName: group.type,
      hostWidget: widget,
      surface: group.surface,
      initialValues: group.param_indices.map(slotRaw),
      contextParams: group.context_params,
    });
  }

  const fields: FormField[] = [];
  site.slots.forEach((slot, i) => {
    if (grouped.has(i)) return;
    fields.push({
      paramName: slot.param_name,
      paramIndex: i,
      typeName: slot.type_name,
      typeDisplay: slot.type_display,
      widgetKind: slot.widget,
      values: slot.values,
      hostWidget: matchHostWidget(slot),
      initial: slot.state.kind === "filled" ? rawAt(slot.state.start, slot.state.end) : undefined,
    });
  });

  const sig = `${site.callee}(${site.slots.map((s) => s.param_name).join(", ")})`;
  openArgumentForm(anchor, {
    title: sig,
    external: site.callee,
    applyLabel: "Apply",
    fields,
    groups,
    onApply: (literals) => {
      const range = liveParenRange(view, site.name_end);
      if (range) {
        view.dispatch({
          changes: { from: range.open + 1, to: range.close, insert: literals.join(", ") },
        });
      }
    },
    onCancel: () => {},
  });
}

/**
 * Open the Form for the call the cursor is inside (the keybinding command).
 * Anchors a transient element at the cursor — the popover captures its rect, so
 * it is removed immediately. Returns false (so the key falls through) when the
 * cursor is not within a call.
 */
function openFormAtCursor(
  view: EditorView,
  getArgumentWidgets: (source: string, start: number, end: number) => CallWidgetSite[],
): boolean {
  const pos = view.state.selection.main.head;
  const source = view.state.doc.toString();
  let sites: CallWidgetSite[];
  try {
    sites = getArgumentWidgets(source, 0, source.length);
  } catch {
    return false;
  }
  const site = sites.find((s) => {
    if (s.slots.length === 0) return false;
    const range = liveParenRange(view, s.name_end);
    const end = range ? range.close + 1 : s.name_end;
    return pos >= s.name_start && pos <= end;
  });
  if (!site) return false;

  ensureStructuralStyles();
  const coords = view.coordsAtPos(pos);
  const anchor = document.createElement("div");
  // Invisible measurement scaffolding: `.brink-form-anchor`'s class rule pins
  // it (position: fixed, 1px wide); the cursor rect rides on custom properties.
  anchor.className = "brink-form-anchor";
  if (coords) {
    anchor.style.setProperty("--brink-popup-left", `${coords.left}px`);
    anchor.style.setProperty("--brink-popup-top", `${coords.top}px`);
    anchor.style.setProperty("--brink-anchor-height", `${Math.max(1, coords.bottom - coords.top)}px`);
  }
  document.body.appendChild(anchor);
  openCallForm(anchor, site, view);
  anchor.remove();
  return true;
}

/** Delimiters that end a bare (unquoted) literal argument. */
const ARG_DELIMITERS = new Set([",", ")", " ", "\t", "\n", "\r"]);

/**
 * The current source range of the argument literal starting at `from`. A quoted
 * string runs to its closing quote; a bare literal (int/float/bool/identifier)
 * runs to the next delimiter (`,`, `)`, whitespace). Recomputed at edit time so
 * successive live edits stay correct even when the literal's length changes.
 *
 * Bare literals matter for host widgets on non-string types (e.g. an `int`
 * `item_id`): quote-only matching left their Edit-replace a no-op (#242).
 */
export function liveArgRange(
  view: EditorView,
  from: number,
): { from: number; to: number } | null {
  const doc = view.state.doc;
  if (from < 0 || from >= doc.length) return null;
  if (doc.sliceString(from, from + 1) === '"') {
    for (let i = from + 1; i < doc.length; i++) {
      if (doc.sliceString(i, i + 1) === '"') return { from, to: i + 1 };
    }
    return null;
  }
  let i = from;
  while (i < doc.length && !ARG_DELIMITERS.has(doc.sliceString(i, i + 1))) i++;
  return i > from ? { from, to: i } : null;
}

/**
 * The call's parenthesis range, scanning from just after the function name
 * (`nameEnd`) — only whitespace may sit between the name and `(`. Returns the
 * `(` and matching `)` positions (depth-counted, so nested calls in args are
 * handled). The form replaces `[open+1, close)` with the composed arg list.
 */
function liveParenRange(view: EditorView, nameEnd: number): { open: number; close: number } | null {
  const doc = view.state.doc;
  let open = -1;
  for (let i = nameEnd; i < doc.length; i++) {
    const c = doc.sliceString(i, i + 1);
    if (c === "(") {
      open = i;
      break;
    }
    if (c !== " " && c !== "\t") return null;
  }
  if (open < 0) return null;
  let depth = 0;
  for (let i = open; i < doc.length; i++) {
    const c = doc.sliceString(i, i + 1);
    if (c === "(") depth++;
    else if (c === ")") {
      depth--;
      if (depth === 0) return { open, close: i };
    }
  }
  return null;
}

/** A stable key for a call site — so the glyph reuses its DOM until the call's
 *  identity or slot values change. */
function siteKey(site: CallWidgetSite): string {
  const slots = site.slots
    .map((s) => `${s.param_name}:${s.state.kind === "filled" ? s.state.value : s.state.kind}`)
    .join(",");
  return `${site.name_end}|${site.callee}|${slots}`;
}

/** An inline editor affordance on a filled literal — Edit (replace in place). */
class EditWidget extends WidgetType {
  constructor(
    readonly kind: string,
    readonly value: string,
    readonly from: number,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: EditWidget): boolean {
    return other.kind === this.kind && other.value === this.value && other.from === this.from;
  }

  toDOM(): HTMLElement {
    const widget = getBuiltinWidget(this.kind);
    const el = widget ? widget.renderInline(this.value) : document.createElement("span");
    this.wire(el);
    return el;
  }

  updateDOM(dom: HTMLElement): boolean {
    // Keep the same element across live edits so an open popover stays anchored;
    // refresh its appearance from the widget's inline renderer.
    const widget = getBuiltinWidget(this.kind);
    if (!widget) return false;
    const fresh = widget.renderInline(this.value);
    dom.style.cssText = fresh.style.cssText;
    dom.title = fresh.title;
    return true;
  }

  private wire(el: HTMLElement): void {
    el.addEventListener("click", () => this.open(el));
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        this.open(el);
      }
    });
  }

  private open(anchor: HTMLElement): void {
    const widget = getBuiltinWidget(this.kind);
    if (!widget) return;
    const host: WidgetEditorHost = {
      initial: this.value,
      resolve: (value) => {
        const range = liveArgRange(this.view, this.from);
        if (range) {
          this.view.dispatch({
            changes: { from: range.from, to: range.to, insert: `"${value}"` },
          });
        }
      },
      cancel: () => {},
    };
    widget.openEditor(anchor, host);
  }

  ignoreEvent(): boolean {
    return true;
  }
}

/** A ghost placeholder on an empty slot — Fill (insert, then keep editing). */
class FillGhostWidget extends WidgetType {
  constructor(
    readonly kind: string,
    readonly paramName: string,
    readonly insertAt: number,
    readonly needsComma: boolean,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: FillGhostWidget): boolean {
    return (
      other.kind === this.kind &&
      other.paramName === this.paramName &&
      other.insertAt === this.insertAt &&
      other.needsComma === this.needsComma
    );
  }

  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "brink-fill-ghost";
    el.textContent = `‹${this.paramName}›`;
    el.setAttribute("role", "button");
    el.tabIndex = 0;
    el.title = `Set ${this.paramName}`;
    el.addEventListener("click", () => this.open(el));
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        this.open(el);
      }
    });
    return el;
  }

  private open(anchor: HTMLElement): void {
    const widget = getBuiltinWidget(this.kind);
    if (!widget) return;
    // First resolve inserts the literal; subsequent live resolves replace the
    // range we just inserted (so dragging the picker doesn't stack inserts).
    let inserted: { from: number; to: number } | null = null;
    const host: WidgetEditorHost = {
      initial: "",
      resolve: (value) => {
        const lit = `"${value}"`;
        if (inserted) {
          this.view.dispatch({ changes: { from: inserted.from, to: inserted.to, insert: lit } });
          inserted = { from: inserted.from, to: inserted.from + lit.length };
        } else {
          const prefix = this.needsComma ? ", " : "";
          const from = this.insertAt + prefix.length;
          this.view.dispatch({ changes: { from: this.insertAt, insert: prefix + lit } });
          inserted = { from, to: from + lit.length };
        }
      },
      cancel: () => {},
    };
    widget.openEditor(anchor, host);
  }

  ignoreEvent(): boolean {
    return true;
  }
}

// ── Host widgets (argument-widget-spec §3) ──────────────────────────

/**
 * A host widget registered for a slot's declared widget kind, falling back to
 * its type name — which may itself be a base type (`bool`/`int`/`float`/
 * `string`), not just a semantic type (argument-widget-spec §3.1, #990). This
 * is the one fallback path base-type registration relies on: a host that
 * calls `setHostWidgets([{ type: "bool", … }])` matches every `bool` slot
 * with no per-slot `widget` declaration required.
 */
export function matchHostWidget(slot: SlotWidget): ArgumentWidget | undefined {
  if (slot.widget !== undefined) {
    const byKind = getHostWidget(slot.widget);
    if (byKind) return byKind;
  }
  return slot.type_name !== undefined ? getHostWidget(slot.type_name) : undefined;
}

/** Build the context handed to a host widget for a single slot. */
function hostContext(
  callee: string,
  slot: SlotWidget,
  type: string,
  values: string[],
): ArgumentWidgetContext {
  return {
    type,
    external: callee,
    paramNames: [slot.param_name],
    values,
  };
}

/** Open a host widget's editor in the studio chrome — a popover, or a modal when
 *  the widget requests `surface: "modal"`. `resolve(values)` commits + closes;
 *  `cancel()` closes. */
function openHostEditor(
  anchor: HTMLElement,
  widget: ArgumentWidget,
  ctx: ArgumentWidgetContext,
  onResolve: (values: string[]) => void,
  surfaceOverride?: "popover" | "modal",
): void {
  let teardown: (() => void) | undefined;
  let surface: { close(): void } | null = null;
  const host: ArgumentWidgetEditorHost = {
    resolve: (values) => {
      onResolve(values);
      surface?.close();
    },
    cancel: () => surface?.close(),
  };
  const render = (container: HTMLElement): void => {
    teardown = widget.editor.render(ctx, host, container);
  };
  // The manifest's per-call-site surface wins, else the widget's own preference.
  const kind = surfaceOverride ?? widget.editor.surface;
  // Mount the modal inside the `.brink-studio` root (theme tokens are scoped
  // there) so host content inherits the `--bs-*` palette; the fixed backdrop
  // still covers the viewport. The popover stays on body — it positions
  // absolutely against the page and reparenting would shift its origin.
  const root = anchor.closest<HTMLElement>(".brink-studio") ?? undefined;
  surface =
    kind === "modal"
      ? openModal(render, () => teardown?.(), root)
      : openPopover(anchor, render, () => teardown?.());
}

/** An inline chip on a filled literal, rendered from a host widget's label
 *  data — clicking opens the host editor; resolve replaces the literal. */
class HostEditWidget extends WidgetType {
  constructor(
    readonly widget: ArgumentWidget,
    readonly type: string,
    readonly callee: string,
    readonly slot: SlotWidget,
    readonly value: string,
    readonly from: number,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: HostEditWidget): boolean {
    return (
      other.type === this.type && other.value === this.value && other.from === this.from
    );
  }

  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "brink-host-chip";
    const ctx = hostContext(this.callee, this.slot, this.type, [this.value]);
    const label = this.widget.inline?.(ctx);
    el.textContent = label?.text ?? this.value;
    if (label?.className) el.classList.add(label.className);
    el.setAttribute("role", "button");
    el.tabIndex = 0;
    el.title = `Edit ${this.slot.param_name}`;
    const open = (): void =>
      openHostEditor(el, this.widget, ctx, (values) => {
        const range = liveArgRange(this.view, this.from);
        if (range && values.length > 0) {
          this.view.dispatch({ changes: { from: range.from, to: range.to, insert: values[0] } });
        }
      });
    el.addEventListener("click", open);
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        open();
      }
    });
    return el;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

/** A ghost on an empty slot whose type has a host widget — Fill via the host
 *  editor; resolve inserts the literal. */
class HostFillGhostWidget extends WidgetType {
  constructor(
    readonly widget: ArgumentWidget,
    readonly type: string,
    readonly callee: string,
    readonly slot: SlotWidget,
    readonly insertAt: number,
    readonly needsComma: boolean,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: HostFillGhostWidget): boolean {
    return (
      other.type === this.type &&
      other.insertAt === this.insertAt &&
      other.needsComma === this.needsComma
    );
  }

  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "brink-fill-ghost";
    el.textContent = `‹${this.slot.param_name}›`;
    el.setAttribute("role", "button");
    el.tabIndex = 0;
    el.title = `Set ${this.slot.param_name}`;
    const ctx = hostContext(this.callee, this.slot, this.type, []);
    const open = (): void =>
      openHostEditor(el, this.widget, ctx, (values) => {
        if (values.length === 0) return;
        const prefix = this.needsComma ? ", " : "";
        this.view.dispatch({ changes: { from: this.insertAt, insert: prefix + values[0] } });
      });
    el.addEventListener("click", open);
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        open();
      }
    });
    return el;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

// ── Value-list picker (#174 / #224) ─────────────────────────────────

/** A studio dropdown for a value-list slot: a filter box + the items (label +
 *  detail). Picking calls `onPick` with the chosen item's literal value. */
function openValuePicker(
  anchor: HTMLElement,
  items: ValueItem[],
  current: string,
  onPick: (value: string) => void,
): void {
  let popover: { close(): void } | null = null;
  popover = openPopover(
    anchor,
    (container) => {
      const root = document.createElement("div");
      root.className = "brink-value-picker";
      const filter = document.createElement("input");
      filter.type = "text";
      filter.className = "brink-value-filter";
      filter.placeholder = "Filter…";
      filter.spellcheck = false;
      const list = document.createElement("div");
      list.className = "brink-value-list";
      const render = (q: string): void => {
        const ql = q.toLowerCase();
        list.replaceChildren();
        for (const it of items) {
          // Filter on label, value, AND detail (#211) — e.g. "Switch #5".
          if (
            ql &&
            !it.label.toLowerCase().includes(ql) &&
            !it.value.toLowerCase().includes(ql) &&
            !(it.detail?.toLowerCase().includes(ql) ?? false)
          ) {
            continue;
          }
          const btn = document.createElement("button");
          btn.type = "button";
          btn.className = "brink-value-item";
          if (it.value === current) btn.setAttribute("aria-current", "true");
          const label = document.createElement("span");
          label.className = "brink-value-item-label";
          label.textContent = it.label;
          btn.appendChild(label);
          if (it.detail) {
            const detail = document.createElement("span");
            detail.className = "brink-value-item-detail";
            detail.textContent = it.detail;
            btn.appendChild(detail);
          }
          btn.addEventListener("click", () => {
            onPick(it.value);
            popover?.close();
          });
          list.appendChild(btn);
        }
      };
      filter.addEventListener("input", () => render(filter.value));
      render("");
      root.append(filter, list);
      container.appendChild(root);
      setTimeout(() => filter.focus(), 0);
    },
    () => {},
  );
}

/** A clickable label chip on a filled value-list literal (#224): shows the
 *  matched item's label and opens the dropdown picker; choosing rewrites the
 *  literal in place, mirroring the existing literal's quoting. */
class ValueEditWidget extends WidgetType {
  constructor(
    readonly slot: SlotWidget,
    readonly value: string,
    readonly start: number,
    readonly end: number,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: ValueEditWidget): boolean {
    return other.value === this.value && other.start === this.start && other.end === this.end;
  }

  toDOM(): HTMLElement {
    const item = this.slot.values?.find((it) => it.value === this.value);
    const el = document.createElement("span");
    el.className = "brink-value-chip";
    el.textContent = `⟨${item ? item.label : this.value}⟩`;
    el.setAttribute("role", "button");
    el.tabIndex = 0;
    el.title = `Pick ${this.slot.param_name}`;
    const open = (): void => {
      openValuePicker(el, this.slot.values ?? [], this.value, (picked) => {
        const range = liveArgRange(this.view, this.start);
        if (!range) return;
        const raw = this.view.state.doc.sliceString(range.from, range.to);
        const insert = raw.startsWith('"') ? `"${picked}"` : picked;
        this.view.dispatch({ changes: { from: range.from, to: range.to, insert } });
      });
    };
    el.addEventListener("click", open);
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        open();
      }
    });
    return el;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

// ── Arg-group widgets (argument-widget-spec §2) ─────────────────────

/** The context handed to a host widget for an arg-group. */
function groupContext(
  group: GroupWidgetSite,
  callee: string,
  values: string[],
): ArgumentWidgetContext {
  return {
    type: group.type,
    external: callee,
    paramNames: group.param_names,
    values,
    context: group.context,
  };
}

/** A chip over a uniformly-filled arg group — Edit; resolve multi-replaces each
 *  member's span. Anchored at the first member's start. */
class GroupEditWidget extends WidgetType {
  constructor(
    readonly widget: ArgumentWidget,
    readonly group: GroupWidgetSite,
    readonly callee: string,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: GroupEditWidget): boolean {
    return JSON.stringify(other.group) === JSON.stringify(this.group);
  }

  toDOM(): HTMLElement {
    const values = this.group.state.kind === "filled" ? this.group.state.values : [];
    const ctx = groupContext(this.group, this.callee, values);
    const label = this.widget.inline?.(ctx);
    const el = document.createElement("span");
    el.className = "brink-host-chip";
    el.textContent = label?.text ?? `(${values.join(", ")})`;
    if (label?.className) el.classList.add(label.className);
    el.setAttribute("role", "button");
    el.tabIndex = 0;
    el.title = `Edit ${this.group.param_names.join(", ")}`;
    const open = (): void =>
      openHostEditor(
        el,
        this.widget,
        ctx,
        (newValues) => {
          if (this.group.state.kind !== "filled") return;
          const spans = this.group.state.spans;
          const changes = spans
            .slice(0, newValues.length)
            .map((s, k) => ({ from: s[0], to: s[1], insert: newValues[k] }));
          if (changes.length > 0) this.view.dispatch({ changes });
        },
        this.group.surface,
      );
    el.addEventListener("click", open);
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        open();
      }
    });
    return el;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

/** A ghost over an empty arg group — Fill; resolve inserts the members joined
 *  by `, ` at the group's insert point. */
class GroupFillWidget extends WidgetType {
  constructor(
    readonly widget: ArgumentWidget,
    readonly group: GroupWidgetSite,
    readonly callee: string,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: GroupFillWidget): boolean {
    return JSON.stringify(other.group) === JSON.stringify(this.group);
  }

  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className = "brink-fill-ghost";
    el.textContent = `‹${this.group.param_names.join(", ")}›`;
    el.setAttribute("role", "button");
    el.tabIndex = 0;
    el.title = `Set ${this.group.param_names.join(", ")}`;
    const ctx = groupContext(this.group, this.callee, []);
    const open = (): void =>
      openHostEditor(
        el,
        this.widget,
        ctx,
        (newValues) => {
          if (this.group.state.kind !== "empty" || newValues.length === 0) return;
          const prefix = this.group.state.needs_leading_comma ? ", " : "";
          this.view.dispatch({
            changes: { from: this.group.state.insert_at, insert: prefix + newValues.join(", ") },
          });
        },
        this.group.surface,
      );
    el.addEventListener("click", open);
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        open();
      }
    });
    return el;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

/** A call-level glyph at the function name — opens the whole-call Form. */
class FormGlyphWidget extends WidgetType {
  constructor(
    readonly site: CallWidgetSite,
    readonly mode: FormGlyphMode,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: FormGlyphWidget): boolean {
    return other.mode === this.mode && siteKey(other.site) === siteKey(this.site);
  }

  toDOM(): HTMLElement {
    const el = document.createElement("span");
    el.className =
      this.mode === "hover" ? "brink-form-glyph brink-form-glyph--hover" : "brink-form-glyph";
    el.innerHTML = FORM_GLYPH_ICON;
    el.setAttribute("role", "button");
    el.tabIndex = 0;
    el.title = `Edit ${this.site.callee}(…)`;
    el.addEventListener("click", () => openCallForm(el, this.site, this.view));
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        openCallForm(el, this.site, this.view);
      }
    });
    return el;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

export interface ArgumentWidgetsOptions {
  getArgumentWidgets: (source: string, start: number, end: number) => CallWidgetSite[];
  /** How the inline call-level form glyph is shown. Default `off`. */
  formGlyph?: FormGlyphMode;
  /** Accepting a function completion inserts `()` + opens the Form. Default false. */
  autoOpen?: boolean;
}

export function argumentWidgetsExtension(options: ArgumentWidgetsOptions): Extension {
  const build = (view: EditorView): DecorationSet => {
    const source = view.state.doc.toString();
    let sites: CallWidgetSite[];
    try {
      sites = options.getArgumentWidgets(source, 0, source.length);
    } catch {
      return Decoration.none;
    }

    // Collect (pos, side, deco) then sort — Fill ghosts and Edit swatches can
    // interleave across calls, and RangeSetBuilder needs sorted input.
    const glyphMode = view.state.field(formGlyphField);
    const decos: { pos: number; deco: Decoration }[] = [];
    for (const site of sites) {
      // Call-level form glyph, just after the function name (`off` shows none —
      // the always-on hover-card action launches the Form without it).
      if ((glyphMode === "inline" || glyphMode === "hover") && site.slots.length > 0) {
        decos.push({
          pos: site.name_end,
          deco: Decoration.widget({
            widget: new FormGlyphWidget(site, glyphMode, view),
            side: 1,
          }),
        });
      }
      // Arg-group widgets (spec §2): one chip/ghost over the group, anchored at
      // its first member. Grouped params are then skipped per-slot.
      const groupedParams = new Set<number>();
      for (const group of site.groups) {
        const widget = getHostWidget(group.type);
        if (widget === undefined) continue;
        for (const idx of group.param_indices) groupedParams.add(idx);
        if (group.state.kind === "filled") {
          const pos = group.state.spans[0]?.[0];
          if (pos !== undefined) {
            decos.push({
              pos,
              deco: Decoration.widget({
                widget: new GroupEditWidget(widget, group, site.callee, view),
                side: 2,
              }),
            });
          }
        } else {
          decos.push({
            pos: group.state.insert_at,
            deco: Decoration.widget({
              widget: new GroupFillWidget(widget, group, site.callee, view),
              side: 2,
            }),
          });
        }
      }

      let filledGhost = false; // render a ghost only for the first empty slot
      for (let slotIdx = 0; slotIdx < site.slots.length; slotIdx++) {
        if (groupedParams.has(slotIdx)) continue; // rendered by the group widget
        const slot = site.slots[slotIdx];
        const kind = slot.widget;
        const builtin = kind !== undefined ? getBuiltinWidget(kind) : undefined;
        const host = builtin ? undefined : matchHostWidget(slot);
        if (builtin === undefined && host === undefined) {
          // Value-list slot (#174/#224): a clickable label chip on the filled
          // literal that opens the studio dropdown picker.
          if (slot.state.kind === "filled" && slot.values && slot.values.length > 0) {
            decos.push({
              pos: slot.state.end,
              deco: Decoration.widget({
                widget: new ValueEditWidget(
                  slot,
                  slot.state.value,
                  slot.state.start,
                  slot.state.end,
                  view,
                ),
                side: 1,
              }),
            });
          }
          continue;
        }

        if (slot.state.kind === "filled") {
          const widget =
            builtin !== undefined
              ? new EditWidget(kind!, slot.state.value, slot.state.start, view)
              : new HostEditWidget(
                  host!,
                  host!.type,
                  site.callee,
                  slot,
                  slot.state.value,
                  slot.state.start,
                  view,
                );
          decos.push({
            pos: slot.state.start,
            deco: Decoration.widget({ widget, side: 2 }),
          });
        } else if (slot.state.kind === "empty" && !filledGhost) {
          filledGhost = true;
          const widget =
            builtin !== undefined
              ? new FillGhostWidget(
                  kind!,
                  slot.param_name,
                  slot.state.insert_at,
                  slot.state.needs_leading_comma,
                  view,
                )
              : new HostFillGhostWidget(
                  host!,
                  host!.type,
                  site.callee,
                  slot,
                  slot.state.insert_at,
                  slot.state.needs_leading_comma,
                  view,
                );
          decos.push({
            pos: slot.state.insert_at,
            deco: Decoration.widget({ widget, side: 2 }),
          });
        }
      }
    }

    decos.sort((a, b) => a.pos - b.pos);
    const builder = new RangeSetBuilder<Decoration>();
    for (const { pos, deco } of decos) {
      if (pos < 0 || pos > source.length) continue;
      builder.add(pos, pos, deco);
    }
    return builder.finish();
  };

  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;
      constructor(view: EditorView) {
        this.decorations = build(view);
      }
      update(update: ViewUpdate): void {
        const modeChanged =
          update.startState.field(formGlyphField) !== update.state.field(formGlyphField);
        if (update.docChanged || update.viewportChanged || modeChanged) {
          this.decorations = build(update.view);
        }
      }
    },
    { decorations: (v) => v.decorations },
  );

  // Mod-Shift-A opens the Form for the call the cursor is inside.
  const keys = keymap.of([
    {
      key: "Mod-Shift-a",
      run: (view) => openFormAtCursor(view, options.getArgumentWidgets),
    },
  ]);

  // Auto-open: accepting a function/method completion inserts `()` and opens the
  // Form. Deferred out of the update listener (dispatch isn't allowed in it) so
  // the inserted parens are parsed before we query for the call.
  const autoOpen = EditorView.updateListener.of((update) => {
    if (!update.state.field(autoOpenField)) return;
    const view = update.view;
    for (const tr of update.transactions) {
      const picked = tr.annotation(pickedCompletion);
      if (!picked || (picked.type !== "function" && picked.type !== "method")) continue;
      setTimeout(() => {
        const pos = view.state.selection.main.head;
        const doc = view.state.doc;
        const nextChar = pos < doc.length ? doc.sliceString(pos, pos + 1) : "";
        if (nextChar !== "(") {
          view.dispatch({ changes: { from: pos, insert: "()" }, selection: { anchor: pos + 1 } });
        }
        openFormAtCursor(view, options.getArgumentWidgets);
      }, 0);
      break;
    }
  });

  return [
    formGlyphField.init(() => options.formGlyph ?? DEFAULT_FORM_GLYPH_MODE),
    autoOpenField.init(() => options.autoOpen ?? false),
    plugin,
    keys,
    autoOpen,
  ];
}
