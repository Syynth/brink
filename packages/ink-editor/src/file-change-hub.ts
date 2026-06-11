/**
 * FileChangeHub — the single seam every session-content mutation reports
 * through (issues #154/#137).
 *
 * The host-facing egress contract (`onFilesChanged`) is only trustworthy if
 * every edit path feeds it; #137 proved that paths can silently skip an
 * optional seam. The hub makes omission impossible by being the one place
 * changes are recorded: CM6 edit flushes, binder structural ops, search
 * replacements, and file creation all land in `record()` (via
 * `ProjectSession.notifyFileChanged` / `applyEdit` / `addFile`).
 *
 * Responsibilities:
 *
 * - **Batching + debounce.** Records coalesce per path; a trailing debounce
 *   (default 500 ms) delivers one `FileChange[]` batch to `onFlush`. An
 *   event per keystroke is forbidden by design — and the CM6 path is
 *   additionally bounded by the editor's own compile debounce upstream.
 * - **Deferred content.** `record()` stores only (path, kind); file content
 *   is read from the session at flush time (defer resolution — the host
 *   gets the latest text, not a stale intermediate).
 * - **Dirty tracking.** A file is dirty when its session content diverges
 *   from its baseline: the content last loaded from the host (mount /
 *   external change) or last delivered to it (flush / explicit save).
 *   `markSaved` re-baselines without requiring a host callback, so
 *   `file.save` works in the standalone playground too.
 *
 * "deleted" is part of the contract by design (so host-side mirrors of
 * renames/deletes are additive later) but is currently unreachable — the
 * studio has no delete UI and `ProjectSession.closeFile` only evicts from
 * the wasm session without deleting anything.
 */

// ── Public types ───────────────────────────────────────────────────

export type FileChangeType = "modified" | "created" | "deleted";

/** One file's change, as delivered to the host (`onFilesChanged`). */
export interface FileChange {
  path: string;
  type: FileChangeType;
  /** The file's full content at flush time. Omitted for `deleted`. */
  content?: string;
}

export interface FileChangeHubOptions {
  /** Read a file's current session content (null when unknown/removed). */
  getContent(path: string): string | null;
  /** The host's change callback. Without it, nothing is ever delivered —
   *  the hub still tracks dirty state, and explicit saves re-baseline. */
  onFlush?: (changes: FileChange[]) => void;
  /** Dirty-file count changed (drives the public-state summary). */
  onDirtyChange?: (dirtyCount: number) => void;
  /** Trailing debounce before an automatic flush (default 500 ms). */
  debounceMs?: number;
}

const DEFAULT_DEBOUNCE_MS = 500;

// ── Hub ────────────────────────────────────────────────────────────

export class FileChangeHub {
  private readonly getContent: (path: string) => string | null;
  private readonly onFlush?: (changes: FileChange[]) => void;
  private onDirtyChange?: (dirtyCount: number) => void;
  private readonly debounceMs: number;

  /** Pending (recorded, not yet flushed) changes, coalesced per path. */
  private readonly pending = new Map<string, FileChangeType>();
  /** Last host-synced content per path (load / flush / save). */
  private readonly baselines = new Map<string, string>();
  /** Paths whose session content diverges from baseline. */
  private readonly dirty = new Set<string>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;

  constructor(options: FileChangeHubOptions) {
    this.getContent = options.getContent;
    this.onFlush = options.onFlush;
    this.onDirtyChange = options.onDirtyChange;
    this.debounceMs = options.debounceMs ?? DEFAULT_DEBOUNCE_MS;
  }

  /** Late-bind the dirty listener (the store exists after the session). */
  setDirtyListener(listener: ((dirtyCount: number) => void) | undefined): void {
    this.onDirtyChange = listener;
    listener?.(this.dirty.size);
  }

  // ── Recording ────────────────────────────────────────────────────

  /**
   * Record a change to `path`. Coalescing rules: a "created" pending entry
   * absorbs later "modified" records (the host has never seen the file —
   * it is still a creation); a "modified" record whose content equals the
   * baseline is dropped entirely (no-op edits, e.g. an initial compile
   * flush or an undo back to the saved text, must not reach the host).
   */
  record(path: string, type: FileChangeType): void {
    if (this.disposed) return;

