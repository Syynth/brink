/**
 * Built-in color picker for `hex_color` arguments (#174).
 *
 * For every `EXTERNAL` call argument whose semantic type is `hex_color`, the
 * brink-ide `color_hints` query returns the literal's span + hex value. This
 * extension renders a native `<input type="color">` swatch just before each
 * such literal; picking a color rewrites the literal in place. Studio-builtin —
 * no host involvement (unlike the host-provided value pickers).
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

/** Coerce a stored value to a `#rrggbb` an `<input type=color>` accepts. */
function toInputHex(value: string): string {
  const v = value.trim().replace(/^#?/, "");
  if (/^[0-9a-fA-F]{6}$/.test(v)) return `#${v.toLowerCase()}`;
  if (/^[0-9a-fA-F]{3}$/.test(v)) {
    return `#${v.split("").map((c) => c + c).join("").toLowerCase()}`;
  }
  return "#000000";
}

class ColorSwatchWidget extends WidgetType {
  constructor(
    readonly value: string,
    readonly from: number,
    readonly to: number,
    readonly view: EditorView,
  ) {
    super();
  }

  eq(other: ColorSwatchWidget): boolean {
    return other.value === this.value && other.from === this.from && other.to === this.to;
  }

  toDOM(): HTMLElement {
    const input = document.createElement("input");
    input.type = "color";
    input.className = "brink-color-swatch";
    input.value = toInputHex(this.value);
    input.title = `Pick color (${this.value})`;
    // `input` fires live as the user drags the picker; rewrite the literal,
    // preserving the surrounding quotes.
    input.addEventListener("input", () => {
      const hex = input.value.toUpperCase();
      this.view.dispatch({ changes: { from: this.from, to: this.to, insert: `"${hex}"` } });
    });
    return input;
  }

  ignoreEvent(): boolean {
    return true; // let the native input handle its own pointer/keyboard events
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
          widget: new ColorSwatchWidget(h.value, h.start, h.end, view),
          side: -1,
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
