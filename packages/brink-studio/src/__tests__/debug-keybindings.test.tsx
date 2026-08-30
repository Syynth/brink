/**
 * W10/#3303 — the F-row transport keybindings (spec §F3's table, RULED
 * desktop-first; user-remappable through the keymap overrides) and the
 * status bar's paused state.
 */
import { describe, expect, it, vi } from "vitest";
import { act, createElement } from "react";
import { createRoot } from "react-dom/client";
import {
  CommandRegistry,
  Keymap,
  ShellProvider,
  attachKeyHandler,
  createEditorGroupsStore,
  createShellLayoutStore,
  parseKeybinding,
} from "@brink/studio-shell";
import { StorySegment, StoreProvider } from "@brink/studio-ui";
import { createStudioStore } from "@brink/studio-store";
import { registerDebugCommands } from "../debug-commands";
import { registerStoryCommands } from "../story-commands";

function keymapFor(store = createStudioStore()) {
  const commands = new CommandRegistry();
  registerDebugCommands(commands, store, () => "main.ink");
  registerStoryCommands(commands, store);
  return { commands, keymap: Keymap.fromCommands(commands.list(), {}), store };
}

function resolves(keymap: Keymap, binding: string): string | undefined {
  const chord = parseKeybinding(binding);
  expect(chord, binding).not.toBeNull();
  return keymap.resolveChord(chord!);
}

describe("debug transport keybindings (W10/#3303)", () => {
  it("the spec §F3 table binds verbatim", () => {
    const { keymap } = keymapFor();
    expect(resolves(keymap, "F5")).toBe("debug.continue");
    expect(resolves(keymap, "F6")).toBe("debug.pause");
    expect(resolves(keymap, "F9")).toBe("debug.toggleBreakpoint");
    expect(resolves(keymap, "F10")).toBe("debug.stepOver");
    expect(resolves(keymap, "F11")).toBe("debug.stepInto");
    expect(resolves(keymap, "Shift-F11")).toBe("debug.stepOut");
    expect(resolves(keymap, "Shift-F5")).toBe("story.restart");
  });

  it("F9 toggles a breakpoint at the focused file's cursor line (0-based)", () => {
    const store = createStudioStore();
    const toggle = vi.fn();
    store.setState({ breakpointToggleAtLine: toggle, cursor: { line: 12, col: 3 } });
    const commands = new CommandRegistry();
    registerDebugCommands(commands, store, () => "scenes/intro.ink");

    expect(commands.dispatch("debug.toggleBreakpoint")).toBe(true);
    expect(toggle).toHaveBeenCalledWith("scenes/intro.ink", 11);
  });

  it("F9 is gated on a focused ink file, not on debug capability", () => {
    const store = createStudioStore();
    const commands = new CommandRegistry();
    registerDebugCommands(commands, store, () => null);
    expect(commands.isEnabled("debug.toggleBreakpoint")).toBe(false);
  });

  it("a function key fires from an editable target (the keyhandler exemption)", () => {
    const store = createStudioStore();
    const run = vi.fn();
    const commands = new CommandRegistry();
    commands.register({ id: "debug.continue", title: "c", keybinding: "F5", run });
    const keymap = Keymap.fromCommands(commands.list(), {});
    const dispose = attachKeyHandler(window, commands, keymap, { isMac: true });

    const input = document.createElement("input");
    document.body.appendChild(input);
    input.focus();
    input.dispatchEvent(
      new KeyboardEvent("keydown", { key: "F5", bubbles: true, cancelable: true }),
    );
    expect(run).toHaveBeenCalled();
    dispose();
    input.remove();
  });
});

describe("status bar paused state (W10/#3303)", () => {
  it("a paused session reads 'paused' with the warning dot", () => {
    const store = createStudioStore();
    store.setState({ sessionStatus: "running", sessionPaused: true });
    const host = document.createElement("div");
    document.body.appendChild(host);
    const root = createRoot(host);
    act(() => {
      root.render(
        createElement(
          ShellProvider,
          {
            commands: new CommandRegistry(),
            editorGroups: createEditorGroupsStore(),
            layout: createShellLayoutStore(),
          } as never,
          createElement(StoreProvider, { store } as never, createElement(StorySegment)),
        ),
      );
    });
    expect(host.textContent).toContain("paused");
    expect(host.querySelector(".brink-status-story-dot.status-paused")).not.toBeNull();
    act(() => root.unmount());
    host.remove();
  });
});
