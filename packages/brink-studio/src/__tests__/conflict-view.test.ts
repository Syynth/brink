/**
 * ConflictView tests (issue #320, Track V).
 *
 * The framework-agnostic merge surface: banner ("changed on disk…" +
 * [Keep mine] / [Use disk] / [Apply merge]) over a 2-way @codemirror/merge
 * view (YOURS vs ON DISK). These prove the banner wiring, the Apply-merge
 * enable-on-edit gate, the resolution callbacks, and — per the CM6 teardown
 * contract — that destroy() removes every node it added (no DOM leak).
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { ConflictView } from "@brink/ink-editor";
import type { FileConflict } from "@brink/ink-editor";
import { EditorView } from "@codemirror/view";

const CONFLICT: FileConflict = {
  path: "main.ink",
  disk: "host edit",
  buffer: "studio edit",
  baseline: "original",
};

function makeView() {
  const host = document.createElement("div");
  document.body.appendChild(host);
  const cb = {
    onUseDisk: vi.fn(),
    onKeepMine: vi.fn(),
    onMerge: vi.fn(),
  };
  const view = new ConflictView(host, CONFLICT, cb);
  return { host, view, ...cb };
}

beforeEach(() => {
  document.body.replaceChildren();
});

describe("ConflictView", () => {
  it("renders the banner message and three actions", () => {
    const { host, view } = makeView();
    const banner = host.querySelector(".brink-conflict-banner");
    expect(banner).not.toBeNull();
    expect(banner?.textContent).toContain("main.ink");
    expect(banner?.textContent?.toLowerCase()).toContain("changed on disk");
    expect(host.querySelector(".brink-conflict-keep-mine")).not.toBeNull();
    expect(host.querySelector(".brink-conflict-use-disk")).not.toBeNull();
    expect(host.querySelector(".brink-conflict-apply-merge")).not.toBeNull();
    view.destroy();
  });

  it("mounts a 2-way merge view (two editors, YOURS + ON DISK)", () => {
    const { host, view } = makeView();
    // @codemirror/merge renders its container as .cm-mergeView with two editors.
    expect(host.querySelector(".cm-mergeView")).not.toBeNull();
    expect(host.querySelectorAll(".cm-editor").length).toBe(2);
    view.destroy();
  });

  it("[Keep mine] fires onKeepMine", () => {
    const { host, view, onKeepMine } = makeView();
    host.querySelector<HTMLButtonElement>(".brink-conflict-keep-mine")!.click();
    expect(onKeepMine).toHaveBeenCalledOnce();
    view.destroy();
  });

  it("[Use disk] fires onUseDisk", () => {
    const { host, view, onUseDisk } = makeView();
    host.querySelector<HTMLButtonElement>(".brink-conflict-use-disk")!.click();
    expect(onUseDisk).toHaveBeenCalledOnce();
    view.destroy();
  });

  it("[Apply merge] is disabled until the YOURS pane is edited, then fires onMerge", () => {
    const { host, view, onMerge } = makeView();
    const apply = host.querySelector<HTMLButtonElement>(".brink-conflict-apply-merge")!;
    expect(apply.disabled).toBe(true);

    // Edit the YOURS (left) editor to a hand-merged result. The left pane is
    // the first .cm-editor; resolve its EditorView from the DOM and dispatch.
    const leftDom = host.querySelectorAll<HTMLElement>(".cm-editor")[0]!;
    const left = EditorView.findFromDOM(leftDom);
    expect(left).not.toBeNull();
    left!.dispatch({
      changes: { from: 0, to: left!.state.doc.length, insert: "merged!" },
    });

    expect(view.minedText()).toBe("merged!");
    expect(apply.disabled).toBe(false);
    apply.click();
    expect(onMerge).toHaveBeenCalledWith("merged!");
    view.destroy();
  });

  it("destroy() removes every node it added (no DOM leak)", () => {
    const { host, view } = makeView();
    expect(host.childElementCount).toBeGreaterThan(0);
    view.destroy();
    expect(host.childElementCount).toBe(0);
    expect(host.querySelector(".cm-mergeView")).toBeNull();
    expect(host.querySelector(".brink-conflict-banner")).toBeNull();
  });
});
