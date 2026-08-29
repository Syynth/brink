/**
 * Async compile landings in the diagnostics extension (W2a of
 * docs/editor-worker-spec.md): a promise-returning `compile` lands its
 * diagnostics when it resolves — unless the doc moved (a newer compile
 * is scheduled and lands instead), the view detached, or the plugin was
 * destroyed. The sync path is untouched (embedding hosts may still pass
 * a synchronous compile).
 */

import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { EditorView } from "@codemirror/view";
import { EditorState } from "@codemirror/state";
import { forEachDiagnostic } from "@codemirror/lint";
import type { CompileResult } from "@brink/wasm-types";
import { diagnosticsExtension, refreshDiagnosticsEffect } from "../diagnostics.js";

interface Deferred {
  promise: Promise<CompileResult>;
  resolve(result: CompileResult): void;
  reject(error: Error): void;
}

function deferred(): Deferred {
  let resolve!: Deferred["resolve"];
  let reject!: Deferred["reject"];
  const promise = new Promise<CompileResult>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function resultWithError(message: string): CompileResult {
  return {
    warnings: [
      {
        file: "main.ink",
        start: 0,
        end: 1,
        severity: "Error",
        message,
        code: "E001",
      },
    ],
  } as unknown as CompileResult;
}

function diagnosticsIn(view: EditorView): string[] {
  const messages: string[] = [];
  forEachDiagnostic(view.state, (d) => messages.push(d.message));
  return messages;
}

describe("diagnosticsExtension with an async compile", () => {
  let view: EditorView | null = null;
  let compiles: Deferred[];
  let onCompileResults: CompileResult[];

  beforeEach(() => {
    vi.useFakeTimers();
    compiles = [];
    onCompileResults = [];
  });

  afterEach(() => {
    view?.destroy();
    view = null;
    vi.useRealTimers();
  });

  function mount(doc = "hello\n"): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [
          diagnosticsExtension({
            compile: () => {
              const d = deferred();
              compiles.push(d);
              return d.promise;
            },
            getActiveFile: () => "main.ink",
            onCompile: (r) => onCompileResults.push(r),
          }),
        ],
      }),
      parent: document.body, // isConnected must hold for doCompile to run
    });
    return view;
  }

  it("lands the resolved compile as lint diagnostics", async () => {
    const v = mount();
    await vi.advanceTimersByTimeAsync(500);
    expect(compiles).toHaveLength(1);
    compiles[0]!.resolve(resultWithError("bad knot"));
    await vi.advanceTimersByTimeAsync(0);
    expect(diagnosticsIn(v)).toEqual(["bad knot"]);
    expect(onCompileResults).toHaveLength(1);
  });

  it("skips a landing whose doc moved on, then lands the fresh compile", async () => {
    const v = mount();
    await vi.advanceTimersByTimeAsync(500);
    expect(compiles).toHaveLength(1);
    // The doc changes while compile #1 is in flight — its landing must be
    // discarded (offsets belong to the old doc; #2 is already scheduled).
    v.dispatch({ changes: { from: 0, to: 0, insert: "x" } });
    compiles[0]!.resolve(resultWithError("stale landing"));
    await vi.advanceTimersByTimeAsync(0);
    expect(diagnosticsIn(v)).toEqual([]);
    expect(onCompileResults).toHaveLength(0);
    await vi.advanceTimersByTimeAsync(500);
    expect(compiles).toHaveLength(2);
    compiles[1]!.resolve(resultWithError("fresh landing"));
    await vi.advanceTimersByTimeAsync(0);
    expect(diagnosticsIn(v)).toEqual(["fresh landing"]);
  });

  it("drops a landing after the view is destroyed, without throwing", async () => {
    mount();
    await vi.advanceTimersByTimeAsync(500);
    expect(compiles).toHaveLength(1);
    view!.destroy();
    view = null;
    compiles[0]!.resolve(resultWithError("posthumous"));
    await vi.advanceTimersByTimeAsync(0);
    expect(onCompileResults).toHaveLength(0);
  });

  it("swallows a rejected compile (superseded/cancelled) silently", async () => {
    const v = mount();
    await vi.advanceTimersByTimeAsync(500);
    compiles[0]!.reject(new Error("dropped:cancelled"));
    await vi.advanceTimersByTimeAsync(0);
    expect(diagnosticsIn(v)).toEqual([]);
    expect(onCompileResults).toHaveLength(0);
  });

  it("still lands a synchronous compile unchanged", async () => {
    view = new EditorView({
      state: EditorState.create({
        doc: "hello\n",
        extensions: [
          diagnosticsExtension({
            compile: () => resultWithError("sync road"),
            getActiveFile: () => "main.ink",
          }),
        ],
      }),
      parent: document.body,
    });
    await vi.advanceTimersByTimeAsync(500);
    expect(diagnosticsIn(view)).toEqual(["sync road"]);
  });
});

