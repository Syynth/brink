/**
 * Settings › Player › Reading + Reading aids (#3438): the knobs clamp,
 * persist, reach the Player's CSS variables, and the aids gate the
 * markup they name.
 */
import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { CURATED_FONTS, PlayerReadingSection, PlayerReadingAidsSection, StoreProvider, loadPlayerSettings } from "@brink/studio-ui";
import { createStudioStore, sanitizeFontFamily } from "@brink/studio-store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

let root: Root | null = null;
let container: HTMLDivElement | null = null;
afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  window.localStorage.clear();
});

async function mount(store = createStudioStore(), aids = false) {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root!.render(
      createElement(StoreProvider, {
        store,
        children: createElement(aids ? PlayerReadingAidsSection : PlayerReadingSection),
      }),
    );
  });
  return store;
}

describe("player reading knobs (#3438)", () => {
  it("sanitizes a font family: names, quotes and commas only", () => {
    expect(sanitizeFontFamily(" 'Iosevka Etoile', serif ")).toBe("'Iosevka Etoile', serif");
    expect(sanitizeFontFamily("Georgia; color: red")).toBe("");
    expect(sanitizeFontFamily("url(x)")).toBe("");
    expect(sanitizeFontFamily("")).toBe("");
  });

  it("clamps line spacing (1.2–2.2 in tenths) and measure (48–96 ch); 0 resets", () => {
    const store = createStudioStore();
    store.getState().setPlayerLineHeight(30);
    expect(store.getState().playerLineHeight).toBe(22);
    store.getState().setPlayerLineHeight(0);
    expect(store.getState().playerLineHeight).toBe(0);
    store.getState().setPlayerMeasure(10);
    expect(store.getState().playerMeasure).toBe(48);
    store.getState().setPlayerMeasure(200);
    expect(store.getState().playerMeasure).toBe(96);
  });

  it("the picker lists the curated faces in their own face and persists a pick", async () => {
    const store = await mount();
    const rows = Array.from(container!.querySelectorAll<HTMLButtonElement>(".font-picker-row"));
    expect(rows.length).toBe(CURATED_FONTS.length + 1);
    expect(rows[0].textContent).toContain("Theme default");
    const literata = rows.find((r) => r.textContent?.includes("Literata"))!;
    expect(literata.querySelector<HTMLElement>(".font-picker-specimen")!.style.fontFamily).toContain("Literata");
    await act(async () => {
      literata.click();
    });
    expect(store.getState().playerFontFamily).toBe("Literata, Georgia, serif");
    expect(loadPlayerSettings(window.localStorage).fontFamily).toBe("Literata, Georgia, serif");
    expect(literata.getAttribute("aria-selected")).toBe("true");
  });

  it("a host font list replaces the curated one and can be filtered", async () => {
    const store = createStudioStore();
    store.getState().setHostFonts(["Baskerville", "Menlo", "Charter"]);
    await mount(store);
    expect(container!.querySelector(".font-picker-filter")).not.toBeNull();
    const names = Array.from(container!.querySelectorAll(".font-picker-name")).map((n) => n.textContent);
    expect(names).toEqual(["Theme default", "Baskerville", "Menlo", "Charter"]);
  });

  it("a custom family is validated before it can be used", async () => {
    const store = await mount();
    const input = container!.querySelector<HTMLInputElement>(".font-picker-input")!;
    const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
    await act(async () => {
      setter.call(input, "Georgia; x");
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(input.getAttribute("aria-invalid")).toBe("true");
    const use = Array.from(container!.querySelectorAll("button")).find((b) => b.textContent === "Use") as HTMLButtonElement;
    expect(use.disabled).toBe(true);
    await act(async () => {
      setter.call(input, '"Iosevka Etoile", serif');
      input.dispatchEvent(new Event("input", { bubbles: true }));
    });
    expect(use.disabled).toBe(false);
    await act(async () => {
      use.click();
    });
    expect(store.getState().playerFontFamily).toBe('"Iosevka Etoile", serif');
  });

  it("reading aids persist", async () => {
    const store = await mount(createStudioStore(), true);
    const toggles = container!.querySelectorAll<HTMLInputElement>(".settings-toggle input");
    expect(toggles.length).toBe(2);
    await act(async () => {
      toggles[1].click();
    });
    expect(store.getState().showChoiceMarkers).toBe(false);
    expect(loadPlayerSettings(window.localStorage).showChoiceMarkers).toBe(false);
    expect(loadPlayerSettings(window.localStorage).showProvenance).toBe(true);
  });
});