    if (type === "modified") {
      const existing = this.pending.get(path);
      if (existing === undefined || existing === "modified") {
        if (this.getContent(path) === this.baselines.get(path)) {
          // Content matches what the host already has: nothing to report.
          this.pending.delete(path);
        } else {
          this.pending.set(path, "modified");
        }
      }
      // existing === "created": keep "created" (host never saw the file).
      // existing === "deleted": unreachable today; a modify after a delete
      // would be a re-creation — record() callers go through addFile then.
    } else {
      this.pending.set(path, type);
    }

    this.updateDirty(path);
    this.schedule();
  }

  // ── Baselines (host-synced content) ──────────────────────────────

  /** Set a path's baseline (loaded from the host: mount, INCLUDE, request). */
  setBaseline(path: string, content: string): void {
    this.baselines.set(path, content);
    this.updateDirty(path);
  }

  /**
   * The host changed (or deleted) the file externally: its content is the
   * new truth, any pending studio-side change for it is superseded.
   */
  applyExternal(path: string, content: string | null): void {
    if (content === null) {
      this.baselines.delete(path);
    } else {
      this.baselines.set(path, content);
    }
    this.pending.delete(path);
    this.updateDirty(path);
  }

  /** Re-baseline `paths` to their current session content (explicit save).
   *  Pending entries for them are dropped — the save already synced. */
  markSaved(paths: Iterable<string>): void {
    for (const path of paths) {
      const content = this.getContent(path);
      if (content === null) {
        this.baselines.delete(path);
      } else {
        this.baselines.set(path, content);
      }
      this.pending.delete(path);
      this.updateDirty(path);
    }
  }

  // ── Flushing ─────────────────────────────────────────────────────

  /**
   * Deliver every pending change to the host now (save commands, unmount;
   * the debounce timer lands here too). Content is read at this moment.
   * Delivered files are re-baselined — "last-notified" content is, by
   * contract, content the host has persisted. Without an `onFlush` host
   * hook this is a no-op: changes stay pending and dirty until an explicit
   * `markSaved`. Returns the delivered batch (empty when nothing flushed).
   */
  flush(): FileChange[] {
    this.cancelTimer();
    if (this.onFlush === undefined || this.pending.size === 0) return [];

    // Sorted for deterministic batch order (Map insertion order is edit
    // order, which is fine for hosts but unstable for tests/diffing).
    const paths = [...this.pending.keys()].sort();
    const changes: FileChange[] = [];
    for (const path of paths) {
      const type = this.pending.get(path)!;
      if (type === "deleted") {
        changes.push({ path, type });
        this.baselines.delete(path);
      } else {
        const content = this.getContent(path);
        if (content === null) continue; // vanished between record and flush
        changes.push({ path, type, content });
        this.baselines.set(path, content);
      }
    }
    this.pending.clear();
    for (const { path } of changes) this.updateDirty(path);

    if (changes.length > 0) this.onFlush(changes);
    return changes;
  }

  // ── Dirty state ──────────────────────────────────────────────────

  /** Paths whose session content diverges from the host baseline, sorted. */
  dirtyPaths(): string[] {
    return [...this.dirty].sort();
  }

  dirtyCount(): number {
    return this.dirty.size;
  }

  // ── Teardown ─────────────────────────────────────────────────────

  /** Cancel the debounce timer; further records are ignored. Call `flush()`
   *  first when pending changes should still reach the host (unmount). */
  dispose(): void {
    this.disposed = true;
    this.cancelTimer();
  }

  // ── Private ──────────────────────────────────────────────────────

  /** Recompute one path's dirty membership (content vs baseline). */
  private updateDirty(path: string): void {
    const before = this.dirty.size;
    const content = this.getContent(path);
    const baseline = this.baselines.get(path);
    // A file with no baseline is dirty while it exists (created, unsaved);
    // a missing file with no baseline is fully gone — clean.
    const isDirty = content === null ? baseline !== undefined : content !== baseline;
    if (isDirty) {
      this.dirty.add(path);
    } else {
      this.dirty.delete(path);
    }
    if (this.dirty.size !== before) {
      this.onDirtyChange?.(this.dirty.size);
    }
  }

  /** (Re)arm the trailing debounce — only when a host hook can consume it. */
  private schedule(): void {
    if (this.onFlush === undefined || this.pending.size === 0) return;
    this.cancelTimer();
    this.timer = setTimeout(() => {
      this.timer = null;
      this.flush();
    }, this.debounceMs);
  }

  private cancelTimer(): void {
    if (this.timer !== null) {
      clearTimeout(this.timer);
      this.timer = null;
    }
  }
}
