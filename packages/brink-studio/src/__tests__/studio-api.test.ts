/**
 * StudioApi facade tests (shell issue 5.4 / #95, spec §8.2): the curated
 * host-facing API — insertText through the focused-view path, command
 * dispatch, notify, and select/subscribe over the explicit versioned
 * StudioPublicState (derived from the store; reference-stable between
 * relevant changes; unrelated store traffic never fires subscribers).
 */

import { describe, expect, it, vi } from "vitest";
import { CommandRegistry, NotificationCenter } from "@brink/studio-shell";
import {
  createStudioStore,
  ElementTypeEnum,
  type DocumentSessions,
} from "@brink/studio-store";
import { createStudioApi, derivePublicState } from "@brink/studio-ui";

function harness() {
  const store = createStudioStore();
  const commands = new CommandRegistry();
  const notifications = new NotificationCenter();
  const api = createStudioApi({ store, commands, notifications });
  return { store, commands, notifications, api };
}

describe("StudioApi.insertText", () => {
  it("delegates to the focused-view insertion path", () => {
    const { store, api } = harness();
    const insertAtCursor = vi.fn();
    store.setState({ _documents: { insertAtCursor } as unknown as DocumentSessions });
    api.insertText("EXTERNAL has(item)\n");
    expect(insertAtCursor).toHaveBeenCalledWith("EXTERNAL has(item)\n");
  });

  it("is a no-op before documents are bound", () => {
    const { api } = harness();
    expect(() => api.insertText("x")).not.toThrow();
  });
});

describe("StudioApi.dispatch / notify", () => {
  it("dispatches registered commands with args", () => {
    const { commands, api } = harness();
    const run = vi.fn();
    commands.register({ id: "editor.reveal", title: "Reveal", run });
    expect(api.dispatch("editor.reveal", { kind: "symbol", name: "intro" })).toBe(true);
    expect(run).toHaveBeenCalledWith({ kind: "symbol", name: "intro" });
    expect(api.dispatch("nope")).toBe(false);
  });

  it("notify reaches the notification center and returns a live handle", () => {
    const { notifications, api } = harness();
    const handle = api.notify({ severity: "info", message: "inserted has(item)" });
    expect(notifications.getState().visible.map((n) => n.message)).toContain(
      "inserted has(item)",
    );
    handle.dismiss();
    expect(notifications.getState().visible).toHaveLength(0);
  });
});

describe("StudioPublicState (select)", () => {
  it("exposes the versioned subset, derived from the store", () => {
    const { store, api } = harness();
    store.getState().setActiveDocKey("main.ink::start");
    store.getState().setCursor(7, 3);
    store
      .getState()
      .setLineInfo(
        { type: ElementTypeEnum.KnotHeader, depth: 1, sticky: false, standalone: false },
        [],
      );
    store.getState().setCompileResult([], { errors: 2, warnings: 1 }, [], null);

    expect(api.select((s) => s)).toEqual({
      version: 1,
      activeFile: "main.ink", // doc key "main.ink::start" → file path
      cursor: { line: 7, col: 3 },
      element: { type: "KnotHeader", depth: 1 },
      diagnostics: { errors: 2, warnings: 1 },
      compileStatus: "errors",
      sessionStatus: "none",
      dirtyFiles: 0,
    });
  });

  it("reports ok/no-file/no-element defaults", () => {
    const { api } = harness();
    const s = api.select((s) => s);
    expect(s.activeFile).toBeNull();
    expect(s.element).toBeNull();
    expect(s.compileStatus).toBe("ok");
    expect(s.sessionStatus).toBe("none");
  });

  it("is reference-stable across unrelated store changes", () => {
    const { store, api } = harness();
    const before = api.select((s) => s);
    store.getState().appendOutput("compile", "noise"); // not a public input
    expect(api.select((s) => s)).toBe(before);
    store.getState().setCursor(2, 2); // a public input
    expect(api.select((s) => s)).not.toBe(before);
  });
});

describe("StudioPublicState (subscribe)", () => {
  it("fires on selected changes only; unsubscribe stops it", () => {
    const { store, api } = harness();
    const seen: number[] = [];
    const unsubscribe = api.subscribe(
      (s) => s.cursor.line,
      (line) => seen.push(line),
    );

    store.getState().setCursor(5, 1);
    store.getState().setCursor(5, 9); // same line — selected value unchanged
    store.getState().appendOutput("compile", "noise"); // unrelated
    store.getState().setCursor(8, 1);
    expect(seen).toEqual([5, 8]);

    unsubscribe();
    store.getState().setCursor(9, 1);
    expect(seen).toEqual([5, 8]);
  });
});

describe("derivePublicState", () => {
  it("maps a plain file doc key and element names", () => {
    const { store } = harness();
    store.getState().setActiveDocKey("toppled-temple.ink");
    store
      .getState()
      .setLineInfo(
        { type: ElementTypeEnum.Choice, depth: 2, sticky: true, standalone: false },
        [],
      );
    const s = derivePublicState(store.getState());
    expect(s.activeFile).toBe("toppled-temple.ink");
    expect(s.element).toEqual({ type: "Choice", depth: 2 });
  });
});
