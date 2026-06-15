/**
 * The built-in `color` widget (argument-widget-spec §7) — the first widget
 * through the registry, for `hex_color` arguments. Inline: a studio-drawn
 * swatch. Editor: a light popover HSV picker (color-picker-ui), in the
 * studio-owned popover chrome (widget-popover) — replacing the OS-native
 * `<input type=color>` dialog.
 */

import { openPopover, type PopoverHandle } from "./widget-popover.js";
import { mountColorPicker, hexToRgb, rgbToHex } from "./color-picker-ui.js";
import { registerBuiltinWidget, type BuiltinWidget } from "./widget-registry.js";

/** Normalize a stored value (`#fff`, `00ff00`, …) to a CSS `#RRGGBB`. */
export function toDisplayHex(value: string): string {
  const rgb = hexToRgb(value);
  return rgb ? rgbToHex(rgb) : "#000000";
}

// At most one editor popover open at a time across all swatches.
let activePopover: PopoverHandle | null = null;

export const colorWidget: BuiltinWidget = {
  kind: "color",

  renderInline(value: string): HTMLElement {
    const swatch = document.createElement("span");
    swatch.className = "brink-color-swatch";
    swatch.style.background = toDisplayHex(value);
    swatch.setAttribute("role", "button");
    swatch.tabIndex = 0;
    swatch.title = `Edit color (${value})`;
    return swatch;
  },

  openEditor(anchor, host): () => void {
    activePopover?.close();
    const popover = openPopover(
      anchor,
      (container) => {
        mountColorPicker(container, host.initial, (hex) => host.resolve(hex));
      },
      () => {
        activePopover = null;
        host.cancel();
      },
    );
    activePopover = popover;
    return () => popover.close();
  },
};

registerBuiltinWidget(colorWidget);
