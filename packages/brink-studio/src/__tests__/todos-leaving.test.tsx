/**
 * TODOs exit animation (#3050): a note removed from source lingers as a
 * struck-through `.is-leaving` row for the leave window, then drops out.
 * Also pins that identity-only churn (a recompile delivering an equal but
 * new diagnostics array) neither strikes rows nor cancels a pending drop.
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { act, createElement } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
} from "@brink/studio-shell";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import { StoreProvider, TodosView, TODO_DIAGNOSTIC_CODE } from "@brink/studio-ui";
import type { Diagnostic } from "@brink/wasm-types";

let root: Root | null = null;
let container: HTMLElement | null = null;

afterEach(() => {
  act(() => root?.unmount());
  container?.remove();
  root = null;
  container = null;
  vi.useRealTimers();
});

function todoDiag(message: string, start = 0): Diagnostic {
  return {
    file: "a.ink",
    start,
    end: start + 4,
    message,
    severity: "Info",
    code: TODO_DIAGNOSTIC_CODE,
  };
}

function mountTodos(diagnostics: Diagnostic[]): StudioStore {
  const store = createStudioStore();
  store.setState({ diagnosticsList: diagnostics, outline: [] });
  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  const commands = new CommandRegistry();
  const themes = new ThemeService();
  const overrides = new KeymapOverridesService();
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        { commands, themes, keymapOverrides: overrides, isMac: true } as never,
        createElement(StoreProvider, { store } as never, createElement(TodosView)),
      ),
    );
  });
  return store;
}

function rows(): string[] {
  return [...container!.querySelectorAll(".todos-row .todos-text")].map(
    (e) => e.textContent ?? "",
  );
}

function leavingRows(): number {
  return container!.querySelectorAll(".todos-row.is-leaving").length;
}

describe("TODOs exit animation", () => {
  it("a removed note lingers struck-through, then drops after the window", () => {
    vi.useFakeTimers();
    const store = mountTodos([todoDiag("TODO: keep"), todoDiag("TODO: cut this", 50)]);
    expect(rows()).toEqual(["keep", "cut this"]);

    act(() => {
      store.setState({ diagnosticsList: [todoDiag("TODO: keep")] });
    });
    expect(rows()).toEqual(["keep", "cut this"]);
    expect(leavingRows()).toBe(1);

    act(() => {
      vi.advanceTimersByTime(1500);
    });
    expect(rows()).toEqual(["keep"]);
    expect(leavingRows()).toBe(0);
  });

  it("identity-only churn neither strikes rows nor cancels a pending drop", () => {
    vi.useFakeTimers();
    const store = mountTodos([todoDiag("TODO: keep"), todoDiag("TODO: cut this", 50)]);

    act(() => {
      store.setState({ diagnosticsList: [todoDiag("TODO: keep")] });
    });
    expect(leavingRows()).toBe(1);

    // A recompile delivers an equal-but-new array mid-window.
    act(() => {
      vi.advanceTimersByTime(400);
      store.setState({ diagnosticsList: [todoDiag("TODO: keep")] });
    });
    expect(leavingRows()).toBe(1);

    act(() => {
      vi.advanceTimersByTime(1500);
    });
    expect(rows()).toEqual(["keep"]);
    expect(leavingRows()).toBe(0);
  });
});
