/**
 * Player prose-size knob (W13/#3306, RULED: the reading surface's size
 * is not the UI's size). The knob drives `--bs-player-font-size` on the
 * studio root — the Player's prose reads it with the app-scale fallback,
 * so 0 (the default) changes nothing anywhere.
 */
import { describe, expect, it } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { createStudioStore } from "@brink/studio-store";
import {
  SettingsStepper,
  loadPlayerSettings,
  savePlayerSettings,
} from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

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

// ── The knob as an author actually reaches it ────────────────────────
//
// The two tests above pin the store clamp and the persistence, and both
// passed for the whole time the setting did nothing: neither pressed a
// button. The knob's `min` is 0 ("follow the app scale") but its readable
// floor is 10, so ±1 from 0 produced 1, the store's own clamp read that as
// below the floor and collapsed it straight back to 0 — the stepper could
// never leave "off", and the Player's prose never changed size.
//
// Line spacing and measure carried a hop across that dead zone inline at
// their call sites; the font size never got one. `SettingsStepper`'s `floor`
// owns it now, so declaring the floor is the whole job and no future knob
// can forget the expression.

/** A live stepper over real state, driven through its own buttons. */
function stepper(opts: { start: number; min: number; max: number; floor?: number }) {
  const container = document.createElement("div");
  document.body.appendChild(container);
  const root: Root = createRoot(container);
  let value = opts.start;
  const seen: number[] = [];
  const render = (): void => {
    act(() => {
      root.render(
        createElement(SettingsStepper, {
          value,
          min: opts.min,
          max: opts.max,
          floor: opts.floor,
          label: "knob",
          onChange: (next: number) => {
            seen.push(next);
            value = next;
            render();
          },
        }),
      );
    });
  };
  render();
  const press = (which: "Increase" | "Decrease"): void => {
    const btn = container.querySelector<HTMLButtonElement>(
      `button[aria-label="${which} knob"]`,
    );
    if (btn === null || btn.disabled) return;
    act(() => btn.click());
  };
  return {
    up: () => press("Increase"),
    down: () => press("Decrease"),
    get value() {
      return value;
    },
    seen,
    done: () => {
      act(() => root.unmount());
      container.remove();
    },
  };
}

describe("the player font-size stepper (the bug: it did nothing)", () => {
  it("steps up out of off and lands on the readable floor", () => {
    const s = stepper({ start: 0, min: 0, max: 32, floor: 10 });
    s.up();
    // Before the fix this emitted 1, which the store clamped back to 0.
    expect(s.seen).toEqual([10]);
    expect(s.value).toBe(10);
    s.done();
  });

  it("keeps stepping normally once above the floor, up to the ceiling", () => {
    const s = stepper({ start: 0, min: 0, max: 32, floor: 10 });
    s.up(); // 10
    s.up(); // 11
    s.up(); // 12
    expect(s.value).toBe(12);
    for (let i = 0; i < 40; i++) s.up();
    expect(s.value).toBe(32); // the ceiling holds, and the button disables
    s.done();
  });

  it("steps down off the floor back to off, not into the dead zone", () => {
    const s = stepper({ start: 10, min: 0, max: 32, floor: 10 });
    s.down();
    expect(s.value).toBe(0);
    s.done();
  });

  it("survives a round trip: off → floor → off", () => {
    const s = stepper({ start: 0, min: 0, max: 32, floor: 10 });
    s.up();
    s.down();
    expect(s.value).toBe(0);
    expect(s.seen).toEqual([10, 0]);
    s.done();
  });

  it("a stepper with no floor is unchanged — plain ±1", () => {
    // The editor/app font size and indent knobs have a real `min`, no dead
    // zone, and must keep stepping one at a time.
    const s = stepper({ start: 14, min: 8, max: 32 });
    s.up();
    expect(s.value).toBe(15);
    s.down();
    s.down();
    expect(s.value).toBe(13);
    s.done();
  });

  it("the floor hop matches what line spacing and measure did inline", () => {
    // Those two call sites carried `v > 0 && v < F ? (cur === 0 ? F : 0) : v`.
    // Same behaviour, now from the component.
    for (const [floor, max] of [
      [12, 22],
      [48, 96],
    ] as const) {
      const s = stepper({ start: 0, min: 0, max, floor });
      s.up();
      expect(s.value).toBe(floor);
      s.down();
      expect(s.value).toBe(0);
      s.done();
    }
  });

  it("the store accepts what the stepper now emits", () => {
    // The two halves meeting: the value the button produces must survive
    // the clamp that used to eat it.
    const store = createStudioStore();
    const s = stepper({ start: store.getState().playerFontSize, min: 0, max: 32, floor: 10 });
    expect(s.value).toBe(0); // the default is "follow the app scale"
    s.up();
    act(() => store.getState().setPlayerFontSize(s.value));
    expect(store.getState().playerFontSize).toBe(10);
    s.done();
  });
});
