/**
 * The worker-backed prose checker (#3491).
 *
 * Prose checking was O(document) on the MAIN thread: 651 ms on a real
 * 1,125-line file, 4.8 s p95 on the 8k-line perf fixture, 700 ms after the
 * author stopped typing. The engine did not get faster — it moved. What is
 * worth pinning is therefore the transport, not the linting:
 *
 * 1. a request reaches the worker and its reply reaches the right caller;
 * 2. a check a newer edit superseded is DROPPED rather than queued behind
 *    the one in flight (the pathological case: a burst of keystrokes each
 *    scheduling a whole-document check);
 * 3. every road that has no worker — jsdom, a bundler that left the
 *    `new URL` pattern alone, a crash — still answers, in process.
 *
 * jsdom has no `Worker`, so the real `postMessage` boundary is exercised in
 * the browser (the perf scenarios). These drive a loopback fake, which is
 * what makes the ordering assertions deterministic.
 */

import { describe, expect, it, vi } from "vitest";
import type { ProseLint } from "@brink-lang/editor";
import {
  ProseCheckSuperseded,
  createProseWorker,
  createWorkerProseChecker,
  type ProseCheckRequest,
  type ProseWorkerLike,
  type ProseWorkerRequest,
  type ProseWorkerResponse,
} from "../prose-checker.js";

const lint = (start: number): ProseLint => ({
  start,
  end: start + 5,
  kind: "Spelling",
  message: "misspelling",
  suggestions: [],
});

function request(text: string): ProseCheckRequest {
  return { text, spans: [{ start: 0, end: text.length }], dictionary: [], dialect: "american" };
}

/**
 * A worker that records what it was asked and answers only when told to —
 * so a test can hold one request "in flight" and post another behind it.
 */
class FakeWorker implements ProseWorkerLike {
  readonly seen: ProseWorkerRequest[] = [];
  terminated = false;
  private onMessage: ((ev: { data: unknown }) => void) | null = null;
  private onError: ((ev: { message?: string }) => void) | null = null;

  postMessage(data: unknown): void {
    this.seen.push(data as ProseWorkerRequest);
  }

  addEventListener(type: "message", listener: (ev: { data: unknown }) => void): void;
  addEventListener(type: "error", listener: (ev: { message?: string }) => void): void;
  addEventListener(type: "message" | "error", listener: (ev: never) => void): void {
    if (type === "message") this.onMessage = listener as (ev: { data: unknown }) => void;
    else this.onError = listener as (ev: { message?: string }) => void;
  }

  terminate(): void {
    this.terminated = true;
  }

  /** Answer the request at `index` of {@link seen}. */
  reply(index: number, response: Omit<ProseWorkerResponse, "id">): void {
    const asked = this.seen[index];
    expect(asked, `no request at index ${index}`).toBeDefined();
    this.post({ ...response, id: asked.id } as ProseWorkerResponse);
  }

  /** Post a raw response — for ids this worker was never asked for. */
  post(response: ProseWorkerResponse): void {
    this.onMessage?.({ data: response });
  }

  crash(message: string): void {
    this.onError?.({ message });
  }
}

/** A fallback that records its calls, so "went in process" is observable. */
function recordingFallback() {
  const calls: ProseCheckRequest[] = [];
  return {
    calls,
    checker: {
      check: async (req: ProseCheckRequest): Promise<ProseLint[]> => {
        calls.push(req);
        return [lint(99)];
      },
    },
  };
}

