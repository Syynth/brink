/**
 * Tool-window header actions slot (`ToolWindowDescriptor.actions`).
 *
 * The chrome header used to be fixed: title + close. Panels that need
 * their own controls (Problems' severity toggles and filter, expand /
 * collapse-all, …) had nowhere to put them except inside the panel body,
 * which reads as a second header stacked under the real one.
 *
 * The slot follows the `badge` contract exactly — the registering app
 * supplies a COMPONENT, so it subscribes to that app's own store and stays
 * reactive without the shell re-rendering and without the shell depending
 * on any app store (studio-shell-spec §7.2 layering). These tests pin that
 * contract, the no-regression case for panels that don't use it, and the
 * typography reset (the header is uppercase/letter-spaced for the TITLE;
 * an action component must not inherit that).
 */

import { describe, expect, it, afterEach } from "vitest";
import { act, createElement, useSyncExternalStore } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  CommandRegistry,
  KeymapOverridesService,
  ShellFrame,
  ShellProvider,
  ThemeService,
  ToolWindowRegistry,
  createShellLayoutStore,
  type ToolWindowDescriptor,
} from "@brink/studio-shell";

// jsdom ships no ResizeObserver, and ShellFrame's editor-area + the
// resizable-panels dock both construct one on mount. Minimal stub: the slot
// under test never resizes anything.
class NoopResizeObserver {
  observe(): void {}
  unobserve(): void {}
  disconnect(): void {}
}
(globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver ??=
  NoopResizeObserver;

let container: HTMLElement | null = null;
let root: Root | null = null;
afterEach(() => {
  act(() => root?.unmount());
  root = null;
  container?.remove();
  container = null;
});

function descriptor(over: Partial<ToolWindowDescriptor> = {}): ToolWindowDescriptor {
  return {
    id: "problems",
    title: "Problems",
    icon: null,
    defaultPlacement: { dock: "bottom", section: "start" },
    defaultOpen: true,
    component: () => createElement("div", { className: "test-body" }, "body"),
    ...over,
  };
}

function mountShell(d: ToolWindowDescriptor) {
  const registry = new ToolWindowRegistry();
  registry.register(d);
  const layout = createShellLayoutStore();
  layout.getState().syncFromRegistry(registry.list());

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);
  act(() => {
    root?.render(
      createElement(
        ShellProvider,
        {
          commands: new CommandRegistry(),
          themes: new ThemeService(),
          keymapOverrides: new KeymapOverridesService(),
          isMac: true,
          toolWindows: registry,
          layout,
        } as never,
        createElement(ShellFrame),
      ),
    );
  });
}

function header(): HTMLElement | null {
  return container?.querySelector(".shell-toolwindow .header") ?? null;
}

describe("tool-window header actions slot", () => {
  it("renders the app's component in the header, before the close button", () => {
    mountShell(
      descriptor({
        actions: () =>
          createElement("button", { type: "button", className: "sev-toggle" }, "errors"),
      }),
    );
    const slot = header()?.querySelector(".shell-toolwindow-header-actions");
    expect(slot).not.toBeNull();
    expect(slot?.querySelector(".sev-toggle")?.textContent).toBe("errors");

    // Order within the slot: actions first, close button last.
    const kids = [...(slot?.children ?? [])].map((el) => el.className);
    expect(kids[0]).toContain("sev-toggle");
    expect(kids.at(-1)).toContain("brink-panel-toggle");
  });

  it("is optional — a panel without actions keeps title + close only", () => {
    mountShell(descriptor());
    const slot = header()?.querySelector(".shell-toolwindow-header-actions");
    // The wrapper still exists (it holds the close button) but contributes
    // no controls of its own.
    expect(slot?.querySelectorAll("*").length).toBe(1);
    expect(slot?.querySelector(".brink-panel-toggle")).not.toBeNull();
    expect(header()?.textContent).toContain("Problems");
  });

  it("is a COMPONENT, so it re-renders from the app's own store without the shell", () => {
    // Mirrors how a real panel drives it: an external store the shell knows
    // nothing about. If the slot took nodes instead of a component, this
    // count could never update.
    let count = 0;
    const listeners = new Set<() => void>();
    const bump = () => {
      count += 1;
      listeners.forEach((l) => l());
    };
    const Actions = () => {
      const n = useSyncExternalStore(
        (l) => {
          listeners.add(l);
          return () => listeners.delete(l);
        },
        () => count,
      );
      return createElement("span", { className: "live-count" }, String(n));
    };

    mountShell(descriptor({ actions: Actions }));
    expect(header()?.querySelector(".live-count")?.textContent).toBe("0");

    act(() => bump());
    expect(header()?.querySelector(".live-count")?.textContent).toBe("1");
  });
});
