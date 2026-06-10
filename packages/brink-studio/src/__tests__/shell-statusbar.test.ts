/**
 * @brink/studio-shell unit tests — status-bar item registry and grouping
 * (shell issue 2.2, spec §7.3).
 */

import { describe, expect, it, vi } from "vitest";
import { StatusBarRegistry, statusBarGroups } from "@brink/studio-shell";

const Noop = () => null;

function item(id: string, alignment: "left" | "right", priority: number) {
  return { id, alignment, priority, component: Noop };
}

describe("statusBarGroups", () => {
  it("splits by alignment and orders by descending priority", () => {
    const groups = statusBarGroups([
      item("a", "right", 10),
      item("b", "left", 5),
      item("c", "left", 20),
      item("d", "right", 30),
    ]);
    expect(groups.left.map((i) => i.id)).toEqual(["c", "b"]);
    expect(groups.right.map((i) => i.id)).toEqual(["d", "a"]);
  });

  it("breaks priority ties by input (registration) order — deterministic", () => {
    const groups = statusBarGroups([
      item("first", "left", 10),
      item("second", "left", 10),
      item("third", "left", 10),
    ]);
    expect(groups.left.map((i) => i.id)).toEqual(["first", "second", "third"]);
  });
});

describe("StatusBarRegistry", () => {
  it("registers, lists in order, rejects duplicates and host ids", () => {
    const registry = new StatusBarRegistry();
    registry.register(item("status.a", "left", 1));
    registry.register(item("status.b", "right", 1));
    expect(registry.list().map((i) => i.id)).toEqual(["status.a", "status.b"]);
    expect(() => registry.register(item("status.a", "left", 1))).toThrow(/duplicate/);
    expect(() => registry.register(item("host.acme.x", "left", 1))).toThrow(/reserved/);
  });

  it("notifies on register/unregister and stops after unsubscribe", () => {
    const registry = new StatusBarRegistry();
    const listener = vi.fn();
    const unsubscribe = registry.onDidChange(listener);

    const dispose = registry.register(item("status.a", "left", 1));
    expect(listener).toHaveBeenCalledTimes(1);
    dispose();
    expect(listener).toHaveBeenCalledTimes(2);
    dispose();
    expect(listener).toHaveBeenCalledTimes(2);

    unsubscribe();
    registry.register(item("status.b", "left", 1));
    expect(listener).toHaveBeenCalledTimes(2);
  });
});
