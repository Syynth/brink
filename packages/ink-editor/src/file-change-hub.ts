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

/**
 * An external (on-disk) change that would clobber unsaved studio edits
 * (issue #320). Reported when the host rewrites a file the studio has a
 * dirty buffer for, and the host's new on-disk content differs from that
 * buffer. The studio must NOT silently overwrite the editor buffer in this
 * case — it keeps the buffer, flags the path conflicted, and surfaces the
 * three texts so a merge/diff UI (Track V) can let the user reconcile.
 */
export interface FileConflict {
  path: string;
  /** The host's new on-disk content (the external change). */
  disk: string;
  /** The studio's current, unsaved editor-buffer content. */
  buffer: string;
  /** The last host-synced baseline the dirty buffer diverged from. */
  baseline: string;
}

export interface FileChangeHubOptions {
  /** Read a file's current session content (null when unknown/removed). */
  getContent(path: string): string | null;
  /** The host's change callback. Without it, nothing is ever delivered —
   *  the hub still tracks dirty state, and explicit saves re-baseline. */
  onFlush?: (changes: FileChange[]) => void;
  /** Dirty-file count changed (drives the public-state summary). */
  onDirtyChange?: (dirtyCount: number) => void;
  /** An external change collided with an unsaved studio buffer (issue #320).
   *  Fired by the session's external-change handler when
   *  {@link FileChangeHub.detectExternalConflict} returns a conflict, so a
   *  merge/diff surface can reconcile the two versions. */
  onFileConflict?: (conflict: FileConflict) => void;
  /** Trailing debounce before an automatic flush (default 500 ms). */
  debounceMs?: number;
  /**
   * Whether a delivered flush counts as persistence (default `true`).
   *
   * `true` — the original write-through contract: "last-notified content
   * is, by contract, content the host has persisted", so `flush()`
   * re-baselines every delivered path and dirty clears on delivery. For
   * hosts whose egress handler writes canonical files (e.g. RPG Maker MZ
   * mirroring `data/brink/**`).
   *
   * `false` — the overlay contract (the celeris file model, 2026-08-07
   * decision-log entry): delivery feeds a **backup ring**, not canonical
   * storage, so `flush()` delivers batches but moves NO baselines — dirty
   * means "diverges from the last canonical save" and only `markSaved`
   * (an explicit save / autosave tick) clears it. The no-op check in
   * `record()` still compares against the canonical baseline, so an undo
   * back to the saved text correctly drops back to clean.
   */
  deliveryPersists?: boolean;
}

const DEFAULT_DEBOUNCE_MS = 500;

// ── Hub ────────────────────────────────────────────────────────────

export class FileChangeHub {
  private readonly getContent: (path: string) => string | null;
  private readonly onFlush?: (changes: FileChange[]) => void;
  private onDirtyChange?: (dirtyCount: number) => void;
  private readonly onFileConflict?: (conflict: FileConflict) => void;
  private readonly debounceMs: number;
  private readonly deliveryPersists: boolean;

  /** Pending (recorded, not yet flushed) changes, coalesced per path. */
  private readonly pending = new Map<string, FileChangeType>();
  /** Last host-synced content per path (load / flush / save). */
  private readonly baselines = new Map<string, string>();
  /** Paths whose session content diverges from baseline. */
  private readonly dirty = new Set<string>();
  /** Paths whose dirty buffer collided with an external change and was kept
   *  (issue #320). Cleared when the path is re-baselined or saved. */
  private readonly conflicted = new Set<string>();
  /**
   * Paths deleted externally while the studio keeps a buffer for them (issue
   * #2371, "External deletion of an open file: keep the view, mark
   * orphaned"). Set by `applyExternal(path, null)`; cleared by a canonical
   * save (`markSaved`, or a write-through `flush()`) or by the path
   * reappearing on disk (`applyExternal(path, <content>)`). Independent of
   * `dirty`/`conflicted` — a path can be orphaned before any edit re-creates
   * dirty content for it.
   */
  private readonly orphaned = new Set<string>();
  private timer: ReturnType<typeof setTimeout> | null = null;
  private disposed = false;

