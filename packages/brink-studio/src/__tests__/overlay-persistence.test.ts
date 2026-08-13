/**
 * Overlay-persistence tests (the 2026-08-07 D2 ruling: celeris overlay
 * model, shared machinery in @brink-lang/editor).
 *
 * Two halves:
 *
 * 1. `FileChangeHub` under `deliveryPersists: false` — the contract change
 *    that makes the model possible: flush delivers batches (the backup-ring
 *    feed) but moves NO baselines, so dirty means "diverges from the last
 *    canonical save" and only `markSaved` clears it.
 * 2. `OverlayPersistence` — the coordinator: egress → sink, canonical
 *    saves re-baseline, autosave IS saveAll, write failures keep paths
 *    dirty for retry.
 */

import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  FileChangeHub,
  OverlayPersistence,
  type BackupEntry,
  type FileChange,
} from "@brink-lang/editor";

beforeEach(() => {
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

// ── Half 1: the hub's overlay contract ─────────────────────────────

function overlayHub() {
  const files = new Map<string, string>();
  const flushes: FileChange[][] = [];
  const hub = new FileChangeHub({
    getContent: (path) => files.get(path) ?? null,
    onFlush: (changes) => flushes.push(changes),
    debounceMs: 500,
    deliveryPersists: false,
  });
  return { files, flushes, hub };
}

describe("FileChangeHub deliveryPersists: false (overlay contract)", () => {
  it("flush delivers the batch but the path STAYS dirty", () => {
    const { files, flushes, hub } = overlayHub();
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "edited");
    hub.record("a.ink", "modified");

    vi.advanceTimersByTime(500);
    expect(flushes).toHaveLength(1);
    expect(flushes[0]).toEqual([{ path: "a.ink", type: "modified", content: "edited" }]);
    // The write-through contract would have re-baselined here; the overlay
    // contract must not — delivery fed a ring, not canonical storage.
    expect(hub.dirtyPaths()).toEqual(["a.ink"]);
  });

  it("repeated edits keep ringing while dirty persists across flushes", () => {
    const { files, flushes, hub } = overlayHub();
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "v1");
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(500);
    files.set("a.ink", "v2");
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(500);

    expect(flushes.map((b) => b[0].content)).toEqual(["v1", "v2"]);
    expect(hub.dirtyPaths()).toEqual(["a.ink"]);
  });

  it("an undo back to the CANONICAL text drops to clean (no-op vs baseline)", () => {
    const { files, hub } = overlayHub();
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "edited");
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(500); // ring hears "edited"; still dirty

    files.set("a.ink", "saved"); // undo to canonical
    hub.record("a.ink", "modified");
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("only markSaved clears dirty, exactly as a canonical save should", () => {
    const { files, hub } = overlayHub();
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "edited");
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(500);
    expect(hub.dirtyPaths()).toEqual(["a.ink"]);

    hub.markSaved(["a.ink"]);
    expect(hub.dirtyPaths()).toEqual([]);
    // Post-save, an identical re-record is a no-op against the NEW baseline.
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(500);
    expect(hub.dirtyPaths()).toEqual([]);
  });

  it("default (deliveryPersists absent) keeps the write-through contract", () => {
    const files = new Map<string, string>();
    const hub = new FileChangeHub({
      getContent: (path) => files.get(path) ?? null,
      onFlush: () => {},
      debounceMs: 500,
    });
    hub.setBaseline("a.ink", "saved");
    files.set("a.ink", "edited");
    hub.record("a.ink", "modified");
    vi.advanceTimersByTime(500);
    expect(hub.dirtyPaths()).toEqual([]); // delivery re-baselined, as before
  });
});

// ── Half 2: the coordinator ────────────────────────────────────────

function fakeSession(initial: Record<string, string>) {
  const files = new Map(Object.entries(initial));
  const baselines = new Map(Object.entries(initial));
  return {
    files,
    baselines,
    edit(path: string, content: string) {
      files.set(path, content);
    },
    dirtyPaths(): string[] {
      return [...files.keys()].filter((p) => files.get(p) !== baselines.get(p)).sort();
    },
    getFiles(): Record<string, string> {
      return Object.fromEntries(files);
    },
    markFilesSaved(paths: Iterable<string>): void {
      for (const p of paths) {
        const c = files.get(p);
        if (c !== undefined) baselines.set(p, c);
      }
    },
  };
}

function fakeSink() {
  const appended: BackupEntry[][] = [];
  return {
    appended,
    append: vi.fn((entries: BackupEntry[]) => {
      appended.push(entries);
      return Promise.resolve();
    }),
  };
}

