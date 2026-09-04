/**
 * The worker ENTRY's own behaviour (#3491 review): `prose-worker-checker.test.ts`
 * drives the client side of the boundary through a `FakeWorker`, so nothing
 * covered `prose-worker.ts` itself — its id echo, its `{id, error}` shape, its
 * non-array guard, or the "remembered, never retried" load memoization the
 * module doc makes a load-bearing claim about.
 *
 * The module registers a `message` listener on the worker global at import
 * time, so each case installs a fake scope, resets the module registry, and
 * imports it fresh.
 */
import { beforeEach, describe, expect, it, vi } from "vitest";

interface Posted {
  id: number;
  lints?: unknown;
  error?: string;
}

let posted: Posted[] = [];
let listener: ((ev: { data: unknown }) => void) | null = null;

/** `check_prose` behaviour for the case under test. */
let checkProse: (json: string) => string;
/** How many times the wasm module was imported+initialised. */
let loads = 0;
/** When set, the dynamic import rejects with it. */
let loadError: Error | null = null;

vi.mock("brink-prose", () => ({
  get default() {
    loads += 1;
    if (loadError) throw loadError;
    return async () => undefined;
  },
  get check_prose() {
    return (json: string) => checkProse(json);
  },
}));

async function importWorkerFresh(): Promise<void> {
  posted = [];
  listener = null;
  vi.resetModules();
  Object.assign(globalThis, {
    postMessage: (data: unknown) => posted.push(data as Posted),
    addEventListener: (type: string, fn: (ev: { data: unknown }) => void) => {
      if (type === "message") listener = fn;
    },
  });
  await import("../prose-worker.js");
}

function send(id: number, text = "The squre is empty."): void {
  listener?.({
    data: { id, request: { text, spans: [{ start: 0, end: text.length }], dictionary: [], dialect: "american" } },
  });
}

const settle = (): Promise<void> => new Promise((r) => setTimeout(r, 0));

describe("prose worker entry", () => {
  beforeEach(() => {
    loads = 0;
    loadError = null;
    checkProse = () => JSON.stringify({ lints: [{ start: 4, end: 9, kind: "Spelling", message: "x", suggestions: [] }] });
  });

  it("echoes the request id on the reply", async () => {
    await importWorkerFresh();
    send(7);
    await settle();
    expect(posted).toHaveLength(1);
    expect(posted[0]?.id).toBe(7);
    expect(posted[0]?.lints).toHaveLength(1);
  });

  it("answers a check_prose throw with the {id, error} shape, not a crash", async () => {
    await importWorkerFresh();
    checkProse = () => {
      throw new Error("boom");
    };
    send(3);
    await settle();
    expect(posted[0]).toEqual({ id: 3, error: "boom" });
  });

  it("treats a response whose `lints` is not an array as no lints", async () => {
    await importWorkerFresh();
    checkProse = () => JSON.stringify({ lints: "not an array" });
    send(1);
    await settle();
    expect(posted[0]?.lints).toEqual([]);
  });

  it("treats unparseable JSON as an error reply rather than throwing", async () => {
    await importWorkerFresh();
    checkProse = () => "{not json";
    send(2);
    await settle();
    expect(posted[0]?.id).toBe(2);
    expect(typeof posted[0]?.error).toBe("string");
  });

  it("loads the wasm module once across many checks", async () => {
    await importWorkerFresh();
    send(1);
    await settle();
    send(2);
    await settle();
    send(3);
    await settle();
    expect(posted).toHaveLength(3);
    expect(loads).toBe(1);
  });

  it("remembers a load failure instead of re-fetching 6.5 MB per check", async () => {
    await importWorkerFresh();
    loadError = new Error("module not found");
    send(1);
    await settle();
    send(2);
    await settle();
    expect(posted.map((p) => p.id)).toEqual([1, 2]);
    expect(posted.every((p) => typeof p.error === "string")).toBe(true);
    // The load promise is memoized on the first attempt: the second check
    // fails from the remembered rejection, not a second import.
    expect(loads).toBe(1);
  });
});
