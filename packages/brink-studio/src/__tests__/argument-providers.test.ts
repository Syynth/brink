/**
 * Host argument providers (#175) — enumerate + push into the value cache.
 */

import { describe, it, expect, vi } from "vitest";
import type { ArgumentProvider } from "@brink/studio-shell";
import type { ValueItem } from "@brink/wasm-types";
import { pushArgumentProviderValues, type HostValueSink } from "../argument-providers.js";

function sink() {
  const calls: Record<string, ValueItem[]>[] = [];
  const session: HostValueSink = {
    setHostValues: vi.fn((v: Record<string, ValueItem[]>) => calls.push(v)),
  };
  return { session, calls };
}

describe("pushArgumentProviderValues", () => {
  it("enumerates sync + async providers and pushes the combined snapshot", async () => {
    const { session, calls } = sink();
    const providers: ArgumentProvider[] = [
      {
        type: "switch_id",
        enumerate: () => [{ value: "5", label: "HarborGate" }],
      },
      {
        type: "item_id",
        enumerate: async () => [
          { value: "1", label: "Potion", detail: "HP" },
          { value: "2", label: "Ether" },
        ],
      },
    ];

    await pushArgumentProviderValues(session, providers);

    expect(session.setHostValues).toHaveBeenCalledTimes(1);
    expect(calls[0]).toEqual({
      switch_id: [{ value: "5", label: "HarborGate" }],
      item_id: [
        { value: "1", label: "Potion", detail: "HP" },
        { value: "2", label: "Ether" },
      ],
    });
  });

  it("is a no-op with no providers", async () => {
    const { session } = sink();
    await pushArgumentProviderValues(session, []);
    expect(session.setHostValues).not.toHaveBeenCalled();
  });

  it("skips a provider that throws, keeping the rest", async () => {
    const { session, calls } = sink();
    const providers: ArgumentProvider[] = [
      { type: "good", enumerate: () => [{ value: "1", label: "One" }] },
      {
        type: "bad",
        enumerate: () => {
          throw new Error("host not ready");
        },
      },
    ];

    await pushArgumentProviderValues(session, providers);

    expect(session.setHostValues).toHaveBeenCalledTimes(1);
    expect(calls[0]).toEqual({ good: [{ value: "1", label: "One" }] });
    expect(calls[0]).not.toHaveProperty("bad");
  });
});
