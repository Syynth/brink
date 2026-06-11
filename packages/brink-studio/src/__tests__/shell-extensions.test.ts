/**
 * Embedder extension API tests (shell issue 5.4 / #95, spec §8.1):
 * host-id namespacing enforcement on the registries' registerHost paths,
 * installStudioExtensions (atomic install, rollback on failure, uninstall),
 * and host-panel layout persistence — placements survive a reload with the
 * extension installed, and a layout mentioning a removed host panel loads
 * cleanly (unknown ids dropped by the registry sync, §7.1).
 */

import { describe, expect, it } from "vitest";
import {
  CommandRegistry,
  StatusBarRegistry,
  ToolWindowRegistry,
  createShellLayoutStore,
  installStudioExtensions,
  loadLayoutSnapshot,
  LAYOUT_STORAGE_KEY,
  type Command,
  type StatusBarItemDescriptor,
  type StudioExtensionRegistries,
  type ToolWindowDescriptor,
} from "@brink/studio-shell";

const toolWindow = (id: string): ToolWindowDescriptor => ({
  id,
  title: id,
  icon: null,
  defaultPlacement: { dock: "right", section: "end" },
  defaultOpen: false,
  component: () => null,
});

const command = (id: string): Command => ({ id, title: id, run: () => {} });

const statusItem = (id: string): StatusBarItemDescriptor => ({
  id,
  alignment: "left",
  priority: 0,
  component: () => null,
});

function registries(): StudioExtensionRegistries {
  return {
    commands: new CommandRegistry(),
    toolWindows: new ToolWindowRegistry(),
    statusBarItems: new StatusBarRegistry(),
  };
}

// ── Namespacing enforcement (registerHost) ──────────────────────────

describe("registerHost namespacing (spec §8.1)", () => {
  it("accepts well-formed host ids on all three registries", () => {
    const r = registries();
    expect(() => r.toolWindows.registerHost(toolWindow("host.example.functions"))).not.toThrow();
    expect(() => r.commands.registerHost(command("host.example.revealStart"))).not.toThrow();
    expect(() => r.statusBarItems.registerHost(statusItem("host.example.status"))).not.toThrow();
  });

  it.each([
    "functions", // no prefix at all
    "myhost.example.functions", // wrong prefix
    "host.", // nothing after the prefix
    "host.example", // vendor only, no name
    "host.example.", // empty name segment
    "host..functions", // empty vendor segment
  ])('rejects malformed host id "%s" with the namespacing error', (id) => {
    const r = registries();
    const message = `must be namespaced "host.<vendor>.<name>"`;
    expect(() => r.toolWindows.registerHost(toolWindow(id))).toThrowError(message);
    expect(() => r.commands.registerHost(command(id))).toThrowError(message);
    expect(() => r.statusBarItems.registerHost(statusItem(id))).toThrowError(message);
  });

  it("still rejects built-ins claiming the host prefix (the inverse rule)", () => {
    const r = registries();
    expect(() => r.toolWindows.register(toolWindow("host.example.functions"))).toThrowError(
      "reserved for embedder hosts",
    );
  });

  it("rejects collisions between host items with a clean error", () => {
    const r = registries();
    r.toolWindows.registerHost(toolWindow("host.example.functions"));
    expect(() => r.toolWindows.registerHost(toolWindow("host.example.functions"))).toThrowError(
      'duplicate tool window id "host.example.functions"',
    );
  });
});

// ── installStudioExtensions ──────────────────────────────────────────

describe("installStudioExtensions", () => {
  it("registers tool windows, commands, and status-bar items", () => {
    const r = registries();
    installStudioExtensions(
      {
        toolWindows: [toolWindow("host.example.functions")],
        commands: [command("host.example.revealStart")],
        statusBarItems: [statusItem("host.example.status")],
      },
      r,
    );
    expect(r.toolWindows.get("host.example.functions")).toBeDefined();
    expect(r.commands.get("host.example.revealStart")).toBeDefined();
    expect(r.statusBarItems.get("host.example.status")).toBeDefined();
    expect(r.commands.dispatch("host.example.revealStart")).toBe(true);
  });

  it("uninstall unregisters everything (and is idempotent)", () => {
    const r = registries();
    const uninstall = installStudioExtensions(
      {
        toolWindows: [toolWindow("host.example.functions")],
        commands: [command("host.example.revealStart")],
        statusBarItems: [statusItem("host.example.status")],
      },
      r,
    );
    uninstall();
    expect(r.toolWindows.list()).toHaveLength(0);
    expect(r.commands.list()).toHaveLength(0);
    expect(r.statusBarItems.list()).toHaveLength(0);
    expect(() => uninstall()).not.toThrow();
  });

  it("a rejected install rolls back everything it already registered", () => {
    const r = registries();
    r.commands.register(command("builtin.command"));
    expect(() =>
      installStudioExtensions(
        {
          toolWindows: [toolWindow("host.example.functions")],
          // Second command is malformed → the whole install must fail …
          commands: [command("host.example.ok"), command("not-namespaced")],
        },
        r,
      ),
    ).toThrowError('host command id "not-namespaced"');
    // … leaving the registries exactly as they were before the call.
    expect(r.toolWindows.list()).toHaveLength(0);
    expect(r.commands.list().map((c) => c.id)).toEqual(["builtin.command"]);
  });

  it("rejects a host id colliding with a built-in registration", () => {
    const r = registries();
    // A built-in can't take a host.* id, but a stale host registration can
    // collide with a fresh install of the same extension.
    installStudioExtensions({ commands: [command("host.example.run")] }, r);
    expect(() =>
      installStudioExtensions({ commands: [command("host.example.run")] }, r),
    ).toThrowError('duplicate command id "host.example.run"');
  });
});

// ── Host-panel layout persistence (spec §7.1 / §8.1) ─────────────────

describe("host panel layout persistence", () => {
  const HOST_ID = "host.example.functions";

  function storedLayout() {
    return {
      version: 1,
      placements: {
        binder: { dock: "left", section: "start" },
        [HOST_ID]: { dock: "bottom", section: "end" }, // user dragged it there
      },
      open: { "bottom.end": HOST_ID },
      dockSizes: { left: 200, right: 200, bottom: 160 },
      maximized: null,
    };
  }

  function storage(payload: unknown) {
    return { getItem: (k: string) => (k === LAYOUT_STORAGE_KEY ? JSON.stringify(payload) : null) };
  }

  it("a dragged host panel's placement survives restore while installed", () => {
    const r = registries();
    installStudioExtensions({ toolWindows: [toolWindow(HOST_ID)] }, r);
    const layout = createShellLayoutStore();
    layout.setState(loadLayoutSnapshot(storage(storedLayout()))!);
    layout.getState().syncFromRegistry([toolWindow("binder"), ...r.toolWindows.list()]);
    const s = layout.getState();
    expect(s.placements[HOST_ID]).toEqual({ dock: "bottom", section: "end" });
    expect(s.open["bottom.end"]).toBe(HOST_ID);
  });

  it("loading without the extension drops the host panel's ids cleanly", () => {
    const layout = createShellLayoutStore();
    layout.setState(loadLayoutSnapshot(storage(storedLayout()))!);
    // Registry sync without the host descriptor (extension removed between
    // sessions): the unknown id is dropped silently, nothing else changes.
    layout.getState().syncFromRegistry([toolWindow("binder")]);
    const s = layout.getState();
    expect(s.placements[HOST_ID]).toBeUndefined();
    expect(s.open["bottom.end"]).toBeNull();
    expect(s.placements.binder).toEqual({ dock: "left", section: "start" });
  });
});
