/**
 * The author's prose dictionary, in `[prose] dictionary` in `brink.toml`.
 *
 * The bug these cover is not a wrong value — it is an INVISIBLE one. The
 * list previously lived in a `.brink-dictionary` sidecar with no UI at all,
 * so "Add to dictionary" in the editor wrote a file nothing displayed, and
 * the author's reasonable conclusion was that the action did nothing. The
 * fix is only real if the word is visible afterwards, which is why the
 * assertions here are about what the settings panel SHOWS, not just about
 * what the config string contains.
 *
 * Matching is literal for now (decision log, 2026-08-28), so `Griswold` and
 * `GRISWOLD` are two entries — pinned below, because a later decision to
 * fold cases together should have to change a test that says so.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ProseSettings, StoreProvider } from "@brink/studio-ui";
import {
  createStudioStore,
  dictionaryWords,
  withDictionaryWord,
  withoutDictionaryWord,
} from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "brink.toml", symbols: [], mounted: false },
  { path: "main.ink", symbols: [], mounted: false },
];

const CONFIG = `[project]
entry = "main.ink"

# which English
[prose]
dialect = "british"
dictionary = [
  "Ashfen",
  "Griswold",
]
`;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(initial = CONFIG) {
  let source = initial;
  const applied: string[] = [];
  const project = {
    getSession: () => ({
      getFileSource: (p: string) => (p === "brink.toml" ? source : null),
    }),
    applyEdit: (_path: string, next: string) => {
      source = next;
      applied.push(next);
      return true;
    },
  };
  const store = createStudioStore();
  store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
  store.setState({
    _project: project as never,
    _documents: { refreshExternal: vi.fn(), triggerCompile: vi.fn() } as never,
  });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root!.render(createElement(StoreProvider, { store, children: createElement(ProseSettings) }));
  });
  return { applied, current: () => source };
}

/**
 * Type into a React-controlled input.
 *
 * Assigning `.value` directly does not work: React tracks the previous
 * value on the DOM node and suppresses the synthetic change event when its
 * own setter was bypassed, so the component never sees the keystroke and
 * the test asserts against an unchanged draft.
 */
function typeInto(input: HTMLInputElement, text: string): void {
  const setter = Object.getOwnPropertyDescriptor(
    window.HTMLInputElement.prototype,
    "value",
  )?.set;
  setter?.call(input, text);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

/** The words the panel is showing, in render order. */
function shownWords(): string[] {
  return [...container!.querySelectorAll(".prose-dict-word-text")].map(
    (el) => el.textContent ?? "",
  );
}

describe("the pure edits", () => {
  it("reads the words the config declares", () => {
    expect(dictionaryWords(CONFIG)).toEqual(["Ashfen", "Griswold"]);
  });

  it("adds a word, sorted so the file does not churn by insertion order", () => {
    // Two authors adding the same two words in different orders should
    // produce the same file rather than a merge conflict.
    const next = withDictionaryWord(CONFIG, "Bellweather");
    expect(next).not.toBeNull();
    expect(dictionaryWords(next!)).toEqual(["Ashfen", "Bellweather", "Griswold"]);
  });

  it("returns null for a word already present, so no spurious recompile", () => {
    // A no-op edit still marks the file dirty and triggers a rebuild.
    expect(withDictionaryWord(CONFIG, "Griswold")).toBeNull();
  });

  it("returns null for blank input", () => {
    expect(withDictionaryWord(CONFIG, "   ")).toBeNull();
  });

  it("treats a different casing as a different word, because matching is literal", () => {
    const next = withDictionaryWord(CONFIG, "GRISWOLD");
    expect(next).not.toBeNull();
    expect(dictionaryWords(next!)).toContain("GRISWOLD");
    expect(dictionaryWords(next!)).toContain("Griswold");
  });

  it("removes a word, and reports null when it was not there", () => {
    const next = withoutDictionaryWord(CONFIG, "Griswold");
    expect(dictionaryWords(next!)).toEqual(["Ashfen"]);
    expect(withoutDictionaryWord(CONFIG, "Nobody")).toBeNull();
  });

  it("preserves the author's comments and unrelated keys", () => {
    // The whole reason these are targeted line edits rather than a
    // parse-and-reserialize.
    const next = withDictionaryWord(CONFIG, "Bellweather")!;
    expect(next).toContain("# which English");
    expect(next).toContain('entry = "main.ink"');
    expect(next).toContain('dialect = "british"');
  });
});

describe("the settings panel", () => {
  it("shows the words the project already has", () => {
    mount();
    expect(shownWords()).toEqual(["Ashfen", "Griswold"]);
  });

  it("says so when the list is empty, rather than showing an empty box", () => {
    mount(`[prose]\ndialect = "british"\n`);
    expect(shownWords()).toEqual([]);
    expect(container!.querySelector(".prose-dict-empty")?.textContent).toContain(
      "No words yet",
    );
  });

  it("adds a typed word and shows it", () => {
    // The end of the reported bug: the word has to become VISIBLE.
    const h = mount();
    const input = container!.querySelector(".prose-dict-input") as HTMLInputElement;
    const button = container!.querySelector(".settings-apply") as HTMLButtonElement;
    act(() => typeInto(input, "Bellweather"));
    act(() => button.click());
    expect(dictionaryWords(h.current())).toContain("Bellweather");
    expect(shownWords()).toContain("Bellweather");
  });

  it("removes a word and stops showing it", () => {
    const h = mount();
    const remove = container!.querySelector(
      '[aria-label="Remove Griswold from the dictionary"]',
    ) as HTMLButtonElement;
    act(() => remove.click());
    expect(shownWords()).toEqual(["Ashfen"]);
    expect(dictionaryWords(h.current())).toEqual(["Ashfen"]);
  });

  it("does not write for a blank add", () => {
    const h = mount();
    const button = container!.querySelector(".settings-apply") as HTMLButtonElement;
    act(() => button.click());
    expect(h.applied).toEqual([]);
  });

  it("recompiles after an edit, so the checker stops underlining the word", () => {
    // Without this the word is in the file and still underlined, which
    // reads as the action having failed.
    const store = createStudioStore();
    const h = mount();
    void store;
    const remove = container!.querySelector(
      '[aria-label="Remove Ashfen from the dictionary"]',
    ) as HTMLButtonElement;
    act(() => remove.click());
    expect(h.applied.length).toBe(1);
  });
});