  constructor(options: FileChangeHubOptions) {
    this.getContent = options.getContent;
    this.onFlush = options.onFlush;
    this.onDirtyChange = options.onDirtyChange;
    this.onFileConflict = options.onFileConflict;
    this.debounceMs = options.debounceMs ?? DEFAULT_DEBOUNCE_MS;
    this.deliveryPersists = options.deliveryPersists ?? true;
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
      // The file is gone from disk (issue #2371): flag it orphaned so a kept
      // editor buffer can be badged "deleted on disk" and the reason this
      // path has no baseline traces back to a real deletion, not an
      // unsaved create.
      this.orphaned.add(path);
    } else {
      this.baselines.set(path, content);
      // The path exists on disk again (external re-creation, or
      // `resolveConflictUseDisk` taking the host's content) — any standing
      // orphan marker no longer applies.
      this.orphaned.delete(path);
    }
    this.pending.delete(path);
    // Re-baselining to the host's content resolves any standing conflict.
    this.conflicted.delete(path);
    this.updateDirty(path);
  }

  // ── External conflicts (issue #320) ──────────────────────────────

  /**
   * Decide whether an external change to `path` (the host's new on-disk
   * content `disk`) would clobber an unsaved studio edit. Returns a
   * {@link FileConflict} ONLY when all hold:
   *
   * - the path is dirty (its buffer diverges from the baseline), AND
   * - the host's `disk` content differs from that live buffer, AND
   * - a baseline exists (the studio knows what the buffer diverged from).
   *
   * Otherwise (clean buffer, or buffer already equals disk, or no baseline)
   * there is nothing to reconcile and `null` is returned — the caller is
   * free to overwrite the buffer and re-baseline as before. Pure query: it
   * records no state, so the caller decides what to do with a conflict.
   */
  detectExternalConflict(path: string, disk: string): FileConflict | null {
    if (!this.dirty.has(path)) return null;
    const baseline = this.baselines.get(path);
    if (baseline === undefined) return null;
    const buffer = this.getContent(path);
    if (buffer === null) return null;
    if (buffer === disk) return null;
    return { path, disk, buffer, baseline };
  }

  /**
   * Apply the safe default for an unresolved external conflict (issue #320):
   * KEEP the dirty editor buffer (do not overwrite it, do not re-baseline)
   * and flag the path as conflicted. Caller invokes this after
   * {@link detectExternalConflict} returns non-null instead of the
   * overwrite+re-baseline path.
   */
  markConflicted(path: string): void {
    this.conflicted.add(path);
  }

  /**
   * Resolve a standing conflict by KEEPING the editor buffer (issue #320):
   * clear the conflicted flag only. The baseline is untouched, so the path
   * stays dirty — the kept buffer still diverges from the last host-synced
   * content and is re-delivered on the next flush/save. This is the "Keep
   * mine" merge action. A merged-edit resolution instead routes its text
   * through `record` (via {@link ProjectSession.applyEdit}) first, then calls
   * this to drop the flag. No-op for an unconflicted path.
   */
  clearConflict(path: string): void {
    this.conflicted.delete(path);
  }

  /** Whether `path` has a kept-but-unreconciled external conflict (#320). */
  isConflicted(path: string): boolean {
    return this.conflicted.has(path);
  }

  /** Paths whose dirty buffer collided with an external change and was kept,
   *  not yet reconciled (issue #320). Sorted for deterministic output. */
  conflictedPaths(): string[] {
    return [...this.conflicted].sort();
  }

  // ── Orphaned paths (issue #2371) ──────────────────────────────────

  /** Whether `path` was deleted externally while a kept buffer for it
   *  survives, not yet recreated by a save or an external re-creation. */
  isOrphaned(path: string): boolean {
    return this.orphaned.has(path);
  }

  /** Sorted paths flagged orphaned (issue #2371) — for tab badging. */
  orphanedPaths(): string[] {
    return [...this.orphaned].sort();
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
      // Saving the kept buffer resolves any standing conflict for the path.
      this.conflicted.delete(path);
      // A canonical save recreates the file on disk (issue #2371) — no
      // longer orphaned even when the save was writing a kept-but-unedited
      // buffer back over an external deletion.
      this.orphaned.delete(path);
      this.updateDirty(path);
    }
  }

  // ── Flushing ─────────────────────────────────────────────────────

  /**
   * Deliver every pending change to the host now (save commands, unmount;
   * the debounce timer lands here too). Content is read at this moment.
   *
   * Under the default write-through contract (`deliveryPersists: true`),
   * delivered files are re-baselined — "last-notified" content is, by
   * contract, content the host has persisted. Under the overlay contract
   * (`deliveryPersists: false`) delivery moves NO baselines: the batch
   * feeds a backup ring, and only `markSaved` (a canonical save) clears
   * dirty. Without an `onFlush` host hook this is a no-op: changes stay
   * pending and dirty until an explicit `markSaved`. Returns the delivered
   * batch (empty when nothing flushed).
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
        if (this.deliveryPersists) this.baselines.delete(path);
      } else {
        const content = this.getContent(path);
        if (content === null) continue; // vanished between record and flush
        changes.push({ path, type, content });
        if (this.deliveryPersists) {
          this.baselines.set(path, content);
          // Write-through contract (issue #2371): delivery here IS the
          // host's persistence, so it clears orphaned the same as an
          // explicit `markSaved` would — no separate save step exists for
          // this contract to wait for.
          this.orphaned.delete(path);
        }
      }
    }
    this.pending.clear();
    if (this.deliveryPersists) {
      for (const { path } of changes) this.updateDirty(path);
    }

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
      // A clean path (buffer back at baseline) has nothing left to reconcile.
      this.conflicted.delete(path);
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
