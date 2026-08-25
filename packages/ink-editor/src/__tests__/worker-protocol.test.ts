/**
 * Session protocol substrate tests (docs/editor-worker-spec.md §5–§6, W1):
 * wire-shape cross-pins against the Rust source of truth, scheduler
 * semantics (ordering, coalescing, staleness, cancel), JSON-safety
 * enforcement, and the fake-session parity gate.
 */

import { describe, expect, it } from "vitest";
import type { SessionRequest, SessionResponse } from "@brink/wasm-types";
import { LocalTransport, type SessionServerLike } from "../worker/local-transport.js";
import { AdmissionScheduler } from "../worker/scheduler.js";
import {
  QueryDroppedError,
  QueryFailedError,
  SessionClient,
} from "../worker/session-client.js";
import type { SessionTransport } from "../worker/transport.js";

/**
 * Golden wire strings — duplicated VERBATIM from
 * `crates/brink-web/src/protocol.rs` (its `tests` module). Rust is the
 * source of truth; a change on either side must be mirrored on the
 * other or this suite / that module's pin fails. Key order matches
 * serde's struct-field order so the pins compare byte-equal.
 */
const GOLDEN_EDIT = `{"kind":"edit","doc":1,"docVersion":7,"edits":[{"from":10,"to":12,"insert":"ab"}]}`;
const GOLDEN_QUERY = `{"kind":"query","id":3,"priority":"background","doc":1,"docVersion":7,"coalesceKey":"tokens:refined:1","method":"getSegmentSemanticTokensDoc","args":[1,"4:0"]}`;
const GOLDEN_ACK = `{"kind":"ack","doc":1,"docVersion":7,"applied":true}`;
const GOLDEN_RESULT = `{"kind":"result","id":3,"docVersion":7,"configEpoch":2,"value":[]}`;
const GOLDEN_ERROR = `{"kind":"error","id":9,"message":"dropped:superseded"}`;

const settle = (): Promise<void> => new Promise((resolve) => setTimeout(resolve, 0));

interface FakeServer extends SessionServerLike {
  calls: string[];
}

function makeServer(): FakeServer {
  let epoch = 0;
  const server: FakeServer & Record<string, unknown> = {
    calls: [],
    updateDocument(doc: number, source: string) {
      server.calls.push(`push:${doc}:${source}`);
      return source === "REFUSE" ? null : { from: 0, to: 0, insert: source };
    },
    applyEditsDocument(doc: number, edits: readonly { insert: string }[]) {
      server.calls.push(`edit:${doc}:${edits.map((e) => e.insert).join(",")}`);
      return edits.length === 1;
    },
    configEpoch: () => epoch,
    setDialect() {
      server.calls.push("setDialect");
      epoch += 1;
    },
    applyProjectConfig(toml: string) {
      server.calls.push(`applyProjectConfig:${toml}`);
      return ["unrecognized key: nope"];
    },
    getThing(x: number) {
      server.calls.push(`getThing:${x}`);
      return { x, s: "π€𝄞 — ünïcode", arr: [1, [2, { k: null }]], ok: true };
    },
    getNull() {
      server.calls.push("getNull");
      return null;
    },
    getUndefined() {
      server.calls.push("getUndefined");
      return undefined;
    },
    boom() {
      throw new Error("kapow");
    },
  };
  return server;
}

function makeStack(): { server: FakeServer; client: SessionClient } {
  const server = makeServer();
  const client = new SessionClient(new LocalTransport(server));
  return { server, client };
}

describe("wire shapes (cross-pin with crates/brink-web/src/protocol.rs)", () => {
  it("serializes each envelope byte-identically to the Rust goldens", () => {
    const edit: SessionRequest = {
      kind: "edit",
      doc: 1,
      docVersion: 7,
      edits: [{ from: 10, to: 12, insert: "ab" }],
    };
    const query: SessionRequest = {
      kind: "query",
      id: 3,
      priority: "background",
      doc: 1,
      docVersion: 7,
      coalesceKey: "tokens:refined:1",
      method: "getSegmentSemanticTokensDoc",
      args: [1, "4:0"],
    };
    const ack: SessionResponse = { kind: "ack", doc: 1, docVersion: 7, applied: true };
    const result: SessionResponse = {
      kind: "result",
      id: 3,
      docVersion: 7,
      configEpoch: 2,
      value: [],
    };
    const error: SessionResponse = { kind: "error", id: 9, message: "dropped:superseded" };
    expect(JSON.stringify(edit)).toBe(GOLDEN_EDIT);
    expect(JSON.stringify(query)).toBe(GOLDEN_QUERY);
    expect(JSON.stringify(ack)).toBe(GOLDEN_ACK);
    expect(JSON.stringify(result)).toBe(GOLDEN_RESULT);
    expect(JSON.stringify(error)).toBe(GOLDEN_ERROR);
  });
});

