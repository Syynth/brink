/**
 * Settings → Conventions, the teach-by-example editor (#3411): pick a
 * passage through the knot/stitch typeahead, mark lines, read the learned
 * rules and the Player preview, confirm — and the write goes through the
 * `[dialogue]` section road, asking before replacing a hand-written one.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { ConventionsSettings, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import type { FileOutline, PassageLine } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "brink.toml", symbols: [], mounted: false },
  {
    path: "act1/chapel.ink",
    mounted: false,
    symbols: [
      {
        name: "chapel",
        kind: "knot",
        children: [{ name: "argument", kind: "stitch", children: [] } as never],
      } as never,
    ],
  },
];

const PASSAGE: PassageLine[] = [
  { text: "@MARA: <>", tags: [], file: "act1/chapel.ink", line: 3, origin: "line" },
  { text: "We don't have until morning.", tags: [], file: "act1/chapel.ink", line: 4, origin: "line" },
  { text: "Not even close.", tags: [], file: "act1/chapel.ink", line: 5, origin: "line" },
  { text: "> She sets the lantern down.", tags: [], file: "act1/chapel.ink", line: 6, origin: "line" },
  { text: "The lantern gutters.", tags: [], file: "act1/chapel.ink", line: 7, origin: "line" },
  { text: "@JUNO: <>", tags: [], file: "act1/chapel.ink", line: 8, origin: "line" },
  { text: "Then we go now.", tags: [], file: "act1/chapel.ink", line: 9, origin: "line" },
  { text: "[Take the lantern]", tags: [], file: "act1/chapel.ink", line: 10, origin: "choice" },
  { text: "Lisa: Where did he go?", tags: [], file: "act1/chapel.ink", line: 11, origin: "choice" },
];

const CONFIG = `[project]\nentry = "main.ink" # keep\n`;

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

async function mount(initial = CONFIG) {
  let source = initial;
  const applied: Array<[string, string]> = [];
  const added: Array<[string, string]> = [];
  const project = {
    getSession: () => ({
      getFileSource: (p: string) => (p === "brink.toml" ? source : null),
    }),
    passageLines: (path: string) => (path === "chapel.argument" ? PASSAGE : null),
    applyEdit: (path: string, next: string) => {
      if (path === "brink.toml") source = next;
      applied.push([path, next]);
      return true;
    },
    addFile: (path: string, content: string) => {
      added.push([path, content]);
      return Promise.resolve();
    },
  };
  const docs = { refreshExternal: vi.fn(), triggerCompile: vi.fn() };
  const store = createStudioStore();
  store.getState().setCompileResult(OUTLINE, { errors: 0, warnings: 0 }, [], null);
  store.setState({ _project: project as never, _documents: docs as never });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  await act(async () => {
    root!.render(
      createElement(StoreProvider, { store, children: createElement(ConventionsSettings) }),
    );
  });
  return { store, applied, added, docs, source: () => source };
}

const $ = <T extends Element>(sel: string): T => {
  const el = container!.querySelector<T>(sel);
  if (!el) throw new Error(`missing ${sel}`);
  return el;
};

async function typeQuery(text: string) {
  const input = $<HTMLInputElement>("input.pl-typeahead-input");
  const setter = Object.getOwnPropertyDescriptor(HTMLInputElement.prototype, "value")!.set!;
  await act(async () => {
    setter.call(input, text);
    input.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

async function pickPassage() {
  await typeQuery("chap");
  const rows = container!.querySelectorAll("button.pl-typeahead-row");
  expect(Array.from(rows).map((r) => r.textContent)).toEqual([
    "chapelact1/chapel.ink",
    "chapel.argumentact1/chapel.ink",
  ]);
  await act(async () => {
    rows[1].dispatchEvent(new MouseEvent("mousedown", { bubbles: true, cancelable: true }));
  });
}

async function mark(line: number, label: string) {
  const btn = $<HTMLButtonElement>(`button[aria-label="Mark line ${line.toString()} as ${label}"]`);
  await act(async () => {
    btn.click();
  });
}

async function markCanvasSample() {
  await mark(1, "Cue");
  await mark(2, "Dialogue");
  await mark(3, "Dialogue");
  await mark(4, "Action");
  await mark(5, "Narration");
  await mark(6, "Cue");
  await mark(7, "Dialogue");
}

describe("ConventionsSettings", () => {
  it("focusing the field lists every knot and stitch before anything is typed", async () => {
    await mount();
    expect(container!.querySelector(".pl-typeahead-list")).toBeNull();
    const input = $<HTMLInputElement>("input.pl-typeahead-input");
    await act(async () => {
      input.focus();
    });
    const rows = Array.from(container!.querySelectorAll("button.pl-typeahead-row")).map(
      (r) => r.querySelector(".pl-save-name")?.textContent,
    );
    expect(rows).toEqual(["chapel", "chapel.argument"]);
    await act(async () => {
      input.blur();
    });
    expect(container!.querySelector(".pl-typeahead-list")).toBeNull();
  });

  it("pulls a passage through the typeahead and lists every line with the mark control", async () => {
    await mount();
    await pickPassage();
    expect(container!.querySelectorAll(".conv-line").length).toBe(7);
    expect($(".conv-line-count").textContent).toContain("chapel.argument · 7 lines · 2 choices hidden");
    // The preview shows the passage, plain until something is marked.
    expect($(".settings-conv-player").textContent).toContain("We don't have until morning.");
    expect(container!.querySelector(".player-run-cue")).toBeNull();
  });

  it("marking lines shows learned rules with support counts and folds the preview into runs", async () => {
    await mount();
    await pickPassage();
    await markCanvasSample();
    const learned = Array.from(container!.querySelectorAll(".conv-learned-row")).map((r) => r.textContent);
    expect(learned[0]).toContain("starts with “@” and ends with “: <>” is a cue naming the speaker.");
    expect(learned[0]).toContain("2 of 2");
    expect(learned.some((t) => t?.includes("is an action line") && t.includes("1 of 1"))).toBe(true);
    expect(learned.some((t) => t?.includes("until an action line, the next cue or the choices"))).toBe(true);
    expect(container!.querySelector(".conv-learned-row.is-decision")).toBeNull();
    const cues = Array.from(container!.querySelectorAll(".player-run-cue")).map((c) => c.textContent);
    expect(cues).toEqual(["MARA", "JUNO"]);
    // As the Player itself renders it: the segment text as delivered, marker included
    // (`renderRowBody` drops only a character row's own cue segment).
    expect($(".player-line-row.dialect-action p").textContent).toBe("> She sets the lantern down.");
  });

  it("a decision blocks confirm and flags the lines involved", async () => {
    await mount();
    await pickPassage();
    await mark(1, "Cue");
    await mark(2, "Dialogue");
    await mark(3, "Narration");
    const decision = $(".conv-learned-row.is-decision");
    expect(decision.textContent).toContain("marked narration but follows a speaker's lines");
    expect(decision.textContent).toContain("(line 3)");
    expect(container!.querySelectorAll(".conv-line.is-flagged").length).toBe(1);
    const confirm = Array.from(container!.querySelectorAll("button")).find(
      (b) => b.textContent === "Use these rules",
    ) as HTMLButtonElement;
    expect(confirm.disabled).toBe(true);
  });

  it("confirm writes the [dialogue] section as the at-cue recipe plus the learned rows, and recompiles", async () => {
    const h = await mount();
    await pickPassage();
    await markCanvasSample();
    const confirm = Array.from(container!.querySelectorAll("button")).find(
      (b) => b.textContent === "Use these rules",
    ) as HTMLButtonElement;
    expect(confirm.disabled).toBe(false);
    await act(async () => {
      confirm.click();
    });
    expect(h.applied.length).toBe(1);
    const [path, next] = h.applied[0];
    expect(path).toBe("brink.toml");
    expect(next.startsWith(CONFIG)).toBe(true);
    expect(next).toContain("# conventions-editor:");
    expect(next).toContain('[dialogue]\npreset = "at-cue"\nrun-ends-at = ["choices", "action"]');
    expect(next).toContain('[[dialogue.elements]]\nkind = "character"\nprefix = "@"\nsuffix = ": "\nglued = true\ncontent-role = "speaker"');
    expect(next).toContain('[[dialogue.elements]]\nkind = "action"\nprefix = "> "');
    expect(h.added).toEqual([]);
    expect(h.docs.refreshExternal).toHaveBeenCalledWith("brink.toml");
    expect(h.docs.triggerCompile).toHaveBeenCalled();
    expect($(".conv-status").textContent).toContain("Written to brink.toml");
  });

  it("asks before replacing a hand-written [dialogue] section, and replaces on the second click only", async () => {
    const hand = `${CONFIG}\n[dialogue]\npreset = "at-cue"\n`;
    const h = await mount(hand);
    await pickPassage();
    await markCanvasSample();
    const confirm = Array.from(container!.querySelectorAll("button")).find(
      (b) => b.textContent === "Use these rules",
    ) as HTMLButtonElement;
    await act(async () => {
      confirm.click();
    });
    expect(h.applied).toEqual([]);
    const ask = $(".conv-ask");
    expect(ask.textContent).toContain("written by hand");
    expect($(".conv-ask-block").textContent).toBe('[dialogue]\npreset = "at-cue"');
    const replace = Array.from(ask.querySelectorAll("button")).find((b) => b.textContent === "Replace it")!;
    await act(async () => {
      replace.click();
    });
    expect(h.applied.length).toBe(1);
    expect(h.source()).toContain("# conventions-editor:");
    expect(h.source().startsWith(CONFIG)).toBe(true);
    expect(container!.querySelector(".conv-ask")).toBeNull();
  });

  it("choice text is hidden by default and comes back, markable, with the toggle", async () => {
    await mount();
    await pickPassage();
    const toggle = $<HTMLInputElement>(".conv-choices-toggle input");
    expect(toggle.checked).toBe(false);
    expect(container!.querySelector(".conv-line-badge")).toBeNull();
    await act(async () => {
      toggle.click();
    });
    expect(container!.querySelectorAll(".conv-line").length).toBe(9);
    expect(container!.querySelectorAll(".conv-line-badge").length).toBe(2);
    expect($(".conv-line-count").textContent).not.toContain("hidden");
    // A choice-text cue teaches the ink docs' `Name: text` shape.
    await mark(9, "Cue");
    const learned = Array.from(container!.querySelectorAll(".conv-learned-row")).map((r) => r.textContent);
    expect(learned.some((t) => t?.includes("starts with a name and a colon"))).toBe(true);
    // Hiding again keeps the mark keyed to its line and drops it from the inference.
    await act(async () => {
      toggle.click();
    });
    expect(container!.querySelectorAll(".conv-line").length).toBe(7);
    expect(container!.querySelector(".conv-learned")).toBeNull();
  });

  it("pasted lines work without a project passage", async () => {
    await mount();
    const paste = $<HTMLButtonElement>('button[aria-label="Paste lines instead"]');
    await act(async () => {
      paste.click();
    });
    const ta = $<HTMLTextAreaElement>("textarea");
    const setter = Object.getOwnPropertyDescriptor(HTMLTextAreaElement.prototype, "value")!.set!;
    await act(async () => {
      setter.call(ta, "MARA\n(quietly)\nWe wait.\n");
      ta.dispatchEvent(new Event("input", { bubbles: true }));
    });
    const use = Array.from(container!.querySelectorAll("button")).find((b) => b.textContent === "Use these lines")!;
    await act(async () => {
      use.click();
    });
    expect(container!.querySelectorAll(".conv-line").length).toBe(3);
    await mark(1, "Cue");
    await mark(2, "Aside");
    await mark(3, "Dialogue");
    const learned = Array.from(container!.querySelectorAll(".conv-learned-row")).map((r) => r.textContent);
    expect(learned.some((t) => t?.includes("in capitals on its own"))).toBe(true);
    expect(learned.some((t) => t?.includes("is a parenthetical"))).toBe(true);
  });
});
