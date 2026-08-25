/**
 * `deferredRefresh` with an async `prepare` (W2b of
 * docs/editor-worker-spec.md): the quiet-fire runs the warm-up first and
 * dispatches the refresh effect only when it settles — skipping the
 * dispatch if the doc moved during the prepare (that change re-armed the
 * timer) or the plugin was torn down, and dispatching anyway on a
 * rejected prepare (superseded by a sibling's identical pull, or a
 * genuine failure — either way the field's sync fallback is correct).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { StateEffect, StateField } from "@codemirror/state";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { DEFER_LINE_THRESHOLD, deferredRefresh } from "../deferred-refresh.js";

const refreshEffect = StateEffect.define<void>();

/** Counts refresh-effect arrivals — the observable the guards act on. */
const refreshCount = StateField.define<number>({
  create: () => 0,
  update: (value, tr) =>
    tr.effects.some((e) => e.is(refreshEffect)) ? value + 1 : value,
});

const LARGE_DOC = "line\n".repeat(DEFER_LINE_THRESHOLD + 5);

interface Deferred {
  promise: Promise<unknown>;
  resolve(): void;
  reject(): void;
}

function deferred(): Deferred {
  let resolve!: () => void;
  let reject!: () => void;
  const promise = new Promise<void>((res, rej) => {
    resolve = res;
    reject = () => rej(new Error("dropped:superseded"));
  });
  promise.catch(() => undefined); // avoid unhandled-rejection noise
  return { promise, resolve, reject };
}

describe("deferredRefresh with prepare", () => {
  let view: EditorView | null = null;
  let prepares: Deferred[];

  beforeEach(() => {
    vi.useFakeTimers();
    prepares = [];
  });

  afterEach(() => {
    view?.destroy();
    view = null;
    vi.useRealTimers();
  });

  function mount(prepare?: () => Promise<unknown> | undefined): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc: LARGE_DOC,
        extensions: [
          refreshCount,
          deferredRefresh(
            refreshEffect,
            120,
            prepare ??
              (() => {
                const d = deferred();
                prepares.push(d);
                return d.promise;
              }),
          ),
        ],
      }),
      parent: document.body,
    });
    return view;
  }

  function type(v: EditorView): void {
    v.dispatch({ changes: { from: 0, to: 0, insert: "x" } });
  }

  it("dispatches the refresh only after the prepare resolves", async () => {
    const v = mount();
    type(v);
    await vi.advanceTimersByTimeAsync(120);
    expect(prepares).toHaveLength(1);
    expect(v.state.field(refreshCount)).toBe(0); // warm-up still in flight
    prepares[0]!.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(v.state.field(refreshCount)).toBe(1);
  });

  it("skips a landing whose doc moved during the prepare (a fresh fire follows)", async () => {
    const v = mount();
    type(v);
    await vi.advanceTimersByTimeAsync(120);
    expect(prepares).toHaveLength(1);
    type(v); // re-arms the timer, invalidates the in-flight prepare
    prepares[0]!.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(v.state.field(refreshCount)).toBe(0);
    await vi.advanceTimersByTimeAsync(120);
    expect(prepares).toHaveLength(2);
    prepares[1]!.resolve();
    await vi.advanceTimersByTimeAsync(0);
    expect(v.state.field(refreshCount)).toBe(1);
  });

  it("dispatches anyway when the prepare rejects (sync fallback, never stranded)", async () => {
    const v = mount();
    type(v);
    await vi.advanceTimersByTimeAsync(120);
    prepares[0]!.reject();
    await vi.advanceTimersByTimeAsync(0);
    expect(v.state.field(refreshCount)).toBe(1);
  });

  it("never dispatches into a destroyed view", async () => {
    const v = mount();
    type(v);
    await vi.advanceTimersByTimeAsync(120);
    expect(prepares).toHaveLength(1);
    v.destroy();
    view = null;
    prepares[0]!.resolve();
    await vi.advanceTimersByTimeAsync(0); // must not throw
  });

  it("skips the warm-up entirely when prepare returns undefined", async () => {
    const v = mount(() => undefined);
    type(v);
    await vi.advanceTimersByTimeAsync(120);
    expect(v.state.field(refreshCount)).toBe(1);
  });
});
