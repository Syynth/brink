import { describe, it, expect } from "vitest";

import {
  parseEvaluateSource,
  fragmentContentHash,
  cacheFragmentInto,
  isNativeEntry,
  expressionWrapSource,
  contentWrapSource,
  type FragmentCompileEntry,
} from "./evaluate-dispatch";

// Pure, wasm-free coverage of `evaluate()`'s Tier-0/Tier-1 dispatch logic —
// the classification, fragment-identity hash, and cache-eviction that decide
// which path a `source` takes and how compiled fragments are retained.

describe("parseEvaluateSource", () => {
  it("classifies a bare knot path as Tier-0 path", () => {
    expect(parseEvaluateSource("cellar")).toEqual({ kind: "path", path: "cellar" });
    expect(parseEvaluateSource("cellar.intro")).toEqual({
      kind: "path",
      path: "cellar.intro",
    });
    expect(parseEvaluateSource("a.b.c")).toEqual({ kind: "path", path: "a.b.c" });
  });

  it("trims surrounding whitespace before classifying", () => {
    expect(parseEvaluateSource("  cellar.intro  ")).toEqual({
      kind: "path",
      path: "cellar.intro",
    });
  });

  it("classifies a literal-arg call as Tier-0 call and parses the literals", () => {
    expect(parseEvaluateSource("check(1, 2)")).toEqual({
      kind: "call",
      name: "check",
      args: [1, 2],
    });
    expect(parseEvaluateSource('greet("hi", true, null, -3, 1.5)')).toEqual({
      kind: "call",
      name: "greet",
      args: ["hi", true, null, -3, 1.5],
    });
    expect(parseEvaluateSource("noargs()")).toEqual({
      kind: "call",
      name: "noargs",
      args: [],
    });
  });

  it("respects quoted commas when splitting a literal-arg list", () => {
    expect(parseEvaluateSource('say("a, b", 2)')).toEqual({
      kind: "call",
      name: "say",
      args: ["a, b", 2],
    });
  });

  it("falls through to Tier-1 (invalid) for a call with a non-literal argument", () => {
    // `gold` is an identifier, not a literal — this is a genuine fragment.
    expect(parseEvaluateSource("check(gold, 2)")).toEqual({ kind: "invalid" });
  });

  it("falls through to Tier-1 (invalid) for arbitrary expressions and content", () => {
    expect(parseEvaluateSource("has(sword) && gold > 2")).toEqual({ kind: "invalid" });
    expect(parseEvaluateSource("gold + 1")).toEqual({ kind: "invalid" });
    expect(parseEvaluateSource("You have {gold}")).toEqual({ kind: "invalid" });
    expect(parseEvaluateSource("-> cellar")).toEqual({ kind: "invalid" });
    expect(parseEvaluateSource("")).toEqual({ kind: "invalid" });
  });
});

describe("fragmentContentHash", () => {
  it("is deterministic — the same source always hashes the same", () => {
    const a = fragmentContentHash("has(sword) && gold > 2");
    const b = fragmentContentHash("has(sword) && gold > 2");
    expect(a).toBe(b);
  });

  it("produces exactly 8 lowercase hex digits", () => {
    for (const src of ["gold", "You have {gold}", "-> cellar", ""]) {
      expect(fragmentContentHash(src)).toMatch(/^[0-9a-f]{8}$/);
    }
  });

  it("distinguishes different sources", () => {
    expect(fragmentContentHash("gold")).not.toBe(fragmentContentHash("gold + 1"));
    expect(fragmentContentHash("a")).not.toBe(fragmentContentHash("b"));
    // Whitespace is significant — the hash is over the raw source.
    expect(fragmentContentHash("gold ")).not.toBe(fragmentContentHash("gold"));
  });
});

// #1598: `StoryRunnerHandle.compileFragment` must append the synthetic
// symbol using the entry's own dialect's wrap syntax — ink `=== ===` knots
// are a native parse error against a `.brink` entry, so Tier-1 fragment eval
// could never reach a native project without this.
describe("isNativeEntry", () => {
  it("is true for a .brink entry, at any depth", () => {
    expect(isNativeEntry("main.brink")).toBe(true);
    expect(isNativeEntry("chapters/main.brink")).toBe(true);
    expect(isNativeEntry("a/b/c/story.brink")).toBe(true);
  });

  it("is false for an ink entry or an extensionless one", () => {
    expect(isNativeEntry("main.ink")).toBe(false);
    expect(isNativeEntry("chapters/main.ink")).toBe(false);
    expect(isNativeEntry("main")).toBe(false);
  });

  it("is false for a dotfile with no real extension (mirrors Path::extension())", () => {
    expect(isNativeEntry(".brink")).toBe(false);
    expect(isNativeEntry("dir/.brink")).toBe(false);
  });
});