describe("AdmissionScheduler", () => {
  const query = (
    id: number,
    priority: "interactive" | "background",
    extra: Partial<Extract<SessionRequest, { kind: "query" }>> = {},
  ): SessionRequest => ({ kind: "query", id, priority, method: "m", args: [], ...extra });

  it("runs every mutation before any query, and interactive before background", () => {
    const s = new AdmissionScheduler();
    s.enqueue(query(1, "background"));
    s.enqueue(query(2, "interactive"));
    s.enqueue({ kind: "push", doc: 1, docVersion: 1, source: "x" });
    const order = [s.nextAction(), s.nextAction(), s.nextAction()].map(
      (a) => a && a.kind === "run" && (a.request.kind === "query" ? `q${a.request.id}` : a.request.kind),
    );
    expect(order).toEqual(["push", "q2", "q1"]);
  });

  it("supersedes a background query only via an equal coalesce key", () => {
    const s = new AdmissionScheduler();
    s.enqueue(query(1, "background", { coalesceKey: "k" }));
    s.enqueue(query(2, "background", { method: "m" })); // same method, no key
    s.enqueue(query(3, "background", { coalesceKey: "k" }));
    expect(s.nextAction()).toEqual({
      kind: "drop",
      request: expect.objectContaining({ id: 1 }),
      reason: "superseded",
    });
    expect(s.nextAction()).toMatchObject({ kind: "run", request: { id: 2 } });
    expect(s.nextAction()).toMatchObject({ kind: "run", request: { id: 3 } });
  });

  it("drops a stale background query but never an interactive one", () => {
    const s = new AdmissionScheduler();
    s.enqueue(query(1, "background", { doc: 1, docVersion: 1 }));
    s.enqueue(query(2, "interactive", { doc: 1, docVersion: 1 }));
    s.enqueue({ kind: "edit", doc: 1, docVersion: 2, edits: [] });
    expect(s.nextAction()).toMatchObject({ kind: "run", request: { kind: "edit" } });
    expect(s.nextAction()).toMatchObject({ kind: "run", request: { id: 2 } });
    expect(s.nextAction()).toEqual({
      kind: "drop",
      request: expect.objectContaining({ id: 1 }),
      reason: "stale",
    });
  });

  it("cancel removes a queued query from either class", () => {
    const s = new AdmissionScheduler();
    s.enqueue(query(1, "interactive"));
    s.enqueue(query(2, "background"));
    s.enqueue({ kind: "cancel", id: 1 });
    s.enqueue({ kind: "cancel", id: 2 });
    expect(s.nextAction()).toBeNull();
  });
});

