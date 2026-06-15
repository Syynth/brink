/**
 * A compact HSV color picker rendered into a container — a saturation/value
 * square, a hue slider, a hex field, and preset swatches. No OS-native dialog
 * (argument-widget-spec §9: "a light studio popover, not the OS-native dialog").
 * Pure DOM so it can mount inside the CodeMirror-anchored popover.
 */

interface Rgb {
  r: number;
  g: number;
  b: number;
}
interface Hsv {
  h: number; // 0..360
  s: number; // 0..1
  v: number; // 0..1
}

const clamp = (n: number, lo: number, hi: number): number => Math.min(hi, Math.max(lo, n));

/** Parse `#rgb` / `#rrggbb` (with or without `#`) to RGB, or null. */
export function hexToRgb(hex: string): Rgb | null {
  const v = hex.trim().replace(/^#/, "");
  if (/^[0-9a-fA-F]{3}$/.test(v)) {
    return {
      r: parseInt(v[0] + v[0], 16),
      g: parseInt(v[1] + v[1], 16),
      b: parseInt(v[2] + v[2], 16),
    };
  }
  if (/^[0-9a-fA-F]{6}$/.test(v)) {
    return {
      r: parseInt(v.slice(0, 2), 16),
      g: parseInt(v.slice(2, 4), 16),
      b: parseInt(v.slice(4, 6), 16),
    };
  }
  return null;
}

/** Format RGB as an uppercase `#RRGGBB`. */
export function rgbToHex({ r, g, b }: Rgb): string {
  const part = (n: number): string => Math.round(clamp(n, 0, 255)).toString(16).padStart(2, "0");
  return `#${part(r)}${part(g)}${part(b)}`.toUpperCase();
}

function rgbToHsv({ r, g, b }: Rgb): Hsv {
  const rn = r / 255;
  const gn = g / 255;
  const bn = b / 255;
  const max = Math.max(rn, gn, bn);
  const min = Math.min(rn, gn, bn);
  const d = max - min;
  let h = 0;
  if (d !== 0) {
    if (max === rn) h = ((gn - bn) / d) % 6;
    else if (max === gn) h = (bn - rn) / d + 2;
    else h = (rn - gn) / d + 4;
    h *= 60;
    if (h < 0) h += 360;
  }
  return { h, s: max === 0 ? 0 : d / max, v: max };
}

function hsvToRgb({ h, s, v }: Hsv): Rgb {
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  let r = 0;
  let g = 0;
  let b = 0;
  if (h < 60) {
    r = c;
    g = x;
  } else if (h < 120) {
    r = x;
    g = c;
  } else if (h < 180) {
    g = c;
    b = x;
  } else if (h < 240) {
    g = x;
    b = c;
  } else if (h < 300) {
    r = x;
    b = c;
  } else {
    r = c;
    b = x;
  }
  return { r: (r + m) * 255, g: (g + m) * 255, b: (b + m) * 255 };
}

const PRESETS = [
  "#FF3B30",
  "#FF8800",
  "#FFD60A",
  "#34C759",
  "#00C7BE",
  "#0A84FF",
  "#5E5CE6",
  "#BF5AF2",
  "#FFFFFF",
  "#8E8E93",
  "#000000",
];

export interface ColorPickerUi {
  destroy(): void;
}

/**
 * Mount the picker into `container`, seeded from `initialHex`. `onChange` fires
 * live (on drag / input) with an uppercase `#RRGGBB`.
 */
export function mountColorPicker(
  container: HTMLElement,
  initialHex: string,
  onChange: (hex: string) => void,
): ColorPickerUi {
  let hsv = rgbToHsv(hexToRgb(initialHex) ?? { r: 0, g: 0, b: 0 });

  const root = document.createElement("div");
  root.className = "brink-cp";

  const sv = document.createElement("div");
  sv.className = "brink-cp-sv";
  const svThumb = document.createElement("div");
  svThumb.className = "brink-cp-sv-thumb";
  sv.appendChild(svThumb);

  const hue = document.createElement("input");
  hue.type = "range";
  hue.min = "0";
  hue.max = "360";
  hue.step = "1";
  hue.className = "brink-cp-hue";

  const row = document.createElement("div");
  row.className = "brink-cp-row";
  const hexInput = document.createElement("input");
  hexInput.type = "text";
  hexInput.className = "brink-cp-hex";
  hexInput.spellcheck = false;
  hexInput.setAttribute("aria-label", "Hex color");
  row.appendChild(hexInput);

  const presets = document.createElement("div");
  presets.className = "brink-cp-presets";
  for (const p of PRESETS) {
    const sw = document.createElement("button");
    sw.type = "button";
    sw.className = "brink-cp-preset";
    sw.style.background = p;
    sw.title = p;
    sw.addEventListener("click", () => {
      hsv = rgbToHsv(hexToRgb(p) ?? { r: 0, g: 0, b: 0 });
      syncFromHsv(true, true);
    });
    presets.appendChild(sw);
  }

  root.append(sv, hue, row, presets);
  container.appendChild(root);

  const currentHex = (): string => rgbToHex(hsvToRgb(hsv));

  // Refresh the UI from the model. `editText` rewrites the hex field (skip while
  // typing into it); `emit` calls `onChange`. The initial seed updates the UI
  // WITHOUT emitting — so opening the picker doesn't commit a value (Fill stays
  // empty until the user actually picks; Escape leaves the slot untouched).
  function syncFromHsv(editText: boolean, emit: boolean): void {
    const hex = currentHex();
    sv.style.background = `linear-gradient(to top, #000, rgba(0,0,0,0)), linear-gradient(to right, #fff, hsl(${hsv.h} 100% 50%)), #fff`;
    svThumb.style.left = `${hsv.s * 100}%`;
    svThumb.style.top = `${(1 - hsv.v) * 100}%`;
    svThumb.style.background = hex;
    hue.value = String(Math.round(hsv.h));
    if (editText) hexInput.value = hex;
    if (emit) onChange(hex);
  }

  const onSvPointer = (e: PointerEvent): void => {
    const rect = sv.getBoundingClientRect();
    hsv = {
      ...hsv,
      s: clamp((e.clientX - rect.left) / rect.width, 0, 1),
      v: clamp(1 - (e.clientY - rect.top) / rect.height, 0, 1),
    };
    syncFromHsv(true, true);
  };
  sv.addEventListener("pointerdown", (e) => {
    sv.setPointerCapture(e.pointerId);
    onSvPointer(e);
  });
  sv.addEventListener("pointermove", (e) => {
    if (sv.hasPointerCapture(e.pointerId)) onSvPointer(e);
  });

  hue.addEventListener("input", () => {
    hsv = { ...hsv, h: Number(hue.value) };
    syncFromHsv(true, true);
  });

  hexInput.addEventListener("input", () => {
    const rgb = hexToRgb(hexInput.value);
    if (rgb) {
      hsv = rgbToHsv(rgb);
      syncFromHsv(false, true);
    }
  });

  syncFromHsv(true, false);

  return {
    destroy(): void {
      root.remove();
    },
  };
}
