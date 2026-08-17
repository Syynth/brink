/**
 * `InlineNameInput` Escape dismissal (#279 audit gap).
 *
 * The #279 audit table claimed every transient surface in `ink-editor` and
 * `studio-ui`/`studio-shell` was either already correct or fixed by this PR
 * — but missed `InlineNameInput` (the shared F2-rename / extract-to-knot
 * inline prompt) entirely. It has the exact bug shape #279 names:
 *
 *  1. Escape is handled only by `input.addEventListener("keydown", this.keyHandler)`
 *     — element-scoped to the `<input>`, not `document`. Any Escape dispatched
 *     outside that one element's subtree never reaches it.
 *  2. Worse: when the breakage report is showing, the code deliberately moves
 *     focus to the "force" override button (`renderReport()`'s
 *     `forceFocusTimer`) — and that button lives in `report`, a SIBLING
 *     subtree of `input` (`row.append(input, badge); root.append(row, report)`).
 *     A keydown dispatched at (or bubbling from) `report`/`force` never
 *     passes through `input` at all, so in that state Escape did *nothing
 *     whatsoever* — a live, unescapable surface, exactly what #279 reported,
 *     sitting inside the very packages the audit claims to have swept.
 *
 * The fix: `render()` registers `() => this.cancel()` with the global dismiss
 * net (dismiss-registry.ts), unregistered from the shared `close()` teardown
 * behind both `cancel()` and `commit()` (and defensively in `dispose()`).
 * Both cases below open the report and focus the force button for real (no
 * mocking of `query`/timers beyond fake-timer sequencing), then dispatch
 * Escape from a target that is provably outside `input`'s own subtree.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import type { StructuralResult } from "@brink/wasm-types";
import { InlineNameInput, type InlineNameInputOptions } from "../inline-name-input.js";
import { resetDismissRegistryForTests } from "../dismiss-registry.js";

const unsafe = (n: number): StructuralResult => ({
  ok: true,
  path: "main.ink",
  new_source: "=== hello ===\n",
  cross_file_edits: [{ path: "other.ink", new_source: "-> hello\n" }],
  introduced_diagnostics: Array.from({ length: n }, (_, i) => ({
    severity: "error" as const,
    code: "E022",
    message: `unresolved divert ${i}`,
    path: "other.ink",
    line: i + 1,
    col: 1,
  })),
  safe: false,
});

function makeInput(
  overrides: Partial<InlineNameInputOptions> = {},
  onClose: () => void = () => {},
): InlineNameInput {
  const options: InlineNameInputOptions = {
    initialValue: "hello",
    ariaLabel: "Rename hello",
    forceLabel: "Rename anyway",
    query: () => unsafe(1),
    onCommit: () => {},
    liveBadge: true,
    ...overrides,
  };
  return new InlineNameInput(options, onClose);
}

describe("InlineNameInput Escape safety net (#279)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetDismissRegistryForTests();
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
    resetDismissRegistryForTests();
  });

  it("Escape dismisses even when focus is on the breakage report's force button — a sibling subtree of input", () => {
    const onClose = vi.fn();
    const widget = makeInput({ query: () => unsafe(2) }, onClose);
    const root = widget.render();
    document.body.append(root);

    // Settle the post-mount focus timer (render()'s own deferred focus/select).
    vi.runOnlyPendingTimers();

    const input = root.querySelector<HTMLInputElement>(".brink-inline-rename-input");
    if (input === null) throw new Error("input not mounted");
    input.value = "goodbye";
    // Enter with no cached result requests a commit and kicks off the
    // deferred query (beginQuery), same as a live debounce settle.
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));

    // Fire the deferred query: resolves unsafe -> settleCommit -> renderReport()
    // (which schedules its OWN forceFocusTimer, not yet pending at this point).
    vi.runOnlyPendingTimers();
    // Fire the report's force-focus timer.
    vi.runOnlyPendingTimers();

    const report = root.querySelector<HTMLElement>(".brink-inline-rename-report");
    expect(report?.hidden).toBe(false);
    const force = report?.querySelector<HTMLButtonElement>(".brink-inline-rename-force");
    expect(force).not.toBeNull();
    if (force === undefined || force === null) throw new Error("force button not rendered");
    expect(document.activeElement).toBe(force);

    expect(onClose).not.toHaveBeenCalled();
    // Dispatched at `force` itself — proves the path this event travels
    // (force -> report -> root -> body -> ... -> document -> window) never
    // passes through `input`, so `input`'s own element-scoped keyHandler
    // structurally cannot be what closes this.
    force.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("Escape dispatched at document (outside the widget entirely) still dismisses", () => {
    const onClose = vi.fn();
    const widget = makeInput({}, onClose);
    const root = widget.render();
    document.body.append(root);
    vi.runOnlyPendingTimers();

    expect(onClose).not.toHaveBeenCalled();
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("unregisters from the dismiss net on close, so a later Escape does not call cancel() on a torn-down widget", () => {
    const onCancel = vi.fn();
    const onClose = vi.fn();
    const widget = makeInput({ onCancel }, onClose);
    const root = widget.render();
    document.body.append(root);
    vi.runOnlyPendingTimers();

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);

    // A second Escape after the widget already closed itself must not
    // re-invoke onCancel/onClose — proves close() actually unregistered.
    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("dispose() also unregisters from the dismiss net, so a widget torn down without cancel()/commit() cannot be reached by a later Escape", () => {
    const onClose = vi.fn();
    const widget = makeInput({}, onClose);
    const root = widget.render();
    document.body.append(root);
    vi.runOnlyPendingTimers();

    widget.dispose();

    document.dispatchEvent(
      new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }),
    );
    // dispose() does not call onClose() itself (it is a distinct teardown
    // path from cancel()/commit()) — this only asserts the registry entry
    // was cleaned up, i.e. no stray callback fires.
    expect(onClose).not.toHaveBeenCalled();
  });
});
