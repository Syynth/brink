/**
 * Player prose-size knob (W13/#3306, RULED: the reading surface's size
 * is not the UI's size). The knob drives `--bs-player-font-size` on the
 * studio root — the Player's prose reads it with the app-scale fallback,
 * so 0 (the default) changes nothing anywhere.
 */
import { describe, expect, it } from "vitest";
import { createStudioStore } from "@brink/studio-store";
import {
  loadPlayerSettings,
  savePlayerSettings,
} from "@brink/studio-ui";

function memStorage(): Storage {
  const map = new Map<string, string>();
  return {
    getItem: (k) => map.get(k) ?? null,
    setItem: (k, v) => void map.set(k, v),
  } as Storage;
}

describe("player font size (W13/#3306)", () => {
  it("clamps to the readable range; below the floor resets to follow-scale", () => {
    const store = createStudioStore();
    store.getState().setPlayerFontSize(20);
    expect(store.getState().playerFontSize).toBe(20);
    store.getState().setPlayerFontSize(99);
    expect(store.getState().playerFontSize).toBe(32);
    // Stepping down from 10 lands on the reset, never a stuck clamp.
    store.getState().setPlayerFontSize(9);
    expect(store.getState().playerFontSize).toBe(0);
  });

  it("persists alongside the paced setting and survives a reload", () => {
    const storage = memStorage();
    savePlayerSettings(storage, { pacedRevealMs: 150, fontSize: 21, saveLocation: "local", followInEditor: true, fontFamily: "", lineHeight: 0, measure: 0, showProvenance: true, showChoiceMarkers: true });
    expect(loadPlayerSettings(storage)).toEqual({ pacedRevealMs: 150, fontSize: 21, saveLocation: "local", followInEditor: true, fontFamily: "", lineHeight: 0, measure: 0, showProvenance: true, showChoiceMarkers: true });
    // Garbage → defaults (never throws).
    storage.setItem("brink-studio.player.v1", "{nope");
    expect(loadPlayerSettings(storage)).toEqual({ pacedRevealMs: 150, fontSize: 0, saveLocation: "local", followInEditor: true, fontFamily: "", lineHeight: 0, measure: 0, showProvenance: true, showChoiceMarkers: true });
  });
});