describe("expressionWrapSource", () => {
  it("wraps as a native fn for a native entry", () => {
    expect(expressionWrapSource("__eval_test", "gold + 1", true)).toBe(
      "fn __eval_test() {\n  return (gold + 1);\n}\n",
    );
  });

  it("wraps as an ink function knot for an ink entry", () => {
    expect(expressionWrapSource("__eval_test", "gold + 1", false)).toBe(
      "=== function __eval_test() ===\n~ return (gold + 1)\n",
    );
  });
});

describe("contentWrapSource", () => {
  it("wraps as a native flow for a native entry", () => {
    expect(contentWrapSource("__eval_test", "You have {gold} gold.", true)).toBe(
      "flow __eval_test() {\nYou have {gold} gold.\n}\n",
    );
  });

  it("wraps as an ink knot for an ink entry", () => {
    expect(contentWrapSource("__eval_test", "You have {gold} gold.", false)).toBe(
      "=== __eval_test ===\nYou have {gold} gold.\n",
    );
  });
});

describe("cacheFragmentInto", () => {
  const ok = (name: string): FragmentCompileEntry => ({
    ok: true,
    kind: "expression",
    symbolName: name,
    storyBytes: new Uint8Array([1]),
  });

  it("inserts and returns the entry", () => {
    const cache = new Map<string, FragmentCompileEntry>();
    const entry = ok("__eval_a");
    expect(cacheFragmentInto(cache, "k", entry, 200)).toBe(entry);
    expect(cache.get("k")).toBe(entry);
    expect(cache.size).toBe(1);
  });

  it("evicts the oldest entry (FIFO) when at the limit", () => {
    const cache = new Map<string, FragmentCompileEntry>();
    const limit = 3;
    cacheFragmentInto(cache, "a", ok("a"), limit);
    cacheFragmentInto(cache, "b", ok("b"), limit);
    cacheFragmentInto(cache, "c", ok("c"), limit);
    expect([...cache.keys()]).toEqual(["a", "b", "c"]);

    // Fourth insert at the limit evicts "a" (the oldest), keeps the rest, and
    // never over-fills — no use-after-evict.
    cacheFragmentInto(cache, "d", ok("d"), limit);
    expect(cache.size).toBe(limit);
    expect([...cache.keys()]).toEqual(["b", "c", "d"]);
    expect(cache.has("a")).toBe(false);
  });

  it("holds exactly at the 200 boundary, then evicts on overflow", () => {
    const cache = new Map<string, FragmentCompileEntry>();
    for (let i = 0; i < 200; i += 1) {
      cacheFragmentInto(cache, `k${i}`, ok(`k${i}`));
    }
    expect(cache.size).toBe(200);
    expect(cache.has("k0")).toBe(true); // nothing evicted yet at exactly the cap

    cacheFragmentInto(cache, "k200", ok("k200")); // 201st distinct key
    expect(cache.size).toBe(200);
    expect(cache.has("k0")).toBe(false); // oldest evicted
    expect(cache.has("k200")).toBe(true);
    expect(cache.has("k1")).toBe(true); // second-oldest survives
  });

  it("re-inserting an existing key does not evict (a cache hit / refresh)", () => {
    const cache = new Map<string, FragmentCompileEntry>();
    const limit = 2;
    cacheFragmentInto(cache, "a", ok("a"), limit);
    cacheFragmentInto(cache, "b", ok("b"), limit);
    // Overwriting "a" while full must not evict "b" — the map already holds
    // the key, so size doesn't grow.
    const refreshed = ok("a2");
    cacheFragmentInto(cache, "a", refreshed, limit);
    expect(cache.size).toBe(2);
    expect(cache.get("a")).toBe(refreshed);
    expect(cache.has("b")).toBe(true);
  });
});
