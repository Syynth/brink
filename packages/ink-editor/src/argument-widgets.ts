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
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import type { CallWidgetSite } from "@brink/wasm-types";
import { getBuiltinWidget, type WidgetEditorHost } from "./widget-registry.js";
import "./color-widget.js"; // side-effect: registers the built-in "color" widget

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

export interface ArgumentWidgetsOptions {
  getArgumentWidgets: (source: string, start: number, end: number) => CallWidgetSite[];
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
    const decos: { pos: number; deco: Decoration }[] = [];
    for (const site of sites) {
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

  return ViewPlugin.fromClass(
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
}
