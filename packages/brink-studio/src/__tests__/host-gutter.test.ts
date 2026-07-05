/**
 * Host gutter-marker contribution API (#343).
 *
 * Exercises the CM6 wiring with a real `EditorView` (jsdom): markers from the
 * host's `getGutterMarkers` callback render in the `brink-host-gutter`,
 * out-of-range lines are dropped, ordering is deterministic (by line, host
 * array order within a line), clicks dispatch the per-marker `onClick` then the
 * shared `onGutterMarkerClick`, document edits recompute the set, external
 * changes re-render via `refreshGutterMarkers`, and destroying the view tears
 * the gutter DOM down cleanly.
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  hostGutterExtension,
  refreshGutterMarkers,
  type HostGutterMarker,
} from "@brink-lang/editor";

let view: EditorView | null = null;

afterEach(() => {
  view?.destroy();
  view = null;
});

function mount(
  doc: string,
  getGutterMarkers: (source: string, fromLine: number, toLine: number) => HostGutterMarker[],
  onGutterMarkerClick?: (marker: HostGutterMarker, line: number) => void,
): EditorView {
  view = new EditorView({
    state: EditorState.create({
      doc,
      extensions: [hostGutterExtension({ getGutterMarkers, onGutterMarkerClick })],
    }),
    parent: document.body,
  });
  return view;
}

function markerEls(v: EditorView): HTMLElement[] {
  return Array.from(v.dom.querySelectorAll<HTMLElement>(".brink-host-gutter-marker"));
}

describe("hostGutterExtension", () => {
  it("renders host markers keyed by line in the brink-host-gutter", () => {
    const v = mount("one\ntwo\nthree", () => [
      { line: 1, text: "●", className: "bp", title: "Breakpoint" },
      { line: 3, text: "⚑" },
    ]);
    expect(v.dom.querySelector(".cm-gutter.brink-host-gutter")).not.toBeNull();
    const els = markerEls(v);
    expect(els.map((el) => el.textContent)).toEqual(["●", "⚑"]);
    expect(els[0].classList.contains("bp")).toBe(true);
    expect(els[0].title).toBe("Breakpoint");
    expect(els[0].getAttribute("aria-label")).toBe("Breakpoint");
  });

  it("queries the whole document as an inclusive 1-based line range", () => {
    const calls: Array<[number, number]> = [];
    mount("a\nb\nc\nd", (source, fromLine, toLine) => {
      calls.push([fromLine, toLine]);
      expect(source).toBe("a\nb\nc\nd");
      return [];
    });
    expect(calls).toEqual([[1, 4]]);
  });

  it("drops out-of-range and non-integer lines", () => {
    const v = mount("one\ntwo", () => [
      { line: 0, text: "x" },
      { line: -3, text: "x" },
      { line: 99, text: "x" },
      { line: 1.5, text: "x" },
      { line: 2, text: "ok" },
    ]);
    expect(markerEls(v).map((el) => el.textContent)).toEqual(["ok"]);
  });

  it("orders deterministically: by line, host array order within a line", () => {
    const v = mount("one\ntwo\nthree", () => [
      { line: 3, text: "c" },
      { line: 1, text: "a1" },
      { line: 1, text: "a2" },
    ]);
    expect(markerEls(v).map((el) => el.textContent)).toEqual(["a1", "a2", "c"]);
  });

  it("dispatches the marker onClick, then the shared onGutterMarkerClick", () => {
    const order: string[] = [];
    const marker: HostGutterMarker = {
      line: 2,
      text: "●",
      onClick: (line) => order.push(`own:${line}`),
    };
    const v = mount(
      "one\ntwo",
      () => [marker],
      (m, line) => order.push(`shared:${m.text}:${line}`),
    );
    const el = markerEls(v)[0];
    expect(el.tagName).toBe("BUTTON");
    el.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    expect(order).toEqual(["own:2", "shared:●:2"]);
  });

  it("renders a non-interactive span when no click handler is wired", () => {
    const v = mount("one", () => [{ line: 1, text: "i" }]);
    expect(markerEls(v)[0].tagName).toBe("SPAN");
  });

  it("recomputes on document changes", () => {
    let v!: EditorView;
    v = mount("one\ntwo", (source) =>
      source
        .split("\n")
        .map((text, i) => ({ text, i }))
        .filter(({ text }) => text.includes("!"))
        .map(({ i }) => ({ line: i + 1, text: "!" })),
    );
    expect(markerEls(v)).toHaveLength(0);
    v.dispatch({ changes: { from: 0, insert: "!" } });
    expect(markerEls(v).map((el) => el.textContent)).toEqual(["!"]);
  });

  it("re-queries on refreshGutterMarkers without a document change", () => {
    let markers: HostGutterMarker[] = [];
    const v = mount("one\ntwo", () => markers);
    expect(markerEls(v)).toHaveLength(0);

    markers = [{ line: 2, text: "●" }];
    refreshGutterMarkers(v);
    expect(markerEls(v).map((el) => el.textContent)).toEqual(["●"]);

    markers = [];
    refreshGutterMarkers(v);
    expect(markerEls(v)).toHaveLength(0);
  });

  it("swallows a throwing callback (renders no markers)", () => {
    const v = mount("one", () => {
      throw new Error("host bug");
    });
    expect(markerEls(v)).toHaveLength(0);
  });

  it("tears down cleanly on destroy", () => {
    const v = mount("one", () => [{ line: 1, text: "●" }]);
    expect(markerEls(v)).toHaveLength(1);
    const dom = v.dom;
    v.destroy();
    view = null;
    expect(document.body.contains(dom)).toBe(false);
    expect(document.querySelectorAll(".brink-host-gutter-marker")).toHaveLength(0);
  });
});
