/**
 * Inlay hints on/off toggle (#3350, Settings ▸ Editor "Show inlay hints"):
 * an app-scope preference, mirroring `showGutters`/`formGlyph`'s persistence
 * shape, and broadcast live to every open editor through
 * `DocumentSessions.setInlayHints` — the same `_documents?.setXxx(...)`
 * shape `setFormGlyph`/`setAutoOpenForm` already use. Default ON (current
 * behavior; the issue's ruled default).
 */

import { describe, expect, it, vi } from "vitest";
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

describe("inlay hints visibility toggle", () => {
  it("defaults ON in the store and the loader", () => {
    expect(createStudioStore().getState().showInlayHints).toBe(true);
    expect(loadEditorSettings(memoryStorage()).showInlayHints).toBe(true);
  });

  it("only a persisted explicit false hides them (lenient on garbage)", () => {
    const off = memoryStorage({
      [EDITOR_STORAGE_KEY]: JSON.stringify({ showInlayHints: false }),
    });
    expect(loadEditorSettings(off).showInlayHints).toBe(false);
    const garbage = memoryStorage({ [EDITOR_STORAGE_KEY]: "not json" });
    expect(loadEditorSettings(garbage).showInlayHints).toBe(true);
    const preExisting = memoryStorage({
      // Settings written before this field shipped: absent, not false.
      [EDITOR_STORAGE_KEY]: JSON.stringify({ formGlyph: "hover" }),
    });
    expect(loadEditorSettings(preExisting).showInlayHints).toBe(true);
  });

  it("round-trips through save/load with the other editor settings intact", () => {
    const storage = memoryStorage();
    saveEditorSettings(storage, {
      formGlyph: "off",
      autoOpenForm: false,
      showGutters: true,
      showInlayHints: false,
      fontSize: 14,
      appFontSize: 12,
    });
    const loaded = loadEditorSettings(storage);
    expect(loaded).toEqual({
      formGlyph: "off",
      autoOpenForm: false,
      showGutters: true,
      showInlayHints: false,
      fontSize: 14,
      appFontSize: 12,
    });
  });

  it("setShowInlayHints drives the store flag", () => {
    const store = createStudioStore();
    store.getState().setShowInlayHints(false);
    expect(store.getState().showInlayHints).toBe(false);
    store.getState().setShowInlayHints(true);
    expect(store.getState().showInlayHints).toBe(true);
  });

  it("broadcasts live to every open editor via DocumentSessions.setInlayHints", () => {
    // This is the regression the fix is FOR: without wiring the store
    // action to `_documents`, the toggle would only affect newly-opened
    // editors (or nothing at all) rather than the ones already on screen.
    const setInlayHints = vi.fn();
    const store = createStudioStore();
    store.setState({ _documents: { setInlayHints } as never });

    store.getState().setShowInlayHints(false);
    expect(setInlayHints).toHaveBeenCalledWith(false);

    store.getState().setShowInlayHints(true);
    expect(setInlayHints).toHaveBeenCalledWith(true);
    expect(setInlayHints).toHaveBeenCalledTimes(2);
  });

  it("does not throw when no project is open yet (_documents is null)", () => {
    const store = createStudioStore();
    expect(() => store.getState().setShowInlayHints(false)).not.toThrow();
  });
});