describe("OverlayPersistence", () => {
  it("feeds egress batches to the sink, skipping deletions", () => {
    const session = fakeSession({});
    const sink = fakeSink();
    const p = new OverlayPersistence({
      session,
      canonical: { write: () => Promise.resolve() },
      sink,
      now: () => 1234,
    });

    p.handleEgress([
      { path: "a.ink", type: "modified", content: "one" },
      { path: "b.ink", type: "deleted" },
      { path: "c.ink", type: "created", content: "new" },
    ]);
    expect(sink.appended).toEqual([
      [
        { path: "a.ink", content: "one", at: 1234 },
        { path: "c.ink", content: "new", at: 1234 },
      ],
    ]);
    p.dispose();
  });

  it("saveAll writes only dirty files canonically and re-baselines them", async () => {
    const session = fakeSession({ "a.ink": "saved", "b.ink": "saved" });
    session.edit("a.ink", "edited");
    const writes: Array<[string, string]> = [];
    const p = new OverlayPersistence({
      session,
      canonical: {
        write: (path, content) => {
          writes.push([path, content]);
          return Promise.resolve();
        },
      },
    });

    const saved = await p.saveAll();
    expect(saved).toEqual(["a.ink"]);
    expect(writes).toEqual([["a.ink", "edited"]]);
    expect(session.dirtyPaths()).toEqual([]);
    p.dispose();
  });

  it("a rejected write keeps the path dirty for retry and routes onError", async () => {
    const session = fakeSession({ "a.ink": "saved", "b.ink": "saved" });
    session.edit("a.ink", "edited-a");
    session.edit("b.ink", "edited-b");
    const errors: string[] = [];
    let failOnce = true;
    const p = new OverlayPersistence({
      session,
      canonical: {
        write: (path) => {
          if (path === "a.ink" && failOnce) {
            failOnce = false;
            return Promise.reject(new Error("disk full"));
          }
          return Promise.resolve();
        },
      },
      onError: (_e, ctx) => errors.push(ctx),
    });

    expect(await p.saveAll()).toEqual(["b.ink"]);
    expect(errors).toEqual(["canonical"]);
    expect(session.dirtyPaths()).toEqual(["a.ink"]); // still dirty, retryable

    expect(await p.saveAll()).toEqual(["a.ink"]); // retry succeeds
    expect(session.dirtyPaths()).toEqual([]);
    p.dispose();
  });

  it("autosave IS saveAll: ticks save when dirty, no-op when clean", async () => {
    const session = fakeSession({ "a.ink": "saved" });
    const writes: string[] = [];
    const p = new OverlayPersistence({
      session,
      canonical: {
        write: (path) => {
          writes.push(path);
          return Promise.resolve();
        },
      },
      autosaveMs: 1000,
    });

    await vi.advanceTimersByTimeAsync(1000); // clean tick: nothing
    expect(writes).toEqual([]);

    session.edit("a.ink", "edited");
    await vi.advanceTimersByTimeAsync(1000); // dirty tick: a real save
    expect(writes).toEqual(["a.ink"]);
    expect(session.dirtyPaths()).toEqual([]);
    p.dispose();
  });

  it("save(paths) saves only the requested subset that is actually dirty", async () => {
    const session = fakeSession({ "a.ink": "saved", "b.ink": "saved" });
    session.edit("a.ink", "ea");
    session.edit("b.ink", "eb");
    const writes: string[] = [];
    const p = new OverlayPersistence({
      session,
      canonical: {
        write: (path) => {
          writes.push(path);
          return Promise.resolve();
        },
      },
    });

    expect(await p.save(["a.ink", "clean.ink"])).toEqual(["a.ink"]);
    expect(writes).toEqual(["a.ink"]);
    expect(session.dirtyPaths()).toEqual(["b.ink"]);
    p.dispose();
  });

  it("does not falsely mark a path clean when an edit lands mid-write (review, #2417)", async () => {
    // #2412 review, "Scope gaps": saveDirty snapshots session.getFiles() up
    // front, but markFilesSaved re-baselines each path to whatever content
    // is CURRENT at completion time — not to the content that was actually
    // written. An edit landing mid-write must not be baselined away.
    const session = fakeSession({ "a.ink": "v0" });
    session.edit("a.ink", "v1");
    const writes: string[] = [];
    let releaseWrite: (() => void) | undefined;
    let wroteFirst = false;
    const p = new OverlayPersistence({
      session,
      canonical: {
        write: (_path, content) => {
          writes.push(content);
          if (!wroteFirst) {
            wroteFirst = true;
            return new Promise<void>((resolve) => {
              releaseWrite = resolve;
            });
          }
          return Promise.resolve();
        },
      },
    });

    const firstSave = p.saveAll();
    await Promise.resolve();
    await Promise.resolve();

    // A new edit arrives while the v1 write is still in flight.
    session.edit("a.ink", "v2");

    releaseWrite?.();
    const saved = await firstSave;

    // v1 was written to disk; v2 is what the buffer actually holds now.
    // The path must stay dirty — v2 was never persisted.
    expect(saved).toEqual([]);
    expect(session.dirtyPaths()).toEqual(["a.ink"]);

    // A later save must still pick up and write v2.
    expect(await p.saveAll()).toEqual(["a.ink"]);
    expect(writes).toEqual(["v1", "v2"]);
    expect(session.dirtyPaths()).toEqual([]);
    p.dispose();
  });

  it("a failed ring append routes onError('backup') and never throws", () => {
    const session = fakeSession({});
    const errors: string[] = [];
    const p = new OverlayPersistence({
      session,
      canonical: { write: () => Promise.resolve() },
      sink: { append: () => Promise.reject(new Error("ring full")) },
      onError: (_e, ctx) => errors.push(ctx),
    });

    p.handleEgress([{ path: "a.ink", type: "modified", content: "x" }]);
    return vi.runAllTimersAsync().then(() => {
      expect(errors).toEqual(["backup"]);
      p.dispose();
    });
  });
});
