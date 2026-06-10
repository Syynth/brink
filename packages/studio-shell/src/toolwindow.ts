/**
 * @brink/studio-shell — tool-window registry (docs/studio-shell-spec.md §7.1).
 *
 * Tool windows are the movable unit of the shell: registered descriptors the
 * shell renders into docks/strips purely from this registry plus the layout
 * store. Feature components are *registered into* the shell at bootstrap —
 * the shell package never imports them (spec §7.2).
 */

import type { ComponentType, ReactNode } from "react";
import { HOST_ID_PREFIX } from "./command.js";

/** Edge docks that can host tool windows (spec §2). */
export type Dock = "left" | "right" | "bottom";

/** Each dock has two sections: start/end (top/bottom or left/right). */
export type Section = "start" | "end";

/** A dock+section address, e.g. "left.start". */
export type DockSectionId = `${Dock}.${Section}`;

/** Where a tool window lives. */
export interface Placement {
  dock: Dock;
  section: Section;
}

/** All six dock sections, in deterministic order. */
export const DOCK_SECTION_IDS: readonly DockSectionId[] = [
  "left.start",
  "left.end",
  "right.start",
  "right.end",
  "bottom.start",
  "bottom.end",
];

/** Lookup key for a placement, e.g. "right.end". */
export function dockSectionId(placement: Placement): DockSectionId {
  return `${placement.dock}.${placement.section}`;
}

export interface ToolWindowDescriptor {
  /** Stable, namespaced id, e.g. "binder", "player". */
  id: string;
  /** Strip tooltip / chrome header / palette title fragment. */
  title: string;
  /** Strip icon (monochrome, currentColor). */
  icon: ReactNode;
  /** Where the window docks before the user moves it. */
  defaultPlacement: Placement;
  /** Whether the window starts open in its section. */
  defaultOpen: boolean;
  /**
   * Optional strip badge (spec §5.1 — e.g. Problems error count), as a
   * component rather than a value selector: the registering app provides a
   * component that subscribes to its own store and renders the count bubble
   * (typically `<span className="shell-strip-badge">{n}</span>`) or null.
   * This keeps badges reactive (the strip re-renders only on layout/registry
   * changes) without the shell depending on any app store (spec §7.2).
   */
  badge?: ComponentType;
  /** The window's content. Rendered below the shell's chrome header. */
  component: ComponentType;
}

/**
 * Registry of dockable tool windows, mirroring CommandRegistry's semantics:
 * register/list/get/onDidChange, duplicate-id and host-prefix rejection.
 */
export class ToolWindowRegistry {
  private readonly toolWindows = new Map<string, ToolWindowDescriptor>();
  private readonly changeListeners = new Set<() => void>();

  /**
   * Register a tool window. Throws on duplicate ids and on built-ins claiming
   * the host-reserved prefix. Returns an unregister function.
   */
  register(descriptor: ToolWindowDescriptor): () => void {
    if (descriptor.id.startsWith(HOST_ID_PREFIX)) {
      throw new Error(
        `tool window id "${descriptor.id}" uses the prefix reserved for embedder hosts`,
      );
    }
    if (this.toolWindows.has(descriptor.id)) {
      throw new Error(`duplicate tool window id "${descriptor.id}"`);
    }
    this.toolWindows.set(descriptor.id, descriptor);
    this.notifyChange();
    return () => {
      if (this.toolWindows.delete(descriptor.id)) this.notifyChange();
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

  get(id: string): ToolWindowDescriptor | undefined {
    return this.toolWindows.get(id);
  }

  /** All registered tool windows, in registration order. */
  list(): ToolWindowDescriptor[] {
    return [...this.toolWindows.values()];
  }
}
