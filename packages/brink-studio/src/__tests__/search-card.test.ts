/**
 * Search result cards — pure model + the per-card CM6 buffer
 * (docs/search-results-cards-spec.md, PR C).
 *
 * Covers `cardSlice` (the context window around a match: clamping at file
 * edges, hit offsets, tunable before/after), `cardLineSegments` (static
 * rendering: token classes + the hit mark, overlap producing combined
 * classes), and `SearchCardBuffer` (whole-slice commit on idle/destroy,
 * doc reconciliation via setCard, the dirty-hold that keeps a user's
 * uncommitted text from being clobbered by a remap).
 */

import { describe, expect, it, afterEach } from "vitest";
import {
  SearchCardBuffer,
  cardLineSegments,
  cardSlice,
  type ReplacementEdit,
  type SearchCardHighlight,
  type SearchCardModel,
} from "@brink/studio-store";

const SOURCE = "line one\nYour torch is lit.\nline three\nline four\nline five";
const TORCH = SOURCE.indexOf("torch");

// ── cardSlice ───────────────────────────────────────────────────────

describe("cardSlice", () => {
  it("window: 1 above, 2 below around the match line (the ruled default)", () => {
    const slice = cardSlice(SOURCE, TORCH, TORCH + 5, { before: 1, after: 2 });
    expect(slice.text).toBe("line one\nYour torch is lit.\nline three\nline four");
    expect(slice.from).toBe(0);
    expect(slice.firstLine).toBe(1);
    expect(slice.hit).toEqual({ from: TORCH, to: TORCH + 5 });
  });

  it("clamps at the file start and end", () => {
    const first = cardSlice(SOURCE, 0, 4, { before: 3, after: 0 });
    expect(first.text).toBe("line one");
    expect(first.firstLine).toBe(1);

    const lastStart = SOURCE.lastIndexOf("five");
    const last = cardSlice(SOURCE, lastStart, lastStart + 4, { before: 0, after: 5 });
    expect(last.text).toBe("line five");
    expect(last.firstLine).toBe(5);
    expect(last.hit).toEqual({ from: 5, to: 9 });
  });

  it("zero context is exactly the match line", () => {
    const slice = cardSlice(SOURCE, TORCH, TORCH + 5, { before: 0, after: 0 });
    expect(slice.text).toBe("Your torch is lit.");
    expect(slice.firstLine).toBe(2);
  });

  it("walks back through an empty first line without sticking", () => {
    const src = "\nabc";
    const slice = cardSlice(src, 1, 4, { before: 1, after: 0 });
    expect(slice.from).toBe(0);
    expect(slice.firstLine).toBe(1);
    expect(slice.text).toBe("\nabc");
  });

  it("covers a multi-line span through to the end line", () => {
    const start = SOURCE.indexOf("Your");
    const end = SOURCE.indexOf("three") + 5;
    const slice = cardSlice(SOURCE, start, end, { before: 0, after: 0 });
    expect(slice.text).toBe("Your torch is lit.\nline three");
  });

  it("drops the hit when the span collapsed to nothing", () => {
    const slice = cardSlice(SOURCE, TORCH, TORCH, { before: 0, after: 0 });
    expect(slice.hit).toBeNull();
  });
});

// ── cardLineSegments ────────────────────────────────────────────────

function model(over: Partial<SearchCardModel> = {}): SearchCardModel {
  const slice = cardSlice(SOURCE, TORCH, TORCH + 5, { before: 1, after: 1 });
  return {
    path: "a.ink",
    from: slice.from,
    to: slice.to,
    firstLine: slice.firstLine,
    text: slice.text,
    hit: slice.hit,
    ...over,
  };
}

describe("cardLineSegments", () => {
  it("marks the hit and applies token classes on the right card lines", () => {
    // A keyword token on file line 2 (0-based line 1): "Your" (cols 0–4).
    const highlight: SearchCardHighlight = {
      tokens: [
        { line: 1, start_char: 0, length: 4, token_type: 0, token_modifiers: 0 },
      ],
      typeNames: ["keyword"],
    };
    const lines = cardLineSegments(model(), highlight);
    expect(lines).toHaveLength(3);
    // Line 2 of the card: "Your torch is lit." with tok-keyword on "Your"
    // and the hit on "torch".
    const flat = lines[1]?.map((s) => ({ text: s.text, classes: s.classes.join(" ") }));
    expect(flat).toEqual([
      { text: "Your", classes: "tok-keyword" },
      { text: " ", classes: "" },
      { text: "torch", classes: "brink-search-hit" },
      { text: " is lit.", classes: "" },
    ]);
  });

  it("combines classes where a token and the hit overlap", () => {
    const highlight: SearchCardHighlight = {
      // Token covering "torch is" — overlaps the hit ("torch").
      tokens: [
        { line: 1, start_char: 5, length: 8, token_type: 0, token_modifiers: 0 },
      ],
      typeNames: ["variable"],
    };
    const lines = cardLineSegments(model(), highlight);
    const overlapped = lines[1]?.find((s) => s.text === "torch");
    expect(overlapped?.classes.sort()).toEqual(["brink-search-hit", "tok-variable"]);
  });

  it("renders plain segments without highlight data", () => {
    const lines = cardLineSegments(model({ hit: null }), null);
    expect(lines[1]).toEqual([{ text: "Your torch is lit.", classes: [] }]);
  });
});