describe("createWorkerProseChecker", () => {
  it("sends the request to the worker and resolves with its lints", async () => {
    const worker = new FakeWorker();
    const fallback = recordingFallback();
    const checker = createWorkerProseChecker(() => worker, fallback.checker);

    const pending = checker.check(request("The squre is empty."));
    expect(worker.seen).toHaveLength(1);
    expect(worker.seen[0]?.request.text).toBe("The squre is empty.");
    worker.reply(0, { lints: [lint(4)] });

    await expect(pending).resolves.toEqual([lint(4)]);
    // The point of the worker is that the main thread did NOT do this work.
    expect(fallback.calls).toEqual([]);
  });

  it("does not spawn the worker until the first check", () => {
    // The lazy posture the interface promises: registering a checker must
    // cost nothing, because the studio registers one unconditionally and a
    // project may never contain prose.
    const spawn = vi.fn(() => new FakeWorker());
    createWorkerProseChecker(spawn, recordingFallback().checker);
    expect(spawn).not.toHaveBeenCalled();
  });

  it("spawns once across many checks", async () => {
    const worker = new FakeWorker();
    const spawn = vi.fn(() => worker);
    const checker = createWorkerProseChecker(spawn, recordingFallback().checker);

    const first = checker.check(request("one"));
    worker.reply(0, { lints: [] });
    await first;
    const second = checker.check(request("two"));
    worker.reply(1, { lints: [] });
    await second;

    expect(spawn).toHaveBeenCalledTimes(1);
  });

  it("drops a check a newer one superseded instead of queueing it", async () => {
    // The burst case. Without coalescing, three keystroke pauses post three
    // whole-document checks and the worker grinds through all three — the
    // last of which is the only one whose answer anybody will use.
    const worker = new FakeWorker();
    const checker = createWorkerProseChecker(() => worker, recordingFallback().checker);

    const first = checker.check(request("one"));
    expect(worker.seen).toHaveLength(1);

    // Both of these arrive while `first` is still in flight. The middle one
    // is superseded before it is ever posted.
    const superseded = checker.check(request("two"));
    const newest = checker.check(request("three"));
    await expect(superseded).rejects.toBeInstanceOf(ProseCheckSuperseded);
    expect(worker.seen, "a superseded check must not reach the worker").toHaveLength(1);

    worker.reply(0, { lints: [lint(1)] });
    await expect(first).resolves.toEqual([lint(1)]);

    // Only now does the newest go out — one behind, not two.
    expect(worker.seen).toHaveLength(2);
    expect(worker.seen[1]?.request.text).toBe("three");
    worker.reply(1, { lints: [lint(3)] });
    await expect(newest).resolves.toEqual([lint(3)]);
  });

  it("ignores a reply whose id nobody is waiting for", async () => {
    // A late reply from an abandoned request must not resolve the current
    // one with the previous document's findings — the offsets would be
    // stale, which is squiggles under the wrong words rather than none.
    const worker = new FakeWorker();
    const checker = createWorkerProseChecker(() => worker, recordingFallback().checker);

    const pending = checker.check(request("one"));
    const asked = worker.seen[0];
    expect(asked).toBeDefined();

    // An id that was never issued at all.
    worker.post({ id: 9_999, lints: [lint(7)] });
    // The real reply settles the caller...
    worker.reply(0, { lints: [lint(8)] });
    // ...and a duplicate of that same id, arriving after it was settled,
    // must be dropped rather than re-settling with different offsets.
    worker.reply(0, { lints: [lint(3)] });

    await expect(pending).resolves.toEqual([lint(8)]);
  });

  it("rejects rather than resolving empty when the worker reports an error", async () => {
    // The editor treats a rejection as "leave the previous squiggles
    // standing" and `[]` as "there is nothing to show". A failed check knows
    // the second is false, so it must not claim it.
    const worker = new FakeWorker();
    const checker = createWorkerProseChecker(() => worker, recordingFallback().checker);
    const pending = checker.check(request("one"));
    worker.reply(0, { error: "wasm module unavailable" });
    await expect(pending).rejects.toThrow("wasm module unavailable");
  });

  it("falls back in process when no worker can be created", async () => {
    // jsdom, node, and any bundler that left `new URL(..., import.meta.url)`
    // alone. Checking must still happen — slowly — rather than silently stop.
    const fallback = recordingFallback();
    const checker = createWorkerProseChecker(() => null, fallback.checker);
    await expect(checker.check(request("one"))).resolves.toEqual([lint(99)]);
    expect(fallback.calls).toHaveLength(1);
  });

  it("falls back in process after the worker crashes, and terminates it", async () => {
    const worker = new FakeWorker();
    const fallback = recordingFallback();
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const checker = createWorkerProseChecker(() => worker, fallback.checker);

    const pending = checker.check(request("one"));
    worker.crash("script error");
    await expect(pending).rejects.toThrow("script error");
    expect(worker.terminated).toBe(true);

    await expect(checker.check(request("two"))).resolves.toEqual([lint(99)]);
    expect(fallback.calls).toHaveLength(1);
    warn.mockRestore();
  });
});

describe("createProseWorker", () => {
  it("returns null where there is no Worker global, rather than throwing", () => {
    // jsdom is exactly that environment, which is why the fallback road
    // above is the one this suite can reach at all.
    expect(typeof Worker).toBe("undefined");
    expect(createProseWorker()).toBeNull();
  });
});
