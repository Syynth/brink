/**
 * The author's draft patterns, in `[project] drafts` in `brink.toml`.
 *
 * `drafts` has been readable by the compiler since #3145 and editable
 * nowhere — an author could reach it only by hand-editing the config, and
 * nothing told them whether what they typed had worked.
 *
 * "Worked" is the load-bearing word, and why these assertions are about
 * what the panel SHOWS rather than only what the config string holds. A
 * draft is a glob match that the story ALSO does not reach ("reachability
 * wins", 2026-08-27), so a pattern can be spelled perfectly, match a real
 * file, and still produce no draft. Displayed as a bare list of strings
 * that case is indistinguishable from success, and from a typo.
 */
import { describe, it, expect, afterEach, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import { DraftSettings, StoreProvider } from "@brink/studio-ui";
import {
  createStudioStore,
  draftGlobProblem,
  draftGlobs,
  withDraftGlob,
  withoutDraftGlob,
} from "@brink/studio-store";
import type { FileOutline } from "@brink/wasm-types";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

const OUTLINE: FileOutline[] = [
  { path: "brink.toml", symbols: [], mounted: false },
  { path: "main.ink", symbols: [], mounted: false },
];

const CONFIG = `[project]
entry = "main.ink"
drafts = [
  "scratch/**",
]
`;

/** The shape `getDraftGlobReport` returns, as the real session builds it. */
const REPORT = {
  compiled: true,
  globs: [{ glob: "scratch/**", drafts: ["scratch/cut.ink"], inStory: [] }],
};

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
});

