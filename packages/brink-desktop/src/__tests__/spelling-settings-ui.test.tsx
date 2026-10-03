// @vitest-environment jsdom
/**
 * Settings ▸ Spelling, the pane.
 *
 * What a model test cannot reach: that the switch never claims a checker
 * that is not running, and that toggling asks for a fresh compile — which
 * is the only thing that refreshes squiggles already on screen.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { SpellingSettings, type SpellingSettingsApi } from "../SpellingSettings.js";

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

async function mount(over: Partial<SpellingSettingsApi> & { useSystem?: () => boolean } = {}) {
  const setUseSystem = vi.fn();
  const api: SpellingSettingsApi = {
    useSystem: () => true,
    setUseSystem,
    probeAvailable: () => Promise.resolve(true),
    ...over,
  };
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root!.render(createElement(SpellingSettings, { api }));
  });
  return { setUseSystem };
}

function toggle(): HTMLInputElement {
  const el = container!.querySelector<HTMLInputElement>('input[type="checkbox"]');
  expect(el, "no toggle rendered").not.toBeNull();
  return el!;
}

describe("where the platform has a checker", () => {
  it("reflects the stored preference", async () => {
    await mount({ useSystem: () => false });
    expect(toggle().checked).toBe(false);
    expect(container!.textContent).toContain("built-in checker handles spelling");
  });

  it("persists the change and asks for a fresh compile", async () => {
    // The write alone changes nothing on screen: squiggles are refreshed by
    // `refreshProseEffect`, which a compile dispatches. Without that, the
    // switch looks broken until the next edit.
    const { setUseSystem } = await mount({ useSystem: () => true });
    await act(async () => {
      toggle().click();
    });
    expect(setUseSystem).toHaveBeenCalledWith(false);
  });
});

describe("where the platform has none", () => {
  it("reads off even when the stored preference says on", async () => {
    // Harper is what is actually running there. A switch showing "on" over
    // a checker nobody consults is the lie the settings model exists to
    // avoid.
    await mount({ useSystem: () => true, probeAvailable: () => Promise.resolve(false) });
    expect(toggle().checked).toBe(false);
    expect(container!.textContent).toContain("no system spell checker");
  });

  it("is inert rather than writing a preference that changes nothing", async () => {
    const { setUseSystem } = await mount({
      useSystem: () => true,
      probeAvailable: () => Promise.resolve(false),
    });
    await act(async () => {
      toggle().click();
    });
    expect(setUseSystem).not.toHaveBeenCalled();
  });

  it("keeps the preference for machines that do have one", async () => {
    // Stated in the row, so an author who toggles on a Linux box and later
    // opens the same account on a Mac is not surprised.
    await mount({ useSystem: () => true, probeAvailable: () => Promise.resolve(false) });
    expect(container!.textContent).toContain("kept for machines that do");
  });
});
