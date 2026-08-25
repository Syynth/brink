/**
 * Host-level perf queries (prod-perf ruling 2026-08-25): `hostPerfReport`,
 * `hostPerfReset`, and `hostPerfSetEnabled` are answered by the HOST REALM
 * (SessionHostCore) rather than dispatched onto the session facade — the
 * only road to a worker realm's own probe state. These tests run the core
 * directly, so "host realm" here is the test realm's probe module.
 */

import { afterEach, describe, expect, it } from "vitest";
import type { SessionRequest, SessionResponse } from "@brink/wasm-types";
import { isPerfEnabled, perfReset, perfTime, setPerfEnabled } from "../perf/probe.js";
import {
  SessionHostCore,
  type HostPerfBundle,
  type SessionServerLike,
} from "../worker/session-host.js";

function makeHost(extra?: Record<string, unknown>) {
  const responses: SessionResponse[] = [];
  const server = {
    updateDocument: () => null,
    configEpoch: () => 0,
    ...extra,
  } as SessionServerLike;
  const core = new SessionHostCore(server, (r) => responses.push(r));
  const query = (id: number, method: string, args: unknown[] = []): void => {
    core.accept({ kind: "query", id, priority: "interactive", method, args } as SessionRequest);
    core.drain();
  };
  return { responses, query };
}

function resultValue(responses: SessionResponse[], id: number): unknown {
  const r = responses.find((x) => x.kind === "result" && x.id === id);
  expect(r, `no result for query ${id}: ${JSON.stringify(responses)}`).toBeDefined();
  return (r as { value: unknown }).value;
}

afterEach(() => {
  // The probe is module-global to the realm; leave it how vitest found it.
  setPerfEnabled(false);
  perfReset();
});

describe("host-level perf queries", () => {
  it("hostPerfReport bundles the realm probe with the facade's wasm counters", () => {
    setPerfEnabled(true);
    perfTime("test.span", () => {});
    const counters = { "ide.compile": { count: 2, totalMs: 10, maxMs: 7 } };
    const { responses, query } = makeHost({ getPerfCounters: () => counters });
    query(1, "hostPerfReport");
    const bundle = resultValue(responses, 1) as HostPerfBundle;
    expect(bundle.enabled).toBe(true);
    expect(bundle.wasmCounters).toEqual(counters);
    expect(bundle.probe.aggregates.some((a) => a.name === "test.span")).toBe(true);
  });

  it("hostPerfReport reports null counters when the facade has none", () => {
    const { responses, query } = makeHost();
    query(1, "hostPerfReport");
    const bundle = resultValue(responses, 1) as HostPerfBundle;
    expect(bundle.wasmCounters).toBeNull();
  });

  it("hostPerfReset clears the realm probe and the facade counters", () => {
    setPerfEnabled(true);
    perfTime("test.span", () => {});
    let facadeReset = false;
    const { responses, query } = makeHost({
      resetPerfCounters: () => {
        facadeReset = true;
      },
    });
    query(1, "hostPerfReset");
    expect(resultValue(responses, 1)).toBe(true);
    expect(facadeReset).toBe(true);
    query(2, "hostPerfReport");
    const bundle = resultValue(responses, 2) as HostPerfBundle;
    expect(bundle.probe.aggregates).toEqual([]);
  });

  it("hostPerfSetEnabled drives both planes; a non-true arg disables", () => {
    let facadeOn: boolean | null = null;
    const { responses, query } = makeHost({
      setPerfEnabled: (on: boolean) => {
        facadeOn = on;
      },
    });
    query(1, "hostPerfSetEnabled", [true]);
    expect(resultValue(responses, 1)).toBe(true);
    expect(isPerfEnabled()).toBe(true);
    expect(facadeOn).toBe(true);
    query(2, "hostPerfSetEnabled", [false]);
    expect(isPerfEnabled()).toBe(false);
    expect(facadeOn).toBe(false);
  });

  it("a facade method named like a host method can never shadow it", () => {
    const { responses, query } = makeHost({
      hostPerfReport: () => "facade-shadow",
    });
    query(1, "hostPerfReport");
    const bundle = resultValue(responses, 1) as HostPerfBundle;
    expect(typeof bundle).toBe("object");
    expect(bundle).not.toBe("facade-shadow");
    expect(bundle.probe).toBeDefined();
  });

  it("unknown methods still error (host methods are not a wildcard)", () => {
    const { responses, query } = makeHost();
    query(1, "hostPerfNoSuchThing");
    const err = responses.find((r) => r.kind === "error" && r.id === 1);
    expect(err).toBeDefined();
  });
});
