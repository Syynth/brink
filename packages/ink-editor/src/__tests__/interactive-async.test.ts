/**
 * Async interactive queries (W2c of docs/editor-worker-spec.md): the two
 * hand-rolled landing paths — signature help (tooltip with seq +
 * doc-held-still guards) and the code-actions menu (opens only if doc
 * and cursor held still). Completion and hover ride CM6's own native
 * promise handling and are covered by its contract, not re-tested here.
 */

import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import type { CodeAction, SignatureInfo } from "@brink/wasm-types";
import { signatureHelpExtension } from "../signature-help.js";
import { codeActionsExtension } from "../code-actions.js";

const flush = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

function sig(label: string): SignatureInfo {
  return { label, parameters: [], active_parameter: 0 } as unknown as SignatureInfo;
}

interface Deferred<T> {
  promise: Promise<T>;
  resolve(value: T): void;
}

function deferred<T>(): Deferred<T> {
  let resolve!: (v: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

describe("signatureHelpExtension with an async source", () => {
  let view: EditorView | null = null;

  afterEach(() => {
    view?.destroy();
    view = null;
  });

  function mount(source: (src: string, offset: number) => Promise<SignatureInfo | null>): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc: "fn",
        extensions: [signatureHelpExtension({ getSignatureHelp: source })],
      }),
      parent: document.body,
    });
    return view;
  }

  const tooltipShown = (v: EditorView): boolean =>
    v.dom.querySelector(".brink-signature-help") !== null;

  it("lands the resolved signature as a tooltip", async () => {
    const d = deferred<SignatureInfo | null>();
    const v = mount(() => d.promise);
    v.dispatch({ changes: { from: 2, to: 2, insert: "(" } }); // trigger char
    expect(tooltipShown(v)).toBe(false);
    d.resolve(sig("fn(x)"));
    await flush();
    expect(tooltipShown(v)).toBe(true);
  });

  it("discards an out-of-order landing (only the latest query wins)", async () => {
    const pending: Deferred<SignatureInfo | null>[] = [];
    const v = mount(() => {
      const d = deferred<SignatureInfo | null>();
      pending.push(d);
      return d.promise;
    });
    v.dispatch({ changes: { from: 2, to: 2, insert: "(" } });
    v.dispatch({ changes: { from: 3, to: 3, insert: "(" } });
    expect(pending).toHaveLength(2);
    // The SECOND (latest) query resolves null first; the first's stale
    // signature must not resurrect the tooltip afterwards.
    pending[1]!.resolve(null);
    await flush();
    pending[0]!.resolve(sig("stale()"));
    await flush();
    expect(tooltipShown(v)).toBe(false);
  });

  it("discards a landing whose doc changed while in flight", async () => {
    const pending: Deferred<SignatureInfo | null>[] = [];
    const v = mount(() => {
      const d = deferred<SignatureInfo | null>();
      pending.push(d);
      return d.promise;
    });
    v.dispatch({ changes: { from: 2, to: 2, insert: "(" } });
    // Non-trigger typing: no new query, but the doc moved.
    v.dispatch({ changes: { from: 3, to: 3, insert: "x" } });
    expect(pending).toHaveLength(1);
    pending[0]!.resolve(sig("stale()"));
    await flush();
    expect(tooltipShown(v)).toBe(false);
  });
});

describe("codeActionsExtension with an async source", () => {
  let view: EditorView | null = null;

  afterEach(() => {
    view?.destroy();
    view = null;
  });

  function action(title: string): CodeAction {
    return { title, kind: "quickfix" } as unknown as CodeAction;
  }

  function mount(source: (src: string, offset: number) => Promise<CodeAction[]>): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc: "hello",
        extensions: [codeActionsExtension({ getCodeActions: source, onSelect: () => {} })],
      }),
      parent: document.body,
    });
    return view;
  }

  function pressMenuKey(v: EditorView): void {
    // Drive the keymap's run() directly through CM's key dispatch.
    const event = new KeyboardEvent("keydown", { key: ".", ctrlKey: true });
    v.contentDOM.dispatchEvent(event);
  }

  const menuShown = (v: EditorView): boolean =>
    v.dom.querySelector(".brink-code-actions-menu") !== null ||
    document.querySelector(".brink-code-actions-menu") !== null;

  it("opens the menu when the pull lands and the doc held still", async () => {
    const d = deferred<CodeAction[]>();
    const v = mount(() => d.promise);
    pressMenuKey(v);
    d.resolve([action("Do the thing")]);
    await flush();
    expect(menuShown(v)).toBe(true);
  });

  it("does not open when the doc moved while the pull was in flight", async () => {
    const d = deferred<CodeAction[]>();
    const v = mount(() => d.promise);
    pressMenuKey(v);
    v.dispatch({ changes: { from: 0, to: 0, insert: "x" } });
    d.resolve([action("Do the thing")]);
    await flush();
    expect(menuShown(v)).toBe(false);
  });

  it("does not open when the cursor moved while the pull was in flight", async () => {
    const d = deferred<CodeAction[]>();
    const v = mount(() => d.promise);
    pressMenuKey(v);
    v.dispatch({ selection: { anchor: 3 } });
    d.resolve([action("Do the thing")]);
    await flush();
    expect(menuShown(v)).toBe(false);
  });
});
