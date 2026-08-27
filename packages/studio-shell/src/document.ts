/**
 * @brink/studio-shell — document-type registry (docs/studio-shell-spec.md §7.8).
 *
 * Documents are the editor-area counterpart of tool windows: the shell renders
 * tabs and groups (editor-groups.ts / editor-area.tsx) and mounts the component
 * registered for a tab's document type — it never imports feature components
 * (spec §7.2). Text documents (CM6) and custom-rendered documents (Compiled
 * Output #91, Story Graph #97, Player #120) implement the same contract.
 */

import type { ComponentType } from "react";
import { HOST_ID_PREFIX } from "./command.js";

/**
 * A reference to an open document: which type renders it, which document it
 * is within that type, and the tab title. Deliberately small and serializable
 * — anything heavier (content, ranges, handles) belongs to the registered
 * component's own machinery, keyed by `docId`.
 */
export interface DocumentRef {
  /** Registered document-type id, e.g. "ink-file". */
  typeId: string;
  /** Type-scoped document identity, e.g. "main.ink" or "main.ink::intro". */
  docId: string;
  /** Tab label. */
  title: string;
}

/** Props the shell passes to a mounted document component. */
export interface DocumentViewProps {
  doc: DocumentRef;
  /** The editor group this view is mounted in (one instance per group). */
  groupId: string;
  /** True when this view is the focused group's active tab. */
  active: boolean;
}

export interface DocumentTypeDescriptor {
  /** Stable, namespaced id, e.g. "ink-file". */
  id: string;
  /** Renders one view of a document of this type. */
  component: ComponentType<DocumentViewProps>;
  /**
   * This document TAKES OVER the editor root area rather than opening as a
   * tab inside whichever view is active (decision log 2026-08-26, "The
   * editor root area has one occupant").
   *
   * For whole-window activities that are not files — the Story Graph,
   * Settings, the compiled output. Without this they are unreachable in any
   * view that does not render tabs: Continuous view renders the project's
   * FILES, so a Settings tab opened behind it simply never appears.
   */
  takeover?: boolean;
}

// #2737 (follow-up from #2558/#2733): was `${ref.typeId}\x00${ref.docId}`
// (a literal NUL byte as separator) -- that made this file register as
// "binary" to `grep`/`rg` without `-a`/`--text`, silently hiding every
// match in it (including this function's own definition) from any
// repo-wide sweep, exactly as #2558 was for ink-editor/src/rename.ts.
// JSON-encoding the pair keeps the file plain, greppable UTF-8 text, and
// is provably collision-free where a printable separator would not be:
// JSON.stringify of a fixed 2-element array is an injective encoding --
// JSON.parse recovers the exact two original values, so two distinct
// (typeId, docId) pairs can never serialize to the same string,
// regardless of what characters either field contains (JSON escapes
// them, including any embedded NUL). `documentKey()`'s output is
// in-memory identity only (activeKey/tab matching in editor-groups.ts,
// editor-area.tsx, tab-drag.ts) -- never persisted (layout-persistence.ts's
// LayoutSnapshot carries no tab keys) -- so this swap is behavior-neutral,
// not a migration.
/** Stable identity of a document across groups: same key ⇒ same document. */
export function documentKey(ref: Pick<DocumentRef, "typeId" | "docId">): string {
  return JSON.stringify([ref.typeId, ref.docId]);
}

/**
 * Registry of document types, mirroring ToolWindowRegistry's semantics:
 * register/list/get/onDidChange, duplicate-id and host-prefix rejection.
 * Registered at bootstrap by the app (and, later, by embedder hosts §8).
 */
export class DocumentTypeRegistry {
  private readonly types = new Map<string, DocumentTypeDescriptor>();
  private readonly changeListeners = new Set<() => void>();

  /**
   * Register a document type. Throws on duplicate ids and on built-ins
   * claiming the host-reserved prefix. Returns an unregister function.
   */
  register(descriptor: DocumentTypeDescriptor): () => void {
    if (descriptor.id.startsWith(HOST_ID_PREFIX)) {
      throw new Error(
        `document type id "${descriptor.id}" uses the prefix reserved for embedder hosts`,
      );
    }
    if (this.types.has(descriptor.id)) {
      throw new Error(`duplicate document type id "${descriptor.id}"`);
    }
    this.types.set(descriptor.id, descriptor);
    this.notifyChange();
    return () => {
      if (this.types.delete(descriptor.id)) this.notifyChange();
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

  get(id: string): DocumentTypeDescriptor | undefined {
    return this.types.get(id);
  }

  /** All registered document types, in registration order. */
  list(): DocumentTypeDescriptor[] {
    return [...this.types.values()];
  }
}
