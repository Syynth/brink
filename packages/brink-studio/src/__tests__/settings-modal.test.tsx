/**
 * Settings as a modal with a section rail (#3174).
 *
 * The sections' own contents are covered by their own tests; this is about
 * the shell — which door lands where, that the rail filters by what a
 * section is ABOUT, and that `null` means closed.
 */
import { describe, it, expect, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { SettingsModal, StoreProvider, type SettingsSection } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const SECTIONS: SettingsSection[] = [
  {
    id: "project",
    title: "Project",
    keywords: "brink.toml entry drafts",
    icon: null,
    body: createElement("p", null, "project body"),
  },
  {
    id: "diagnostics",
    title: "Diagnostics",
    keywords: "lints todo suppress",
    icon: null,
    body: createElement("p", null, "diagnostics body"),
  },
  {
    id: "appearance",
    title: "Appearance",
    keywords: "theme colour",
    icon: null,
    body: createElement("p", null, "appearance body"),
  },
];

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(section: string | null) {
  const store = createStudioStore();
  store.getState().setSettingsSection(section);
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(
      createElement(StoreProvider, {
        store,
        children: createElement(SettingsModal, { sections: SECTIONS }),
      }),
    );
  });
  return store;
}

const railLabels = (): string[] =>
  [...container!.querySelectorAll(".brink-settings-nav-item")].map((b) => b.textContent ?? "");
const title = (): string | undefined =>
  container!.querySelector(".brink-settings-head h2")?.textContent ?? undefined;
const type = (value: string): void => {
  const input = container!.querySelector<HTMLInputElement>(".brink-settings-search");
  act(() => {
    const setter = Object.getOwnPropertyDescriptor(
      window.HTMLInputElement.prototype,
      "value",
    )!.set!;
    setter.call(input, value);
    input!.dispatchEvent(new Event("input", { bubbles: true }));
  });
};

describe("null means closed", () => {
  it("renders nothing when the section is null", () => {
    // The first version used null for BOTH "closed" and "open at the
    // default section", so the palette command opened Settings and
    // immediately closed it. Every door now names a section.
    mount(null);
    expect(container!.querySelector(".brink-settings-modal")).toBeNull();
  });

  it("opens at the section it is given", () => {
    mount("diagnostics");
    expect(title()).toBe("Diagnostics");
    expect(container!.textContent).toContain("diagnostics body");
  });

  it("closing sets it back to null", () => {
    const store = mount("project");
    act(() => {
      container!.querySelector<HTMLButtonElement>(".brink-settings-close")?.click();
    });
    expect(store.getState().settingsSection).toBeNull();
  });
});

describe("the rail", () => {
  it("lists every section and marks the active one", () => {
    mount("diagnostics");
    expect(railLabels()).toEqual(["Project", "Diagnostics", "Appearance"]);
    const active = container!.querySelector(".brink-settings-nav-item.active");
    expect(active?.textContent).toBe("Diagnostics");
  });

  it("switches the pane without closing", () => {
    const store = mount("project");
    act(() => {
      [...container!.querySelectorAll<HTMLButtonElement>(".brink-settings-nav-item")]
        .find((b) => b.textContent === "Appearance")
        ?.click();
    });
    expect(title()).toBe("Appearance");
    expect(store.getState().settingsSection).toBe("appearance");
  });

  it("falls back rather than rendering an empty pane for an unknown id", () => {
    // A caller naming a section that isn't registered, or one removed while
    // open. Showing the first section beats showing a blank dialog.
    mount("no-such-section");
    expect(title()).toBe("Project");
  });
});

describe("search", () => {
  it("filters by what a section is ABOUT, not only its name", () => {
    // "todo" is nowhere in the word "Diagnostics" — the keywords are what
    // make a section findable by the thing you actually want to change.
    mount("project");
    type("todo");
    expect(railLabels()).toEqual(["Diagnostics"]);
  });

  it("matches the title too", () => {
    mount("project");
    type("appear");
    expect(railLabels()).toEqual(["Appearance"]);
  });

  it("does not move the selection", () => {
    // Filtering must not navigate: a search that jumped would lose the
    // section you were reading the moment you typed.
    mount("project");
    type("todo");
    expect(title()).toBe("Project");
  });

  it("says so when nothing matches", () => {
    mount("project");
    type("zzz");
    expect(railLabels()).toEqual([]);
    expect(container!.querySelector(".brink-settings-nav-empty")).not.toBeNull();
  });
});
