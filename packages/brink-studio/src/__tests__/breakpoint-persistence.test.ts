/** Per-project breakpoint persistence (W4/#3297): lenient loader, scoped key. */
import { describe, expect, it } from "vitest";
import {
  breakpointsStorageKey,
  loadBreakpoints,
  saveBreakpoints,
} from "../breakpoint-persistence";

function memStorage(seed: Record<string, string> = {}) {
  const map = new Map(Object.entries(seed));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    dump: () => Object.fromEntries(map),
  };
}

describe("breakpoint persistence (W4/#3297)", () => {
  it("round-trips per scope", () => {
    const storage = memStorage();
    saveBreakpoints(storage, "projA", [{ file: "main.ink", line: 4, enabled: true }]);
    expect(loadBreakpoints(storage, "projA")).toEqual([
      { file: "main.ink", line: 4, enabled: true },
    ]);
    // Another project sees nothing — the scope is the isolation.
    expect(loadBreakpoints(storage, "projB")).toEqual([]);
    expect(Object.keys(storage.dump())).toEqual([breakpointsStorageKey("projA")]);
  });

  it("drops malformed entries individually and survives garbage", () => {
    const storage = memStorage({
      [breakpointsStorageKey("p")]: JSON.stringify([
        { file: "ok.ink", line: 2, enabled: false },
        { file: "", line: 1 }, // empty file — dropped
        { file: "neg.ink", line: -1 }, // negative line — dropped
        { file: "frac.ink", line: 1.5 }, // non-integer — dropped
        "not-an-object",
        { file: "default-enabled.ink", line: 0 }, // absent enabled → true
      ]),
      [breakpointsStorageKey("junk")]: "{not json",
    });
    expect(loadBreakpoints(storage, "p")).toEqual([
      { file: "ok.ink", line: 2, enabled: false },
      { file: "default-enabled.ink", line: 0, enabled: true },
    ]);
    expect(loadBreakpoints(storage, "junk")).toEqual([]);
  });

  it("degrades to empty on a throwing storage", () => {
    expect(
      loadBreakpoints(
        {
          getItem: () => {
            throw new Error("denied");
          },
        },
        "p",
      ),
    ).toEqual([]);
  });
});
