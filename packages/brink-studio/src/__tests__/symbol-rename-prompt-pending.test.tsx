/**
 * `SymbolRenamePrompt` off-the-paint-path ordering (#696, mirroring #722).
 *
 * `e2e/symbol-rename.spec.ts`'s "a colliding rename shows the breakage
 * report; Force overrides" flaked repeatedly (original run 29267160302;
 * recurred on PR #1500 and again on PR #1888, where it was the sole blocker
 * keeping that PR from merging). PR #714's fix widened the assertion's
 * timeout — a numeric band-aid that reduced but never eliminated the
 * recurrence, because the underlying defect was never about a missing
 * timeout: `performSymbolRename`'s collision analysis
 * (`EditorSession.rename_symbol`, a synchronous wasm call) used to run
 * inline in the Enter/Force handler's own frame, with no yield point of its
 * own, so React could never paint an intermediate state before it — any
 * fixed budget was racing an unbounded, host-load-dependent block.
 *
 * #722 fixed this exact defect for the sibling INLINE rename widget
 * (`InlineNameInput`) by committing a "pending" indicator synchronously (so
 * a paint lands first) and deferring the heavy call to the next idle slot
 * via `scheduleIdleWork`. It never touched this MODAL prompt — the surface
 * the flaky e2e test actually drives — which is why the flake outlived it.
 * `SymbolRenamePrompt` now takes the same discipline (see its OFF THE PAINT
 * PATH doc comment).
 *
 * This suite proves the fix DETERMINISTICALLY rather than by racing a live
 * browser: `scheduleIdleWork` falls back to `setTimeout` in jsdom (no
 * `requestIdleCallback` here), so with fake timers nothing resembling the
 * heavy call can run until the test explicitly advances them. Asserting the
 * pending indicator is already in the DOM *before that advance* is an
 * ordering proof, not a timing one — it cannot flake on how long the
 * analysis itself takes, unlike the e2e assertion it stands in for.
 */

import { describe, expect, it, afterEach, beforeEach, vi } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  BUILTIN_THEMES,
  CommandRegistry,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
} from "@brink/studio-shell";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { initWasm } from "@brink-lang/web";
import {
  createStudioStore,
  type DocumentSessions as StoreDocs,
  type StudioStore,
} from "@brink/studio-store";
import { StoreProvider, SymbolRenamePrompt } from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

// Renaming `a` onto the existing top-level knot `b` collides (E022) — the
// same fixture shape `symbol-rename.test.ts` uses for the identical check
// against `performSymbolRename` directly, one layer below the UI this suite
// drives.
const COLLIDING = "-> a\n=== a ===\n-> END\n=== b ===\n-> END\n";

function stubDocuments(): StoreDocs {
  return {
    invalidateFile: vi.fn(),
    triggerCompile: vi.fn(),
  } as unknown as StoreDocs;
}

function memoryStorage(): Storage {
  const map = new Map<string, string>();
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    getItem: (key: string) => map.get(key) ?? null,
    key: (index: number) => [...map.keys()][index] ?? null,
    removeItem: (key: string) => void map.delete(key),
    setItem: (key: string, value: string) => void map.set(key, value),
  } as Storage;
}

let container: HTMLDivElement | null = null;
let root: Root | null = null;

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  if (root) act(() => root!.unmount());
  container?.remove();
  container = null;
  root = null;
  vi.useRealTimers();
});

/** Mount the prompt against a real (mock-wasm-backed) project session with an
 *  open rename request for `knot`. */
async function renderPrompt(knot: string): Promise<{ store: StudioStore; project: ProjectSession }> {
  await initWasm();
  const provider = new InMemoryFileProvider({ "main.ink": COLLIDING });
  const project = new ProjectSession({ provider, entryFile: "main.ink" });
  await project.initialize();

  const store = createStudioStore();
  store.setState({ _project: project, _documents: stubDocuments() });
  store.getState().openRenamePrompt({ path: "main.ink", knot, currentName: knot });

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);

  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        {
          commands: new CommandRegistry(),
          themes: new ThemeService(BUILTIN_THEMES, memoryStorage()),
          keymapOverrides: new KeymapOverridesService(memoryStorage()),
          isMac: false,
        } as never,
        createElement(StoreProvider, { store } as never, createElement(
          SymbolRenamePrompt,
        ) as ReactNode),
      ),
    );
  });

  return { store, project };
}

function input(): HTMLInputElement {
  const el = container?.querySelector("#brink-rename-input");
  expect(el, "the rename prompt mounted its name input").toBeInstanceOf(HTMLInputElement);
  return el as HTMLInputElement;
}

function pendingIndicator(): Element | null {
  return container?.querySelector(".brink-rename-pending") ?? null;
}

function report(): Element | null {
  return container?.querySelector(".brink-rename-report") ?? null;
}

describe("SymbolRenamePrompt off-the-paint-path pending state (#696)", () => {
  it("paints the pending indicator before the deferred collision analysis runs", async () => {
    const { project } = await renderPrompt("a");

    // Type the colliding name and press Enter — the real user path
    // `confirmName()` drives via `onKeyDown`.
    act(() => {
      const el = input();
      el.value = "b";
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });

    // No timer has been allowed to run yet, so `scheduleIdleWork`'s deferred
    // callback (jsdom's `setTimeout` fallback) has NOT fired — the heavy
    // wasm call could not possibly have executed. Yet the pending indicator
    // is already in the DOM: this is the ordering the #696/#722 fix
    // guarantees, proven by construction rather than by racing a clock.
    expect(pendingIndicator()).not.toBeNull();
    expect(report()).toBeNull();
    // And nothing was renamed yet — the deferred call hasn't run.
    expect(project.getSession().getFileSource("main.ink")).toBe(COLLIDING);

    // Now let the deferred call actually run.
    await act(async () => {
      await vi.runAllTimersAsync();
    });

    // The pending indicator clears and the breakage report lands, exactly as
    // it would have if the analysis had run inline — only the scheduling
    // changed.
    expect(pendingIndicator()).toBeNull();
    const reportEl = report();
    expect(reportEl).not.toBeNull();
    expect(reportEl!.textContent).toMatch(/would break/i);
  });

  it("shows the pending indicator again on Force, before the deferred apply runs", async () => {
    const { project } = await renderPrompt("a");

    act(() => {
      const el = input();
      el.value = "b";
      el.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    });
    await act(async () => {
      await vi.runAllTimersAsync();
    });
    expect(report()).not.toBeNull();

    act(() => {
      container!.querySelector<HTMLButtonElement>(".brink-rename-force")!.click();
    });

    // Same ordering guarantee on the apply path: pending paints before the
    // deferred (force) apply call runs.
    expect(pendingIndicator()).not.toBeNull();
    expect(project.getSession().getFileSource("main.ink")).toBe(COLLIDING); // not applied yet

    await act(async () => {
      await vi.runAllTimersAsync();
    });

    expect(pendingIndicator()).toBeNull();
    expect(project.getSession().getFileSource("main.ink")).not.toBe(COLLIDING);
  });
});
