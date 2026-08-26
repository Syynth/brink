/**
 * Editor font size (beta feedback 2026-08-25, ruled: TWO knobs — this is
 * the editor one; the app-wide knob is separate and not implemented here).
 *
 * The size is one number in three places that must never disagree: the
 * store (live), the persisted settings record (survives restart), and the
 * CSS custom property the CM6 theme reads. `clampEditorFontSize` is the
 * single definition of "usable", shared by the store's setters and the
 * settings parser — these tests pin that sharing, since two clamps drifting
 * apart is exactly how a persisted 200px editor happens.
 */

import { describe, expect, it } from "vitest";
import {
  DEFAULT_EDITOR_FONT_SIZE,
  MAX_EDITOR_FONT_SIZE,
  MIN_EDITOR_FONT_SIZE,
  clampEditorFontSize,
} from "@brink-lang/editor";
import { createStudioStore } from "@brink/studio-store";
import { loadEditorSettings, saveEditorSettings } from "@brink/studio-ui";

function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
    clear: () => map.clear(),
    key: () => null,
    length: 0,
  } as unknown as Storage;
}

describe("clampEditorFontSize", () => {
  it("passes usable sizes through, rounding fractions", () => {
    expect(clampEditorFontSize(18)).toBe(18);
    expect(clampEditorFontSize(15.4)).toBe(15);
    expect(clampEditorFontSize(15.6)).toBe(16);
  });

  it("clamps to the usable range rather than rejecting", () => {
    expect(clampEditorFontSize(2)).toBe(MIN_EDITOR_FONT_SIZE);
    expect(clampEditorFontSize(400)).toBe(MAX_EDITOR_FONT_SIZE);
  });

  it("falls back to the default on garbage (persisted or typed)", () => {
    for (const junk of [undefined, null, "16", NaN, Infinity, {}, []]) {
      expect(clampEditorFontSize(junk)).toBe(DEFAULT_EDITOR_FONT_SIZE);
    }
  });

  it("keeps the default inside its own bounds", () => {
    expect(DEFAULT_EDITOR_FONT_SIZE).toBeGreaterThanOrEqual(MIN_EDITOR_FONT_SIZE);
    expect(DEFAULT_EDITOR_FONT_SIZE).toBeLessThanOrEqual(MAX_EDITOR_FONT_SIZE);
  });
});

describe("editor font size in the store", () => {
  it("starts at the shipped default", () => {
    expect(createStudioStore().getState().editorFontSize).toBe(DEFAULT_EDITOR_FONT_SIZE);
  });

  it("sets an absolute size, clamped", () => {
    const store = createStudioStore();
    store.getState().setEditorFontSize(20);
    expect(store.getState().editorFontSize).toBe(20);
    store.getState().setEditorFontSize(9999);
    expect(store.getState().editorFontSize).toBe(MAX_EDITOR_FONT_SIZE);
  });

  it("steps by a delta and stops at the bounds instead of running away", () => {
    const store = createStudioStore();
    store.getState().adjustEditorFontSize(2);
    expect(store.getState().editorFontSize).toBe(DEFAULT_EDITOR_FONT_SIZE + 2);

    // Hold the shrink chord down: it parks at the floor, never goes 0 or
    // negative (which would render an invisible editor with no way back).
    for (let i = 0; i < 50; i++) store.getState().adjustEditorFontSize(-1);
    expect(store.getState().editorFontSize).toBe(MIN_EDITOR_FONT_SIZE);

    for (let i = 0; i < 100; i++) store.getState().adjustEditorFontSize(1);
    expect(store.getState().editorFontSize).toBe(MAX_EDITOR_FONT_SIZE);
  });
});

describe("editor font size persistence", () => {
  it("round-trips with the other editor settings", () => {
    const storage = memoryStorage();
    saveEditorSettings(storage, {
      formGlyph: "inline",
      autoOpenForm: true,
      showGutters: false,
      fontSize: 18,
      appFontSize: 12,
    });
    const loaded = loadEditorSettings(storage);
    expect(loaded.fontSize).toBe(18);
    // The neighbours are untouched — the new field must not disturb them.
    expect(loaded.formGlyph).toBe("inline");
    expect(loaded.autoOpenForm).toBe(true);
    expect(loaded.showGutters).toBe(false);
  });

  it("defaults when the field is absent (settings written before this shipped)", () => {
    const storage = memoryStorage();
    storage.setItem(
      "brink-studio.editor.v1",
      JSON.stringify({ formGlyph: "hover", autoOpenForm: false, showGutters: true }),
    );
    expect(loadEditorSettings(storage).fontSize).toBe(DEFAULT_EDITOR_FONT_SIZE);
  });

  it("repairs an out-of-range or garbage persisted size", () => {
    const storage = memoryStorage();
    storage.setItem("brink-studio.editor.v1", JSON.stringify({ fontSize: 999 }));
    expect(loadEditorSettings(storage).fontSize).toBe(MAX_EDITOR_FONT_SIZE);
    storage.setItem("brink-studio.editor.v1", JSON.stringify({ fontSize: "big" }));
    expect(loadEditorSettings(storage).fontSize).toBe(DEFAULT_EDITOR_FONT_SIZE);
  });
});
