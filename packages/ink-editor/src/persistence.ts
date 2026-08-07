/**
 * OverlayPersistence — the shared canonical-save + backup-ring coordinator
 * for overlay hosts (brink `docs/decision-log.md` 2026-08-07, "Desktop
 * persistence adopts the celeris overlay model"; celeris `§10.1.1`).
 *
 * The model separates three orthogonal axes the old write-through pattern
 * conflated:
 *
 * - **Dirty** = buffer ≠ last canonical save. Owned by `FileChangeHub`
 *   under `egressPersists: false` — delivery no longer re-baselines, only
 *   a canonical save does.
 * - **Canonical saves** — explicit (⌘S / saveAll) or the autosave tick,
 *   which is the SAME save (one code path, one artifact class; celeris:
 *   "autosave is a real save").
 * - **Crash protection** — the #154 egress batches (debounced ~500 ms,
 *   flushed on unmount) feed a bounded **backup ring** via a host-provided
 *   {@link BackupSink}. Backups never clear dirty.
 *
 * The coordinator is host-agnostic: brink-desktop supplies a sink over
 * Tauri app-data and a canonical store over its guarded fs commands;
 * celeris supplies its asset-io equivalents. Ring BOUNDS (N entries /
 * Y MB) are enforced by the sink, next to the storage it manages — the
 * coordinator neither knows nor cares how pruning works.
 *
 * Wiring (the host does this; the coordinator owns no callbacks itself):
 *
 * ```ts
 * const persistence = new OverlayPersistence({ session, canonical, sink,
 *   autosaveMs: 120_000 });
 * mountStudio(root, {
 *   provider,
 *   egressPersists: false,
 *   onFilesChanged: (b) => persistence.handleEgress(b),
 *   ...
 * });
 * ```
 */

import type { FileChange } from "./file-change-hub.js";

// ── Backup ring contract ───────────────────────────────────────────

/** One ring entry: a full-content snapshot of one file at one moment. */
export interface BackupEntry {
  path: string;
  content: string;
  /** Milliseconds since epoch, from the coordinator's clock. */
  at: number;
}

/** Metadata for a stored ring entry (the read side; used by restore UI). */
export interface BackupMeta {
  id: string;
  path: string;
  at: number;
  bytes: number;
}

/**
 * Host-provided backup storage. `append` is the only required operation —
 * the sink OWNS the ring bounds (keep N entries or ≤ Y MB, pruned
 * near the storage it manages, e.g. atomically in the desktop shell's
 * Rust command). The optional read side exists for the restore/rollback
 * UI; an append-only sink is a valid starting point.
 */
export interface BackupSink {
  append(entries: BackupEntry[]): Promise<void>;
  list?(path?: string): Promise<BackupMeta[]>;
  read?(id: string): Promise<BackupEntry | null>;
}

// ── Coordinator ────────────────────────────────────────────────────

/**
 * The slice of `ProjectSession` the coordinator needs — structural, so
 * tests supply a plain object and the coordinator never imports the
 * concrete class.
 */
export interface PersistenceSession {
  dirtyPaths(): string[];
  getFiles(): Record<string, string>;
  markFilesSaved(paths: Iterable<string>): void;
}

/** Host-provided canonical storage (the real project files). */
export interface CanonicalStore {
  write(path: string, content: string): Promise<void>;
}

export interface OverlayPersistenceOptions {
  session: PersistenceSession;
  canonical: CanonicalStore;
  /** Absent ⇒ no crash-protection ring (egress batches are dropped). */
  sink?: BackupSink;
  /**
   * Autosave interval in ms; `null`/absent ⇒ manual saves only. An
   * autosave tick IS `saveAll` — same write, same dirty-clear. Ticks with
   * nothing dirty are no-ops.
   */
  autosaveMs?: number | null;
  /**
   * Persistence failures (a canonical write or a ring append rejecting).
   * The coordinator never throws from timer/egress context; route this to
   * user-visible surface (the desktop routes to the Output channel).
   */
  onError?: (error: unknown, context: "canonical" | "backup") => void;
  /** Clock override for tests. */
  now?: () => number;
}

