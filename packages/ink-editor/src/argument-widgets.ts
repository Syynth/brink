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

import { RangeSetBuilder, type Extension } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  keymap,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import type { CallWidgetSite } from "@brink/wasm-types";
import { getBuiltinWidget, type WidgetEditorHost } from "./widget-registry.js";
import { openArgumentForm, type FormField } from "./argument-form.js";
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
  const fields: FormField[] = site.slots.map((slot) => ({
    paramName: slot.param_name,
    typeName: slot.type_name,
    widgetKind: slot.widget,
    initial: slot.state.kind === "filled" ? slot.state.value : undefined,
  }));
  const sig = `${site.callee}(${site.slots.map((s) => s.param_name).join(", ")})`;
  openArgumentForm(anchor, {
    title: sig,
    applyLabel: "Apply",
    fields,
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

  const coords = view.coordsAtPos(pos);
  const anchor = document.createElement("div");
  anchor.style.position = "fixed";
  if (coords) {
    anchor.style.left = `${coords.left}px`;
    anchor.style.top = `${coords.top}px`;
    anchor.style.width = "1px";
    anchor.style.height = `${Math.max(1, coords.bottom - coords.top)}px`;
  }
  document.body.appendChild(anchor);
  openCallForm(anchor, site, view);
  anchor.remove();
  return true;
}

/**
 * The current quoted-literal range starting at `from` (the opening quote).
 * Recomputed at edit time so successive live edits stay correct even when the
 * literal's length changes.
 */
function liveLiteralRange(view: EditorView, from: number): { from: number; to: number } | null {
  const doc = view.state.doc;
  if (from < 0 || from >= doc.length || doc.sliceString(from, from + 1) !== '"') return null;
  for (let i = from + 1; i < doc.length; i++) {
    if (doc.sliceString(i, i + 1) === '"') return { from, to: i + 1 };
  }
  return null;
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
        const range = liveLiteralRange(this.view, this.from);
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
  /** How the call-level form glyph is shown. Default `"hover"`. */
  formGlyph?: FormGlyphMode;
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
    const glyphMode = options.formGlyph ?? DEFAULT_FORM_GLYPH_MODE;
    const decos: { pos: number; deco: Decoration }[] = [];
    for (const site of sites) {
      // Call-level form glyph, just after the function name (inline modes only;
      // `hovercard` puts the action in the hover card, `off` shows nothing).
      if ((glyphMode === "inline" || glyphMode === "hover") && site.slots.length > 0) {
        decos.push({
          pos: site.name_end,
          deco: Decoration.widget({
            widget: new FormGlyphWidget(site, glyphMode, view),
            side: 1,
          }),
        });
      }
      let filledGhost = false; // render a ghost only for the first empty slot
      for (const slot of site.slots) {
        const kind = slot.widget;
        if (kind === undefined || getBuiltinWidget(kind) === undefined) continue;
        if (slot.state.kind === "filled") {
          decos.push({
            pos: slot.state.start,
            deco: Decoration.widget({
              widget: new EditWidget(kind, slot.state.value, slot.state.start, view),
              side: 2,
            }),
          });
        } else if (slot.state.kind === "empty" && !filledGhost) {
          filledGhost = true;
          decos.push({
            pos: slot.state.insert_at,
            deco: Decoration.widget({
              widget: new FillGhostWidget(
                kind,
                slot.param_name,
                slot.state.insert_at,
                slot.state.needs_leading_comma,
                view,
              ),
              side: 2,
            }),
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
        if (update.docChanged || update.viewportChanged) {
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

  return [plugin, keys];
}