async function mount(initial = CONFIG, report: unknown = REPORT) {
  let source = initial;
  const applied: string[] = [];
  const project = {
    getSession: () => ({
      getFileSource: (p: string) => (p === "brink.toml" ? source : null),
    }),
    // The match report rides `projectQuery`, which runs on the worker
    // replica when one is live. The main-thread session never compiles
    // there, so a `getSession().getDraftGlobReport()` would always answer
    // `compiled: false` — this mock offers only the real road so a
    // regression back to the direct call fails here.
    projectQuery: (method: string) =>
      method === "getDraftGlobReport"
        ? Promise.resolve(report)
        : Promise.reject(new Error(`unexpected query ${method}`)),
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
  await act(async () => {
    root!.render(createElement(StoreProvider, { store, children: createElement(DraftSettings) }));
  });
  // Let the `projectQuery` pull settle — the match report arrives a
  // microtask after the first paint, exactly as it does in the studio.
  await act(async () => {
    await Promise.resolve();
  });
  return { applied, current: () => source };
}

/** See `prose-dictionary.test.tsx` — React suppresses a bypassed setter. */
function typeInto(input: HTMLInputElement, text: string): void {
  const setter = Object.getOwnPropertyDescriptor(
    window.HTMLInputElement.prototype,
    "value",
  )?.set;
  setter?.call(input, text);
  input.dispatchEvent(new Event("input", { bubbles: true }));
}

const shownPatterns = (): string[] =>
  [...container!.querySelectorAll(".draft-globs-pattern")].map((el) => el.textContent ?? "");

const shownCounts = (): string[] =>
  [...container!.querySelectorAll(".draft-globs-count")].map((el) => el.textContent ?? "");

describe("the pure edits", () => {
  it("reads the patterns the config declares", () => {
    expect(draftGlobs(CONFIG)).toEqual(["scratch/**"]);
  });

  it("reads an empty list from a config with no drafts key", () => {
    expect(draftGlobs('[project]\nentry = "main.ink"\n')).toEqual([]);
  });

  it("adds a pattern, sorted so the file does not churn by insertion order", () => {
    const next = withDraftGlob(CONFIG, "notes/**");
    expect(next).not.toBeNull();
    expect(draftGlobs(next!)).toEqual(["notes/**", "scratch/**"]);
  });

  it("returns null rather than an identical string for a duplicate", () => {
    // An applied no-op still marks the file dirty and triggers a recompile.
    expect(withDraftGlob(CONFIG, "scratch/**")).toBeNull();
  });

  it("removes a pattern, and reports nothing to do for one not there", () => {
    expect(draftGlobs(withoutDraftGlob(CONFIG, "scratch/**")!)).toEqual([]);
    expect(withoutDraftGlob(CONFIG, "notes/**")).toBeNull();
  });

  it("rejects only what the config or the glob dialect cannot represent", () => {
    expect(draftGlobProblem("scratch/**")).toBeNull();
    // Valid today even though nothing matches it — the folder may not exist
    // yet, and the panel reports emptiness from the real match report.
    expect(draftGlobProblem("not-yet/**")).toBeNull();
    expect(draftGlobProblem('a"b')).not.toBeNull();
    expect(draftGlobProblem("/absolute/**")).not.toBeNull();
    expect(draftGlobProblem("../outside/**")).not.toBeNull();
  });
});

describe("the settings panel", () => {
  it("shows the patterns the config declares", async () => {
    await mount();
    expect(shownPatterns()).toEqual(["scratch/**"]);
  });

  it("writes an added pattern back to the config", async () => {
    const { current } = await mount();
    const input = container!.querySelector<HTMLInputElement>(".draft-globs-input")!;
    typeInto(input, "notes/**");
    act(() => {
      container!.querySelector<HTMLButtonElement>(".settings-apply")!.click();
    });
    expect(draftGlobs(current())).toEqual(["notes/**", "scratch/**"]);
    expect(shownPatterns()).toEqual(["notes/**", "scratch/**"]);
  });

  it("removes a pattern from the config and the view", async () => {
    const { current } = await mount();
    act(() => {
      container!.querySelector<HTMLButtonElement>(".draft-globs-remove")!.click();
    });
    expect(draftGlobs(current())).toEqual([]);
    expect(shownPatterns()).toEqual([]);
  });

  it("refuses a pattern the config cannot hold, without writing", async () => {
    const { applied } = await mount();
    const input = container!.querySelector<HTMLInputElement>(".draft-globs-input")!;
    act(() => typeInto(input, '/absolute/**'));
    const add = container!.querySelector<HTMLButtonElement>(".settings-apply")!;
    expect(add.disabled).toBe(true);
    expect(container!.querySelector(".draft-globs-problem")).not.toBeNull();
    expect(applied).toEqual([]);
  });

  it("counts the drafts a pattern actually produced", async () => {
    await mount();
    expect(shownCounts()).toEqual(["1 draft"]);
  });

  it("says a pattern matches nothing, so a typo is visible", async () => {
    // The commonest mistake: a misspelt folder looks exactly like a working
    // pattern in a bare list of strings.
    await mount('[project]\ndrafts = ["scrach/**"]\n', {
      compiled: true,
      globs: [{ glob: "scrach/**", drafts: [], inStory: [] }],
    });
    expect(shownCounts()).toEqual(["matches nothing"]);
  });

  it("distinguishes 'not checked yet' from 'matches nothing'", async () => {
    // Identical empty lists, opposite meanings: before a compile nothing is
    // known to be unreachable, so no glob can have made a draft yet.
    await mount(CONFIG, {
      compiled: false,
      globs: [{ glob: "scratch/**", drafts: [], inStory: [] }],
    });
    expect(shownCounts()).toEqual(["not checked yet"]);
  });

  it("explains a match the story still reaches, which is not a draft", async () => {
    // "Reachability wins" (2026-08-27) made visible. Without this line the
    // author sees a correct pattern that produced no draft and no reason.
    await mount('[project]\ndrafts = ["scenes/**"]\n', {
      compiled: true,
      globs: [{ glob: "scenes/**", drafts: [], inStory: ["scenes/harbour.ink"] }],
    });
    const note = container!.querySelector(".draft-globs-in-story");
    expect(note).not.toBeNull();
    expect(note!.textContent).toContain("scenes/harbour.ink");
    expect(note!.textContent).toContain("not");
  });

  it("lists the files a pattern made drafts", async () => {
    await mount(CONFIG, {
      compiled: true,
      globs: [
        { glob: "scratch/**", drafts: ["scratch/a.ink", "scratch/b.ink"], inStory: [] },
      ],
    });
    const files = container!.querySelector(".draft-globs-files");
    expect(files).not.toBeNull();
    expect(files!.textContent).toContain("scratch/a.ink");
    expect(files!.textContent).toContain("scratch/b.ink");
  });

  it("shows a pattern the report has not seen without inventing matches for it", async () => {
    // A pattern added since the last compile: present in the config, absent
    // from the report. It must still be listed, and must not borrow another
    // pattern's counts.
    await mount('[project]\ndrafts = ["scratch/**", "notes/**"]\n', REPORT);
    expect(shownPatterns()).toEqual(["scratch/**", "notes/**"]);
    expect(shownCounts()).toEqual(["1 draft", "not checked yet"]);
  });
});
