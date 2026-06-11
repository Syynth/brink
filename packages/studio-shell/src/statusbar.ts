/**
 * @brink/studio-shell — status-bar item registry (docs/studio-shell-spec.md §7.3).
 *
 * The status bar is populated via registered items, mirroring the
 * tool-window registry pattern (§7.1): feature segments are React components
 * *registered into* the shell at bootstrap — the shell package never imports
 * them (spec §7.2). Embedder hosts register through the same door with
 * `host.<vendor>.` ids (spec §8.1).
 */

import type { ComponentType } from "react";
import { HOST_ID_PREFIX, assertHostId } from "./command.js";

/** Which segment group an item renders in (spec §7.3). */
export type StatusBarAlignment = "left" | "right";

export interface StatusBarItemDescriptor {
  /** Stable, namespaced id, e.g. "status.compile", "status.cursor". */
  id: string;
  /** Segment group: left (app status) or right (editor context). */
  alignment: StatusBarAlignment;
  /**
   * Ordering within the alignment group. Higher priority renders further
   * left within its group (VS Code semantics); ties break by registration
   * order. Deterministic: equal-priority items never reorder.
   */
  priority: number;
  /** The segment content. Rendered inside the shell's item wrapper. */
  component: ComponentType;
}

/**
 * Group and order items for rendering: items split by alignment, each group
 * sorted by descending priority with ties in input (registration) order.
 * Pure and exported for tests.
 */
export function statusBarGroups(items: readonly StatusBarItemDescriptor[]): {
  left: StatusBarItemDescriptor[];
  right: StatusBarItemDescriptor[];
} {
  const byPriorityDesc = (
    a: StatusBarItemDescriptor,
    b: StatusBarItemDescriptor,
  ): number => b.priority - a.priority;
  return {
    // Array.prototype.sort is stable, so equal priorities keep input order.
    left: items.filter((i) => i.alignment === "left").sort(byPriorityDesc),
    right: items.filter((i) => i.alignment === "right").sort(byPriorityDesc),
  };
}

/**
 * Registry of status-bar items, mirroring ToolWindowRegistry's semantics:
 * register/list/get/onDidChange, duplicate-id and host-prefix rejection.
 */
export class StatusBarRegistry {
  private readonly items = new Map<string, StatusBarItemDescriptor>();
  private readonly changeListeners = new Set<() => void>();

  /**
   * Register an item. Throws on duplicate ids and on built-ins claiming the
   * host-reserved prefix. Returns an unregister function.
   */
  register(descriptor: StatusBarItemDescriptor): () => void {
    if (descriptor.id.startsWith(HOST_ID_PREFIX)) {
      throw new Error(
        `status bar item id "${descriptor.id}" uses the prefix reserved for embedder hosts`,
      );
    }
    return this.insert(descriptor);
  }

  /**
   * Register an embedder-host status-bar item (spec §8.1) — the id MUST
   * carry the `host.<vendor>.` prefix. Throws on malformed ids and
   * collisions. Returns an unregister function.
   */
  registerHost(descriptor: StatusBarItemDescriptor): () => void {
    assertHostId("status bar item", descriptor.id);
    return this.insert(descriptor);
  }

  private insert(descriptor: StatusBarItemDescriptor): () => void {
    if (this.items.has(descriptor.id)) {
      throw new Error(`duplicate status bar item id "${descriptor.id}"`);
    }
    this.items.set(descriptor.id, descriptor);
    this.notifyChange();
    return () => {
      if (this.items.delete(descriptor.id)) this.notifyChange();
    };
  }

  /** Subscribe to registrations/unregistrations. Returns an unsubscribe fn. */
  onDidChange(listener: () => void): () => void {
    this.changeListeners.add(listener);
    return () => {
      this.changeListeners.delete(listener);
    };
  }

  private notifyChange(): void {
    for (const listener of this.changeListeners) listener();
  }

  get(id: string): StatusBarItemDescriptor | undefined {
    return this.items.get(id);
  }

  /** All registered items, in registration order. */
  list(): StatusBarItemDescriptor[] {
    return [...this.items.values()];
  }
}