// ── SearchCardBuffer ────────────────────────────────────────────────

let host: HTMLElement | null = null;
let buffer: SearchCardBuffer | null = null;
afterEach(() => {
  buffer?.destroy();
  buffer = null;
  host?.remove();
  host = null;
});

function mountBuffer(
  m: SearchCardModel,
  commits: Array<{ path: string; edit: ReplacementEdit }>,
  commitDelayMs = 0,
): SearchCardBuffer {
  host = document.createElement("div");
  document.body.appendChild(host);
  buffer = new SearchCardBuffer(host, m, null, {
    onCommit: (path, edit) => commits.push({ path, edit }),
    commitDelayMs,
  });
  return buffer;
}

describe("SearchCardBuffer", () => {
  it("commits an edit as one whole-slice source replacement", () => {
    const commits: Array<{ path: string; edit: ReplacementEdit }> = [];
    const m = model();
    const buf = mountBuffer(m, commits);
    const view = buf.editorView;
    expect(view).not.toBeNull();

    const at = m.text.indexOf("torch");
    view?.dispatch({ changes: { from: at, to: at + 5, insert: "lantern" } });

    expect(commits).toHaveLength(1);
    expect(commits[0]?.path).toBe("a.ink");
    expect(commits[0]?.edit).toEqual({
      start: m.from,
      end: m.to,
      text: m.text.replace("torch", "lantern"),
    });
  });

  it("setCard resets a clean buffer to the reconciled slice", () => {
    const commits: Array<{ path: string; edit: ReplacementEdit }> = [];
    const m = model();
    const buf = mountBuffer(m, commits);
    const next = model({ text: m.text.replace("torch", "lantern") });
    buf.setCard(next, null);
    expect(buf.editorView?.state.doc.toString()).toBe(next.text);
    // A programmatic reset is not a user edit — nothing committed.
    expect(commits).toHaveLength(0);
  });

  it("setCard never clobbers a dirty buffer; destroy flushes the commit", () => {
    const commits: Array<{ path: string; edit: ReplacementEdit }> = [];
    const m = model();
    // Long delay: the edit stays pending.
    const buf = mountBuffer(m, commits, 60_000);
    const at = m.text.indexOf("torch");
    buf.editorView?.dispatch({ changes: { from: at, to: at + 5, insert: "wip" } });
    expect(commits).toHaveLength(0);

    // A remap arrives while the user's text is uncommitted — it must not win.
    buf.setCard(model({ text: m.text.replace("lit", "LIT") }), null);
    expect(buf.editorView?.state.doc.toString()).toBe(m.text.replace("torch", "wip"));

    buf.destroy();
    expect(commits).toHaveLength(1);
    expect(commits[0]?.edit.text).toBe(m.text.replace("torch", "wip"));
  });

  it("offsets the line-number gutter to the slice's file position", () => {
    const m = model();
    expect(m.firstLine).toBe(1);
    const shifted = { ...m, firstLine: 82 };
    const buf = mountBuffer(shifted, []);
    const numbers = [...(host?.querySelectorAll(".cm-gutterElement") ?? [])]
      .map((el) => el.textContent)
      .filter((t) => t !== null && /^\d+$/.test(t));
    // (The first gutter element is CM's hidden width-spacer — don't index,
    // just require the real file-offset numbers to be present.)
    expect(numbers).toEqual(expect.arrayContaining(["82", "83", "84"]));
  });

  it("a no-op round trip commits nothing", () => {
    const commits: Array<{ path: string; edit: ReplacementEdit }> = [];
    const m = model();
    const buf = mountBuffer(m, commits, 60_000);
    const at = m.text.indexOf("torch");
    buf.editorView?.dispatch({ changes: { from: at, to: at + 5, insert: "birch" } });
    buf.editorView?.dispatch({ changes: { from: at, to: at + 5, insert: "torch" } });
    buf.destroy();
    expect(commits).toHaveLength(0);
  });
});