export class OverlayPersistence {
  private readonly session: PersistenceSession;
  private readonly canonical: CanonicalStore;
  private readonly sink?: BackupSink;
  private readonly onError?: (error: unknown, context: "canonical" | "backup") => void;
  private readonly now: () => number;

  private autosaveMs: number | null;
  private timer: ReturnType<typeof setInterval> | null = null;
  private disposed = false;
  /** Serializes saves: a save started while one runs queues behind it. */
  private saving: Promise<unknown> = Promise.resolve();

  constructor(options: OverlayPersistenceOptions) {
    this.session = options.session;
    this.canonical = options.canonical;
    this.sink = options.sink;
    this.onError = options.onError;
    this.now = options.now ?? (() => Date.now());
    this.autosaveMs = options.autosaveMs ?? null;
    this.arm();
  }

  // ── Ring feed (wire to mountStudio's onFilesChanged) ─────────────

  /**
   * Feed a #154 egress batch into the backup ring. Fire-and-forget by
   * design — the egress callback is synchronous and a ring append must
   * never block or break editing. Deletions don't ring (structural ops
   * are disk-immediate through the provider; the ring holds content
   * snapshots only).
   */
  handleEgress(changes: FileChange[]): void {
    if (this.disposed || this.sink === undefined) return;
    const at = this.now();
    const entries: BackupEntry[] = changes
      .filter((c) => c.type !== "deleted" && c.content !== undefined)
      .map((c) => ({ path: c.path, content: c.content ?? "", at }));
    if (entries.length === 0) return;
    void this.sink.append(entries).catch((e: unknown) => {
      this.onError?.(e, "backup");
    });
  }

  // ── Canonical saves ──────────────────────────────────────────────

  /**
   * Canonically save every dirty file: write each through the store, then
   * re-baseline them (`markFilesSaved`), which clears dirty and resolves
   * any kept conflicts for those paths. Resets the autosave timer — a
   * manual save restarts the countdown rather than double-saving.
   * Returns the paths saved (empty when nothing was dirty).
   *
   * A path whose write REJECTS is not re-baselined — it stays dirty, the
   * error routes to `onError("canonical")`, and the next save retries it.
   */
  saveAll(): Promise<string[]> {
    return this.enqueue(() => this.saveDirty(this.session.dirtyPaths()));
  }

  /** Canonically save a subset (the single-file `file.save` command). */
  save(paths: string[]): Promise<string[]> {
    const dirty = new Set(this.session.dirtyPaths());
    return this.enqueue(() => this.saveDirty(paths.filter((p) => dirty.has(p))));
  }

  /** Change the autosave cadence at runtime (`null` disables). */
  setAutosaveInterval(ms: number | null): void {
    this.autosaveMs = ms;
    this.arm();
  }

  dispose(): void {
    this.disposed = true;
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  // ── Private ──────────────────────────────────────────────────────

  private async saveDirty(paths: string[]): Promise<string[]> {
    if (this.disposed || paths.length === 0) return [];
    const files = this.session.getFiles();
    const saved: string[] = [];
    for (const path of paths) {
      const content = files[path];
      if (content === undefined) continue; // vanished; nothing to write
      try {
        await this.canonical.write(path, content);
        saved.push(path);
      } catch (e: unknown) {
        this.onError?.(e, "canonical");
      }
    }
    if (saved.length > 0) this.session.markFilesSaved(saved);
    this.arm(); // restart the autosave countdown after any save
    return saved;
  }

  private enqueue<T>(work: () => Promise<T>): Promise<T> {
    const next = this.saving.then(work, work);
    this.saving = next.catch(() => undefined);
    return next;
  }

  private arm(): void {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
    if (this.disposed || this.autosaveMs === null || this.autosaveMs <= 0) return;
    this.timer = setInterval(() => {
      if (this.session.dirtyPaths().length === 0) return; // nothing to save
      void this.saveAll();
    }, this.autosaveMs);
  }
}
