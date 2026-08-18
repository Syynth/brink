/**
 * Consumer-level paint proof for the "focus must not land in a group
 * EditorArea is not rendering" invariant (§5.4; #2787, #2797, #2826).
 *
 * Every existing test for this invariant — #2787's, #2797's (PR #2817), and
 * the store-level ones added alongside this file for #2826 — asserts only on
 * `EditorGroupsState` (`maximizedGroupId`, `focusedGroupId`, `groups`). None
 * renders `EditorArea` and checks the DOM, so a store-state assertion could
 * pass while the group EditorArea actually paints is still the wrong one (a
 * bug in the store→DOM wiring itself would slip through every one of them).
 * #2826's "Also noted" section calls this out as a standing gap for the
 * maximize area; this file closes it for the #2826 hole-2 fix (openDocument's
 * new-tab fall-through) by mounting a real `ShellProvider` + `EditorArea`
 * over a real editor-groups store and asserting on rendered DOM.
 */

import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { act } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  DocumentTypeRegistry,
  EditorArea,
  ShellProvider,
  createEditorGroupsStore,
  type DocumentRef,
  type DocumentViewProps,
  type EditorGroupsStore,
} from "@brink/studio-shell";

// jsdom has no ResizeObserver; GroupTabBar (mounted by every editor group)
// uses one to compute the overflow-chevron state (#278) — see
// argument-widget-type-honesty.test.tsx / base-type-widgets.test.ts for the
// same stub pattern elsewhere in this suite. jsdom also has no
// scrollIntoView (GroupTabBar keeps the active tab in view, same effect).
beforeAll(() => {
  if (typeof globalThis.ResizeObserver === "undefined") {
    class ResizeObserverStub {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver =
      ResizeObserverStub;
  }
  if (typeof Element.prototype.scrollIntoView === "undefined") {
    Element.prototype.scrollIntoView = () => {};
  }
});

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

function ref(docId: string): DocumentRef {
  return { typeId: "test-doc", docId, title: docId };
}

/** Renders the doc's id as plain text so a test can assert on paint. */
function TestDoc({ doc }: DocumentViewProps) {
  return <div data-testid="doc-body">{doc.docId}</div>;
}

function documents(): DocumentTypeRegistry {
  const registry = new DocumentTypeRegistry();
  registry.register({ id: "test-doc", component: TestDoc });
  return registry;
}

let root: Root | null = null;
let container: HTMLDivElement | null = null;

afterEach(() => {
  if (root) act(() => root!.unmount());
  container?.remove();
  root = null;
  container = null;
});

function mount(editorGroups: EditorGroupsStore, docs: DocumentTypeRegistry): HTMLDivElement {
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const commands = new CommandRegistry();
  act(() => {
    root!.render(
      <ShellProvider commands={commands} editorGroups={editorGroups} documents={docs}>
        <EditorArea />
      </ShellProvider>,
    );
  });
  return container;
}

describe("EditorArea actually paints the revealed group (#2826)", () => {
  it("opening a never-before-opened doc into a maximized-hidden focused group reveals it in the DOM", () => {
    const store = createEditorGroupsStore();
    // Two groups: main.ink in group 1, other.ink split into group 2.
    store.getState().openDocument(ref("main.ink"));
    const first = store.getState().focusedGroupId;
    store.getState().openDocument(ref("other.ink"), { group: "split-right" });
    const second = store.getState().focusedGroupId;
    expect(second).not.toBe(first);

    const docs = documents();
    const el = mount(store, docs);

    // Maximize `first` — same as any maximize entry point (§5.4).
    act(() => store.getState().toggleMaximizeGroup(first));
    expect(store.getState().maximizedGroupId).toBe(first);

    // Sanity: only the maximized group's section is in the DOM right now.
    expect(el.querySelector(`[data-editor-group="${first}"]`)).not.toBeNull();
    expect(el.querySelector(`[data-editor-group="${second}"]`)).toBeNull();

    // Desync focus onto the hidden group — the store-level shape of what
    // editor.focusNextGroup's `when: groups.length > 1` lets happen while
    // maximized (#2826 hole 1). Reproduced directly via `focusGroup` here so
    // this paint proof does not depend on that command's still-undetermined
    // fix shape (clear-and-move vs. `when`-gate) — either answer produces
    // this same store state as its starting point.
    act(() => store.getState().focusGroup(second));
    expect(store.getState().focusedGroupId).toBe(second);
    expect(store.getState().maximizedGroupId).toBe(first);

    // A doc never opened anywhere: openDocument's "focused" target falls
    // through past the reveal branch into the new-tab branch, targeting the
    // (hidden, desynced) focused group — #2826 hole 2's exact repro.
    act(() => store.getState().openDocument(ref("brand-new.ink")));

    // Store contract (mirrors the store-level tests): un-maximized, still
    // focused on `second`.
    expect(store.getState().maximizedGroupId).toBeNull();
    expect(store.getState().focusedGroupId).toBe(second);

    // The actual paint proof: both groups are back in the DOM, and the
    // target group's tab strip + body show the newly opened document — not
    // just a store-state assertion that nothing visibly contradicts.
    const secondSection = el.querySelector(`[data-editor-group="${second}"]`);
    expect(secondSection).not.toBeNull();
    expect(el.querySelector(`[data-editor-group="${first}"]`)).not.toBeNull();
    expect(secondSection!.querySelector('[role="tab"][title="brand-new.ink"]')).not.toBeNull();
    expect(secondSection!.querySelector('[data-testid="doc-body"]')?.textContent).toBe(
      "brand-new.ink",
    );
  });
});