describe("LocalTransport + SessionClient", () => {
  it("applies mutations before queries posted earlier in the same task", async () => {
    const { server, client } = makeStack();
    const handle = client.query("getThing", [1], { doc: 1 });
    client.applyEdits(1, [{ from: 0, to: 0, insert: "a" }]);
    await handle.promise;
    expect(server.calls).toEqual(["edit:1:a", "getThing:1"]);
  });

  it("acks edits with applied, and refusals with applied=false", async () => {
    const { client } = makeStack();
    client.applyEdits(1, [{ from: 0, to: 0, insert: "a" }]);
    await settle();
    expect(client.lastAck(1)).toEqual({ docVersion: 1, applied: true });
    client.pushSource(1, "REFUSE");
    await settle();
    expect(client.lastAck(1)).toEqual({ docVersion: 2, applied: false });
  });

  it("coalesces background queries by key and rejects the superseded one", async () => {
    const { server, client } = makeStack();
    const first = client.query("getThing", [1], {
      priority: "background",
      coalesceKey: "thing",
    });
    const second = client.query("getThing", [2], {
      priority: "background",
      coalesceKey: "thing",
    });
    await expect(first.promise).rejects.toMatchObject({
      name: "QueryDroppedError",
      reason: "superseded",
    });
    const result = await second.promise;
    expect(result.value).toMatchObject({ x: 2 });
    expect(server.calls).toEqual(["getThing:2"]);
  });

  it("drops a background query whose doc moved before it ran", async () => {
    const { server, client } = makeStack();
    const handle = client.query("getThing", [1], { priority: "background", doc: 1 });
    client.applyEdits(1, [{ from: 0, to: 0, insert: "a" }]);
    await expect(handle.promise).rejects.toBeInstanceOf(QueryDroppedError);
    expect(server.calls).toEqual(["edit:1:a"]);
  });

  it("cancel() rejects the promise and the query never runs", async () => {
    const { server, client } = makeStack();
    const handle = client.query("getThing", [1]);
    handle.cancel();
    await expect(handle.promise).rejects.toMatchObject({ reason: "cancelled" });
    await settle();
    expect(server.calls).toEqual([]);
  });

  it("stamps results with the server's config epoch", async () => {
    const { client } = makeStack();
    client.config("setDialect", { linePrefixes: [] });
    const result = await client.query("getThing", [1]).promise;
    expect(result.configEpoch).toBe(1);
  });

  it("forwards mutation return values and thrown errors as events", async () => {
    const { client } = makeStack();
    const events: unknown[] = [];
    client.onEvent((e) => events.push(e));
    client.config("applyProjectConfig", "[project]");
    client.config("noSuchMethod");
    await settle();
    expect(events).toEqual([
      {
        type: "mutationResult",
        method: "applyProjectConfig",
        value: ["unrecognized key: nope"],
      },
      {
        type: "mutationError",
        method: "noSuchMethod",
        message: "unknown session method: noSuchMethod",
      },
    ]);
  });

  it("propagates query throws as QueryFailedError", async () => {
    const { client } = makeStack();
    await expect(client.query("boom").promise).rejects.toMatchObject({
      name: "QueryFailedError",
      message: "kapow",
    });
    await expect(client.query("noSuchMethod").promise).rejects.toBeInstanceOf(
      QueryFailedError,
    );
  });

  it("rejects JSON-unsafe payloads loudly at post time", () => {
    const { client } = makeStack();
    expect(() => client.query("getThing", [new Map([["a", 1]])])).toThrow(TypeError);
    expect(() => client.query("getThing", [{ bad: undefined }])).toThrow(TypeError);
    // The failed query must not leak a pending entry that close() would
    // then double-reject; close() after the throw is clean.
    client.close();
  });

  it("close() rejects everything pending", async () => {
    const { client } = makeStack();
    const handle = client.query("getThing", [1]);
    client.close();
    await expect(handle.promise).rejects.toMatchObject({ reason: "cancelled" });
  });
});

describe("staleness tagging (client-side, driven transport)", () => {
  it("marks a result stale when the doc moved after execution", async () => {
    const posted: SessionRequest[] = [];
    let deliver: (r: SessionResponse) => void = () => undefined;
    const transport: SessionTransport = {
      post: (r) => posted.push(r),
      setOnResponse: (l) => {
        deliver = l;
      },
      close: () => undefined,
    };
    const client = new SessionClient(transport);
    const handle = client.query("getThing", [], { doc: 1 });
    client.applyEdits(1, [{ from: 0, to: 0, insert: "a" }]); // doc moves to v1
    // The host answers with the version it executed at (v0 — before the edit).
    deliver({ kind: "result", id: handle.id, docVersion: 0, configEpoch: 0, value: 42 });
    const result = await handle.promise;
    expect(result.stale).toBe(true);
    expect(result.value).toBe(42);
  });
});

describe("parity gate (spec §11.2): async road ≡ direct calls", () => {
  it("returns byte-identical values to calling the session directly", async () => {
    const cases: { method: string; args: unknown[] }[] = [
      { method: "getThing", args: [7] },
      { method: "getThing", args: [0] },
      { method: "getNull", args: [] },
    ];
    for (const { method, args } of cases) {
      const direct = makeServer();
      const directValue = (direct as unknown as Record<string, (...a: unknown[]) => unknown>)[
        method
      ](...args);
      const { client } = makeStack();
      const viaClient = await client.query(method, args).promise;
      expect(viaClient.value).toEqual(directValue);
      expect(JSON.stringify(viaClient.value)).toBe(JSON.stringify(directValue));
    }
  });

  it("normalizes an undefined return to null (JSON has no undefined)", async () => {
    const { client } = makeStack();
    const result = await client.query("getUndefined").promise;
    expect(result.value).toBeNull();
  });
});