/**
 * Re-publishing without a document change (#3260).
 *
 * The squiggles are published by this extension's ViewPlugin, which wakes on
 * `docChanged`. A compile that lands for some other reason — a `brink.toml`
 * edit changing `[lints]`, a suppression written into a sibling file — has
 * no document change in THIS view, so the squiggles stayed as they were.
 *
 * Reported as: suppress a diagnostic project-wide from the Problems panel,
 * watch it sit in the buffer until you type or reopen the file. "Even
 * editing the buffer updates it" was the clue that named the cause.
 */
describe("refreshDiagnosticsEffect", () => {
  let view: EditorView | null = null;
  let next: CompileResult;

  beforeEach(() => {
    vi.useFakeTimers();
    next = resultWithError("suppress me");
  });

  afterEach(() => {
    view?.destroy();
    view = null;
    vi.useRealTimers();
  });

  function mountSync(): EditorView {
    view = new EditorView({
      state: EditorState.create({
        doc: "hello\n",
        extensions: [
          diagnosticsExtension({
            compile: () => next,
            getActiveFile: () => "main.ink",
          }),
        ],
      }),
      parent: document.body,
    });
    return view;
  }

  it("re-publishes when the compile result changed but the document did not", async () => {
    const v = mountSync();
    await vi.advanceTimersByTimeAsync(500);
    expect(diagnosticsIn(v)).toEqual(["suppress me"]);

    // The suppression lands in `brink.toml`, not in this buffer.
    next = { ok: true, warnings: [] } as CompileResult;
    v.dispatch({ effects: refreshDiagnosticsEffect.of() });
    await vi.advanceTimersByTimeAsync(500);

    expect(diagnosticsIn(v)).toEqual([]);
  });

  it("does nothing without the effect — the bug, stated", async () => {
    // Same setup, no refresh: the stale squiggle survives, which is exactly
    // what an author saw after suppressing a code project-wide.
    const v = mountSync();
    await vi.advanceTimersByTimeAsync(500);
    expect(diagnosticsIn(v)).toEqual(["suppress me"]);

    next = { ok: true, warnings: [] } as CompileResult;
    await vi.advanceTimersByTimeAsync(500);

    expect(diagnosticsIn(v)).toEqual(["suppress me"]);
  });

  it("an ordinary transaction does not trigger a re-publish", async () => {
    // The effect is the signal, not "any transaction" — a selection change
    // must not spend a compile.
    const v = mountSync();
    await vi.advanceTimersByTimeAsync(500);
    let compiles = 0;
    const counted = new EditorView({
      state: EditorState.create({
        doc: "hello\n",
        extensions: [
          diagnosticsExtension({
            compile: () => {
              compiles += 1;
              return next;
            },
            getActiveFile: () => "main.ink",
          }),
        ],
      }),
      parent: document.body,
    });
    await vi.advanceTimersByTimeAsync(500);
    const before = compiles;
    counted.dispatch({ selection: { anchor: 1 } });
    await vi.advanceTimersByTimeAsync(500);
    expect(compiles).toBe(before);
    counted.destroy();
  });
});
