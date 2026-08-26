/**
 * Editor gutter visibility toggle (Settings + editor context menu):
 * `showGutters` on the editor slice, mirrored by `App` as the
 * `brink-gutters-hidden` root class the stylesheet acts on. Besides the
 * visual preference this is the interim WebKit latency escape hatch on
 * large projects (#3119) — hiding gutters removes their per-element
 * layout cost entirely — so the default and persistence semantics are
 * load-bearing: ON unless a persisted, explicit opt-out says otherwise.
 */

import { describe, expect, it } from "vitest";
import { createStudioStore } from "@brink/studio-store";
import {
  EDITOR_STORAGE_KEY,
  loadEditorSettings,
  saveEditorSettings,
} from "@brink/studio-ui";

function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    dump: () => Object.fromEntries(map),
  };
}

describe("gutter visibility toggle", () => {
  it("defaults ON in the store and the loader", () => {
    expect(createStudioStore().getState().showGutters).toBe(true);
    expect(loadEditorSettings(memoryStorage()).showGutters).toBe(true);
  });

  it("only a persisted explicit false hides them (lenient on garbage)", () => {
    const off = memoryStorage({
      [EDITOR_STORAGE_KEY]: JSON.stringify({ showGutters: false }),
    });
    expect(loadEditorSettings(off).showGutters).toBe(false);
    const garbage = memoryStorage({ [EDITOR_STORAGE_KEY]: "not json" });
    expect(loadEditorSettings(garbage).showGutters).toBe(true);
    const unrelated = memoryStorage({
      [EDITOR_STORAGE_KEY]: JSON.stringify({ formGlyph: "hover" }),
    });
    expect(loadEditorSettings(unrelated).showGutters).toBe(true);
  });

  it("round-trips through save/load with the other editor settings intact", () => {
    const storage = memoryStorage();
    saveEditorSettings(storage, {
      formGlyph: "hover",
      autoOpenForm: true,
      showGutters: false,
      fontSize: 14,
    });
    const loaded = loadEditorSettings(storage);
    expect(loaded).toEqual({ formGlyph: "hover", autoOpenForm: true, showGutters: false, fontSize: 14 });
  });

  it("setShowGutters drives the store flag", () => {
    const store = createStudioStore();
    store.getState().setShowGutters(false);
    expect(store.getState().showGutters).toBe(false);
    store.getState().setShowGutters(true);
    expect(store.getState().showGutters).toBe(true);
  });
});
