/**
 * The argument Form (argument-widget spec §1.1–1.2, stage 3). A studio-composed
 * form with one field per parameter — a built-in widget editor (the `color`
 * field embeds the picker) or a plain text input — opened in the studio popover
 * chrome. On Apply it hands the caller one literal per field; the caller writes
 * them (replace an existing call's args, or insert a completed call at the
 * cursor). The Form never formats the call wrapper itself.
 *
 * Reachable from the in-editor call glyph AND the Host Functions panel — both
 * call `openArgumentForm`, differing only in what they do with the result.
 */

import { openPopover, type PopoverHandle } from "./widget-popover.js";
import { mountColorPicker, hexToRgb, rgbToHex } from "./color-picker-ui.js";

export interface FormField {
  paramName: string;
  /** Semantic-type name, shown in the label (e.g. `hex_color`). */
  typeName?: string;
  /** Built-in widget kind (`color`, …); a text input when absent. */
  widgetKind?: string;
  /** Current value — a quotes-stripped value for widget fields, the raw arg
   *  expression for text fields. */
  initial?: string;
}

export interface ArgumentFormOptions {
  /** Heading, e.g. the call signature. */
  title?: string;
  fields: FormField[];
  /** The Apply button label (e.g. "Insert" from the panel, "Apply" in-editor). */
  applyLabel?: string;
  /** One literal per field, in order — ready to join with `, `. */
  onApply: (literals: string[]) => void;
  onCancel: () => void;
}

function toDisplayHex(value: string): string {
  const rgb = hexToRgb(value);
  return rgb ? rgbToHex(rgb) : "#000000";
}

/** Open the form anchored to `anchor`. Returns a teardown that closes it. */
export function openArgumentForm(anchor: HTMLElement, opts: ArgumentFormOptions): () => void {
  let popover: PopoverHandle | null = null;
  let applied = false;

  const render = (container: HTMLElement): void => {
    const root = document.createElement("div");
    root.className = "brink-arg-form";

    if (opts.title) {
      const h = document.createElement("div");
      h.className = "brink-arg-form-title";
      h.textContent = opts.title;
      root.appendChild(h);
    }

    // Each field contributes a getter for its literal.
    const getters: (() => string)[] = [];

    for (const field of opts.fields) {
      const row = document.createElement("div");
      row.className = "brink-arg-form-row";

      const label = document.createElement("span");
      label.className = "brink-arg-form-label";
      label.textContent = field.typeName
        ? `${field.paramName}: ${field.typeName}`
        : field.paramName;
      row.appendChild(label);

      if (field.widgetKind === "color") {
        getters.push(buildColorField(row, field));
      } else {
        getters.push(buildTextField(row, field));
      }
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
      opts.onApply(getters.map((g) => g()));
      popover?.close();
    });
    buttons.append(cancel, apply);
    root.appendChild(buttons);

    container.appendChild(root);
  };

  popover = openPopover(anchor, render, () => {
    if (!applied) opts.onCancel();
  });

  return () => popover?.close();
}

/** A color field: swatch + hex input + a toggleable inline picker. Returns a
 *  getter for the `"#RRGGBB"` literal. */
function buildColorField(row: HTMLElement, field: FormField): () => string {
  let hex = field.initial ? toDisplayHex(field.initial) : "#FF8800";

  const control = document.createElement("div");
  control.className = "brink-arg-form-control";

  const swatch = document.createElement("button");
  swatch.type = "button";
  swatch.className = "brink-color-swatch";
  swatch.style.background = hex;
  swatch.title = "Toggle picker";

  const hexInput = document.createElement("input");
  hexInput.type = "text";
  hexInput.className = "brink-cp-hex";
  hexInput.spellcheck = false;
  hexInput.value = hex;

  const pickerWrap = document.createElement("div");
  pickerWrap.className = "brink-arg-form-picker";
  pickerWrap.style.display = "none";
  let picker: { destroy(): void } | null = null;

  swatch.addEventListener("click", () => {
    if (picker) {
      picker.destroy();
      picker = null;
      pickerWrap.style.display = "none";
    } else {
      pickerWrap.style.display = "block";
      picker = mountColorPicker(pickerWrap, hex, (h) => {
        hex = h;
        swatch.style.background = h;
        hexInput.value = h;
      });
    }
  });

  hexInput.addEventListener("input", () => {
    const rgb = hexToRgb(hexInput.value);
    if (rgb) {
      hex = rgbToHex(rgb);
      swatch.style.background = hex;
    }
  });

  control.append(swatch, hexInput);
  row.append(control, pickerWrap);
  return () => `"${hex}"`;
}

/** A plain text field — the author types the raw arg expression. Empty falls
 *  back to the param name (a placeholder, like the old skeleton insert). */
function buildTextField(row: HTMLElement, field: FormField): () => string {
  const input = document.createElement("input");
  input.type = "text";
  input.className = "brink-arg-form-input";
  input.spellcheck = false;
  input.value = field.initial ?? "";
  input.placeholder = field.typeName ?? "value";
  row.appendChild(input);
  return () => input.value.trim() || field.paramName;
}
