/**
 * FileChangeHub unit tests (issues #154/#137): the shared notify seam's
 * batching/debounce semantics, per-path coalescing, deferred content
 * resolution, baseline/dirty lifecycle, and the no-host-hook behavior.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { FileChangeHub, type FileChange, type FileConflict } from "@brink/ink-editor";

function harness(opts: { withFlush?: boolean; debounceMs?: number } = {}) {
  const files = new Map<string, string>();
  const flushes: FileChange[][] = [];
  const dirtyCounts: number[] = [];
  const hub = new FileChangeHub({
    getContent: (path) => files.get(path) ?? null,
    onFlush: (opts.withFlush ?? true) ? (changes) => flushes.push(changes) : undefined,
    onDirtyChange: (count) => dirtyCounts.push(count),
    debounceMs: opts.debounceMs ?? 500,
  });
  return { files, flushes, dirtyCounts, hub };
}

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("debounced batching", () => {
  it("delivers one batch after the trailing debounce, content read at flush time", () => {
    const { files, flushes, hub } = harness();
    files.set("a.ink", "one");
    hub.record("a.ink", "modified");
    files.set("a.ink", "two"); // content keeps changing before the flush
    hub.record("a.ink", "modified");
    expect(flushes).toHaveLength(0);

    vi.advanceTimersByTime(499);
    expect(flushes).toHaveLength(0);
    vi.advanceTimersByTime(1);
    expect(flushes).toEqual([[{ path: "a.ink", type: "modified", content: "two" }]]);
  });

  it("batches multiple files into one delivery, sorted by path", () => {
    const { files, flushes, hub } = harness();
    files.set("b.ink", "bee");
    files.set("a.ink", "ay");
    hub.record("b.ink", "modified");
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(500);
    expect(flushes).toEqual([
      [
        { path: "a.ink", type: "modified", content: "ay" },
        { path: "b.ink", type: "modified", content: "bee" },
      ],
    ]);
  });

  it("re-arms the trailing debounce on each record", () => {
    const { files, flushes, hub } = harness();
    files.set("a.ink", "x");
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(400);
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(400);
    expect(flushes).toHaveLength(0);
    vi.advanceTimersByTime(100);
    expect(flushes).toHaveLength(1);
  });

  it("flush() delivers immediately and disarms the timer", () => {
    const { files, flushes, hub } = harness();
    files.set("a.ink", "x");
    hub.record("a.ink", "modified");
    const delivered = hub.flush();
    expect(delivered).toEqual([{ path: "a.ink", type: "modified", content: "x" }]);
    expect(flushes).toHaveLength(1);
    vi.advanceTimersByTime(1000);
    expect(flushes).toHaveLength(1); // no double delivery
  });
});

describe("coalescing", () => {
  it("created absorbs later modified records (the host never saw the file)", () => {
    const { files, flushes, hub } = harness();
    files.set("new.ink", "");
    hub.record("new.ink", "created");
    files.set("new.ink", "draft text");
    hub.record("new.ink", "modified");
    vi.advanceTimersByTime(500);
    expect(flushes).toEqual([[{ path: "new.ink", type: "created", content: "draft text" }]]);
  });

  it("drops modified records whose content equals the baseline (no-op flushes)", () => {
    const { files, flushes, hub } = harness();
    files.set("a.ink", "same");
    hub.setBaseline("a.ink", "same");
    hub.record("a.ink", "modified"); // e.g. the initial compile flush
    vi.advanceTimersByTime(500);
    expect(flushes).toHaveLength(0);
  });

  it("an edit undone back to baseline before the flush cancels the pending change", () => {
    const { files, flushes, hub } = harness();
    files.set("a.ink", "saved");
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "edited");
    hub.record("a.ink", "modified");
    files.set("a.ink", "saved"); // undo
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(500);
    expect(flushes).toHaveLength(0);
  });
});

describe("dirty lifecycle", () => {
  it("clean at baseline → dirty on divergence → clean after flush delivery", () => {
    const { files, hub } = harness();
    files.set("a.ink", "v1");
    hub.setBaseline("a.ink", "v1");
    expect(hub.dirtyPaths()).toEqual([]);

    files.set("a.ink", "v2");
    hub.record("a.ink", "modified");
    expect(hub.dirtyPaths()).toEqual(["a.ink"]);
    expect(hub.dirtyCount()).toBe(1);

    // Delivery to the host re-baselines: "last-notified" content is synced.
    vi.advanceTimersByTime(500);
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("markSaved re-baselines without a host delivery", () => {
    const { files, flushes, hub } = harness({ withFlush: false });
    files.set("a.ink", "v1");
    hub.setBaseline("a.ink", "v1");
    files.set("a.ink", "v2");
    hub.record("a.ink", "modified");
    expect(hub.dirtyCount()).toBe(1);

    hub.markSaved(["a.ink"]);
    expect(hub.dirtyCount()).toBe(0);
    expect(flushes).toHaveLength(0);
  });

  it("a created file is dirty until saved", () => {
    const { files, hub } = harness({ withFlush: false });
    files.set("new.ink", "");
    hub.record("new.ink", "created");
    expect(hub.dirtyPaths()).toEqual(["new.ink"]);
    hub.markSaved(["new.ink"]);
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("reports dirty-count transitions through onDirtyChange", () => {
    const { files, dirtyCounts, hub } = harness({ withFlush: false });
    files.set("a.ink", "v1");
    hub.setBaseline("a.ink", "v1");
    files.set("a.ink", "v2");
    hub.record("a.ink", "modified");
    hub.record("a.ink", "modified"); // still dirty — no duplicate report
    hub.markSaved(["a.ink"]);
    expect(dirtyCounts).toEqual([1, 0]);
  });

  it("an external host change supersedes pending changes and re-baselines", () => {
    const { files, flushes, hub } = harness();
    files.set("a.ink", "studio edit");
    hub.setBaseline("a.ink", "old");
    hub.record("a.ink", "modified");

    // Host writes the file externally: its content is the new truth.
    files.set("a.ink", "host edit");
    hub.applyExternal("a.ink", "host edit");
    expect(hub.dirtyPaths()).toEqual([]);
    vi.advanceTimersByTime(500);
    expect(flushes).toHaveLength(0); // nothing echoes back to the host
  });
});

describe("without a host hook", () => {
  it("never delivers, keeps dirty state until an explicit save", () => {
    const { files, hub } = harness({ withFlush: false });
    files.set("a.ink", "v1");
    hub.setBaseline("a.ink", "v1");
    files.set("a.ink", "v2");
    hub.record("a.ink", "modified");

    vi.advanceTimersByTime(5000);
    expect(hub.flush()).toEqual([]); // no hook — nothing to deliver to
    expect(hub.dirtyPaths()).toEqual(["a.ink"]); // still unsaved

    hub.markSaved(["a.ink"]);
    expect(hub.dirtyPaths()).toEqual([]);
  });
});

describe("external conflicts (issue #320)", () => {
  /**
   * Mirrors ProjectSession's onExternalChange handler: detect first, and on a
   * conflict KEEP the buffer (no overwrite) and do NOT re-baseline — instead
   * flag the path and fire the hook. This is the exact two-part-clobber the
   * handler must stop, exercised at the hub's decision point.
   */
  function applyHostChange(
    hub: FileChangeHub,
    files: Map<string, string>,
    path: string,
    disk: string,
    onConflict: (c: FileConflict) => void,
  ): void {
    const conflict = hub.detectExternalConflict(path, disk);
    if (conflict !== null) {
      // SAFE DEFAULT: do not clobber the buffer, do not re-baseline.
      hub.markConflicted(path);
      onConflict(conflict);
      return;
    }
    // No conflict: the host's content is the new truth.
    files.set(path, disk);
    hub.applyExternal(path, disk);
  }

  it("does NOT clobber a dirty buffer; fires the hook with the right FileConflict", () => {
    const { files, hub } = harness({ withFlush: false });
    files.set("a.ink", "saved"); // host baseline
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "studio edit"); // unsaved studio buffer
    hub.record("a.ink", "modified");
    expect(hub.dirtyPaths()).toEqual(["a.ink"]);

    const conflicts: FileConflict[] = [];
    applyHostChange(hub, files, "a.ink", "host edit", (c) => conflicts.push(c));

    // The editor buffer was NOT overwritten with the host's content.
    expect(files.get("a.ink")).toBe("studio edit");
    // The hook fired with all three texts for a merge surface.
    expect(conflicts).toEqual([
      { path: "a.ink", disk: "host edit", buffer: "studio edit", baseline: "saved" },
    ]);
    // The path is flagged conflicted and remains dirty (not re-baselined).
    expect(hub.conflictedPaths()).toEqual(["a.ink"]);
    expect(hub.dirtyPaths()).toEqual(["a.ink"]);
  });

  it("a non-dirty path is updated by the external change (no conflict)", () => {
    const { files, hub } = harness({ withFlush: false });
    files.set("a.ink", "saved");
    hub.setBaseline("a.ink", "saved"); // clean — buffer === baseline
    expect(hub.dirtyPaths()).toEqual([]);

    const conflicts: FileConflict[] = [];
    applyHostChange(hub, files, "a.ink", "host edit", (c) => conflicts.push(c));

    // Clean path: the host's content wins and re-baselines.
    expect(conflicts).toEqual([]);
    expect(files.get("a.ink")).toBe("host edit");
    expect(hub.conflictedPaths()).toEqual([]);
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("no conflict when the dirty buffer already equals the host's disk content", () => {
    const { files, hub } = harness({ withFlush: false });
    files.set("a.ink", "saved");
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "converged"); // studio edited to the same text the host wrote
    hub.record("a.ink", "modified");

    expect(hub.detectExternalConflict("a.ink", "converged")).toBeNull();
  });

  it("no conflict without a baseline (the studio never synced the path)", () => {
    const { files, hub } = harness({ withFlush: false });
    files.set("new.ink", "draft");
    hub.record("new.ink", "created"); // dirty, but no baseline
    expect(hub.dirtyPaths()).toEqual(["new.ink"]);

    expect(hub.detectExternalConflict("new.ink", "host text")).toBeNull();
  });

  it("re-baselining (host re-sync) clears a standing conflict", () => {
    const { files, hub } = harness({ withFlush: false });
    files.set("a.ink", "saved");
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "studio edit");
    hub.record("a.ink", "modified");
    applyHostChange(hub, files, "a.ink", "host edit", () => {});
    expect(hub.conflictedPaths()).toEqual(["a.ink"]);

    // Reconciliation re-syncs the path to the host's content.
    files.set("a.ink", "host edit");
    hub.applyExternal("a.ink", "host edit");
    expect(hub.conflictedPaths()).toEqual([]);
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("saving the kept buffer clears a standing conflict", () => {
    const { files, hub } = harness({ withFlush: false });
    files.set("a.ink", "saved");
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "studio edit");
    hub.record("a.ink", "modified");
    applyHostChange(hub, files, "a.ink", "host edit", () => {});
    expect(hub.conflictedPaths()).toEqual(["a.ink"]);

    hub.markSaved(["a.ink"]); // user keeps their buffer and saves over disk
    expect(hub.conflictedPaths()).toEqual([]);
    expect(hub.dirtyPaths()).toEqual([]);
  });
});

describe("dispose", () => {
  it("cancels the pending timer and ignores further records", () => {
    const { files, flushes, hub } = harness();
    files.set("a.ink", "x");
    hub.record("a.ink", "modified");
    hub.dispose();
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(1000);
    expect(flushes).toHaveLength(0);
  });
});
