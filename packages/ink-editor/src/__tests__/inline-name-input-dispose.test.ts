/**
 * `InlineNameInput.dispose()` must clear every timer/handle it owns (#2557).
 *
 * Before #2557, `dispose()` only cleared the debounce timer (`this.timer`)
 * and the idle-query handle — the two deferred `setTimeout(…, 0)` handles set
 * up by `render()` (the post-mount focus timer) and `renderReport()` (the
 * force-button focus timer) were left running. That was latent rather than
 * an observed bug, because `dispose()` detaches the widget's DOM (`root.remove()`)
 * *before* either callback fires, and `focus()` on a detached node is a
 * silent no-op — so nothing visibly broke, but the class doc's own claim
 * ("tears them all down") was false, and a query/focus callback still ran
 * after the controller considered itself torn down.
 *
 * `packages/brink-studio/src/__tests__/inline-name-input-seed.test.ts`
 * exercises the *seed* value/select behavior of the post-mount focus timer
 * with the same fake-timer technique, but does not call `dispose()` — this
 * suite is the one that pins the #2557 regression itself, and it is exactly
 * the test the issue's build report (#2559) says was awkward to place in the
 * studio suite (its `vi.useFakeTimers` sequencing there interacted with that
 * suite's own setup and went red in CI, fixed in `e8410d2f7`). A same-package
 * suite is the natural home.
 *
 * Imports the module directly (`../inline-name-input.js`), not through the
 * package's `index.ts` barrel or the `@brink-lang/editor` specifier — see
 * `vitest.config.ts`'s header comment for why this suite avoids the wasm
 * dependency chain that barrel would pull in.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import type { StructuralResult } from "@brink/wasm-types";
import { InlineNameInput, type InlineNameInputOptions } from "../inline-name-input.js";

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

describe("InlineNameInput.dispose() clears its timers (#2557)", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
    document.body.replaceChildren();
  });

  it("clears the post-mount focus timer before it fires", () => {
    const widget = makeInput();
    const root = widget.render();
    document.body.append(root);

    // render() schedules exactly one setTimeout(…, 0): the post-mount focus
    // timer. Nothing has run yet under fake timers.
    expect(vi.getTimerCount()).toBe(1);

    widget.dispose();

    // The regression: before #2557 this timer survived dispose() and would
    // still fire on the next tick.
    expect(vi.getTimerCount()).toBe(0);

    // Advancing time must not throw or run any lingering callback against
    // the now-torn-down widget.
    expect(() => vi.runAllTimers()).not.toThrow();
  });

  it("clears an in-flight debounced query and its deferred idle callback, so the query never runs", () => {
    const query = vi.fn<(name: string) => StructuralResult | null>(() => unsafe(1));
    const widget = makeInput({ query });
    const root = widget.render();
    document.body.append(root);

    const input = root.querySelector<HTMLInputElement>(".brink-inline-rename-input");
    expect(input).not.toBeNull();
    if (input === null) throw new Error("input not mounted");

    input.value = "goodbye";
    input.dispatchEvent(new Event("input"));

    // Two timers now pending: the post-mount focus timer (0ms, from render())
    // and the live-query debounce timer (250ms, from the input event).
    expect(vi.getTimerCount()).toBe(2);

    // Run only the timers that are CURRENTLY pending — not any new ones a
    // callback schedules while running. This fires both of the above (the
    // debounce settle calls `beginQuery`, which schedules a fresh idle/
    // fallback `setTimeout` for the actual wasm-shaped call) without letting
    // that new timer run yet, so we can assert dispose() reaches it too.
    vi.runOnlyPendingTimers();
    expect(vi.getTimerCount()).toBe(1); // the deferred query call, still pending
    expect(query).not.toHaveBeenCalled();

    widget.dispose();
    expect(vi.getTimerCount()).toBe(0);

    // Advance past everything: if dispose() had left the deferred query
    // handle uncancelled, this is where it would have run.
    vi.runAllTimers();
    expect(query).not.toHaveBeenCalled();
  });

  it("clears the report's force-focus timer once a report is open", () => {
    const result = unsafe(2);
    const widget = makeInput({ query: () => result });
    const root = widget.render();
    document.body.append(root);

    // Neutralize the post-mount focus timer up front — it's the subject of
    // the first test above, and calling the REAL `HTMLElement.focus()` it
    // invokes has its own jsdom-internal side effect under fake timers (a
    // macrotask jsdom schedules for its own focus-event bookkeeping) that
    // would otherwise show up as an extra, unrelated entry in
    // `vi.getTimerCount()` and make this test's count assertions ambiguous
    // about which timer is which. This test is about the force-focus timer
    // specifically, so keep the two independent.
    const internals = widget as unknown as { focusTimer: ReturnType<typeof setTimeout> | null };
    if (internals.focusTimer !== null) clearTimeout(internals.focusTimer);
    internals.focusTimer = null;
    expect(vi.getTimerCount()).toBe(0);

    // Change the value (Enter on the unchanged seed value just cancels — see
    // `confirm()`'s early-return) then confirm: no cached result exists yet,
    // so this requests a commit and kicks off the deferred query
    // (`beginQuery`) rather than calling `query` inline.
    const input = root.querySelector<HTMLInputElement>(".brink-inline-rename-input");
    if (input === null) throw new Error("input not mounted");
    input.value = "goodbye";
    input.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter" }));
    expect(vi.getTimerCount()).toBe(1); // just the deferred query (beginQuery's idle/fallback timer)

    // Fire it: resolves the query and, since the result is unsafe, opens the
    // report — which schedules its OWN force-focus timer. `query` itself
    // never touches focus/DOM, so this step introduces no side timer.
    vi.advanceTimersToNextTimer();

    const report = root.querySelector<HTMLElement>(".brink-inline-rename-report");
    expect(report?.hidden).toBe(false);
    const force = report?.querySelector<HTMLButtonElement>(".brink-inline-rename-force");
    expect(force).not.toBeNull();
    if (force === undefined || force === null) throw new Error("force button not rendered");
    const focusSpy = vi.spyOn(force, "focus");

    // The report just opened; its force-focus timer is the only thing
    // pending, and it has not fired yet (`force.focus()` was never called).
    expect(vi.getTimerCount()).toBe(1);
    expect(focusSpy).not.toHaveBeenCalled();

    widget.dispose();
    expect(vi.getTimerCount()).toBe(0);

    // The regression: before #2557, dispose() left this timer running, and
    // it would still focus the (by-then-detached) force button on the next
    // tick.
    vi.runAllTimers();
    expect(focusSpy).not.toHaveBeenCalled();
  });
});
