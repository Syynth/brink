/**
 * Built-in color picker for `hex_color` arguments (#174, argument-widget-spec
 * stage 1).
 *
 * For every `EXTERNAL` call argument whose semantic type carries the `color`
 * built-in widget, the brink-ide `color_hints` query returns the literal's span
 * + hex value. This extension renders the registry's `color` widget — a swatch
 * just before the literal; clicking opens a light studio popover picker, which
 * rewrites the literal in place. The swatch + editor go through the shared
 * widget registry (the seam future built-ins and host widgets plug into).
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
import type { ColorHint } from "@brink/wasm-types";
import { getBuiltinWidget, type WidgetEditorHost } from "./widget-registry.js";
import { toDisplayHex } from "./color-widget.js";
import "./color-widget.js"; // side-effect: registers the built-in "color" widget

/**
 * The current quoted-literal range starting at `from` (the opening quote).
 * Recomputed at edit time so successive live edits stay correct even when the
 * literal's length changes (e.g. `#FFF` → `#00FF00`).
 */
function liveLiteralRange(view: EditorView, from: number): { from: number; to: number } | null {
  const doc = view.state.doc;
  if (from < 0 || from >= doc.length || doc.sliceString(from, from + 1) !== '"') return null;
  for (let i = from + 1; i < doc.length; i++) {
    if (doc.sliceString(i, i + 1) === '"') return { from, to: i + 1 };
  }
  return null;
}

class ColorSwatchWidget extends WidgetType {
  constructor(
    readonly value: string,
    readonly from: number,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: ColorSwatchWidget): boolean {
    return other.value === this.value && other.from === this.from;
  }

  toDOM(): HTMLElement {
    const widget = getBuiltinWidget("color");
    const el = widget ? widget.renderInline(this.value) : document.createElement("span");
    el.addEventListener("click", () => this.open(el));
    el.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        this.open(el);
      }
    });
    return el;
  }

  updateDOM(dom: HTMLElement): boolean {
    // Reuse the same element across live edits so an open popover's anchor stays
    // valid; just refresh the swatch color + title.
    dom.style.background = toDisplayHex(this.value);
    dom.title = `Edit color (${this.value})`;
    return true;
  }

  private open(anchor: HTMLElement): void {
    const widget = getBuiltinWidget("color");
    if (!widget) return;
    const host: WidgetEditorHost = {
      initial: this.value,
      resolve: (hex) => {
        const range = liveLiteralRange(this.view, this.from);
        if (range) {
          this.view.dispatch({
            changes: { from: range.from, to: range.to, insert: `"${hex}"` },
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

export interface ColorPickerOptions {
  getColorHints: (source: string, start: number, end: number) => ColorHint[];
}

export function colorPickerExtension(options: ColorPickerOptions): Extension {
  const build = (view: EditorView): DecorationSet => {
    const source = view.state.doc.toString();
    let hints: ColorHint[];
    try {
      hints = options.getColorHints(source, 0, source.length);
    } catch {
      return Decoration.none;
    }
    hints.sort((a, b) => a.start - b.start);
    const builder = new RangeSetBuilder<Decoration>();
    for (const h of hints) {
      if (h.start < 0 || h.end > source.length || h.start > h.end) continue;
      builder.add(
        h.start,
        h.start,
        Decoration.widget({
          widget: new ColorSwatchWidget(h.value, h.start, view),
          // After the param-name inlay (side 1), immediately before the literal:
          // `set_tint(color: ▮"#FF8800")`.
          side: 2,
        }),
      );
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
