/**
 * ClassifierMirror + DocHandle classifier-plane wiring (W3 of
 * docs/editor-worker-spec.md §4): cache semantics on the mirror's own
 * plane (fetch-once, key-change refetch, epoch invalidation, desync
 * fallback) and the DocHandle fast-token blend (positional pairing —
 * cached refined slices keep their colors, uncached segments serve from
 * the classifier).
 */

import { describe, expect, it } from "vitest";
import type { EditorSessionHandle, SegmentManifest } from "@brink-lang/web";
import type { LineContext, SemanticToken } from "@brink/wasm-types";
import { ClassifierMirror, type ClassifierLike } from "../classifier-mirror.js";
import { DocHandle } from "../document-handle.js";

function ctx(line: number): LineContext {
  return { line, kind: "narrative" } as unknown as LineContext;
}

function tok(line: number, tokenType: number): SemanticToken {
  return {
    line,
    start_char: 0,
    length: 3,
    token_type: tokenType,
    token_modifiers: 0,
  } as unknown as SemanticToken;
}

interface FakeClassifier extends ClassifierLike {
  fetches: string[];
  setManifest(m: SegmentManifest | null): void;
  epoch: number;
}

function makeClassifier(manifest: SegmentManifest | null): FakeClassifier {
  let current = manifest;
  const fake: FakeClassifier = {
    available: true,
    epoch: 0,
    fetches: [],
    configEpoch: () => fake.epoch,
    open: () => true,
    updateSource: () => true,
    applyEdits: () => true,
    getSegmentManifest: () => current,
    getSegmentLineContexts: (key) => {
      fake.fetches.push(`ctx:${key}`);
      return current?.segments.some((s) => s.key === key) ? [ctx(0)] : null;
    },
    getSegmentSemanticTokensFast: (key) => {
      fake.fetches.push(`tok:${key}`);
      return current?.segments.some((s) => s.key === key) ? [tok(0, 1)] : null;
    },
    setDialect: () => {
      fake.epoch += 1;
    },
    clearDialect: () => {
      fake.epoch += 1;
    },
    free: () => {},
    setManifest: (m) => {
      current = m;
    },
  };
  return fake;
}

const manifestA: SegmentManifest = {
  totalLines: 4,
  segments: [
    { key: "1:0", ownedFrom: 0 },
    { key: "2:0", ownedFrom: 2 },
  ],
};

describe("ClassifierMirror cache semantics", () => {
  it("fetches each segment once and refetches only changed keys", () => {
    const fake = makeClassifier(manifestA);
    const mirror = new ClassifierMirror(fake);
    mirror.lineContextSlices();
    mirror.lineContextSlices();
    expect(fake.fetches).toEqual(["ctx:1:0", "ctx:2:0"]);
    // Segment 2 edited: new key; segment 1 survives the edit untouched.
    fake.setManifest({
      totalLines: 4,
      segments: [
        { key: "1:0", ownedFrom: 0 },
        { key: "3:0", ownedFrom: 2 },
      ],
    });
    mirror.applyEdits([{ from: 10, to: 10, insert: "x" }]); // marks stale
    mirror.lineContextSlices();
    expect(fake.fetches).toEqual(["ctx:1:0", "ctx:2:0", "ctx:3:0"]);
  });

  it("clears the cache when the config epoch moves", () => {
    const fake = makeClassifier(manifestA);
    const mirror = new ClassifierMirror(fake);
    mirror.lineContextSlices();
    mirror.setDialect({});
    mirror.lineContextSlices();
    expect(fake.fetches).toEqual(["ctx:1:0", "ctx:2:0", "ctx:1:0", "ctx:2:0"]);
  });

  it("desync makes every read null until the next full push resyncs", () => {
    const fake = makeClassifier(manifestA);
    const mirror = new ClassifierMirror(fake);
    mirror.markDesynced();
    expect(mirror.manifest()).toBeNull();
    expect(mirror.lineContextSlices()).toBeNull();
    mirror.push("full text again");
    expect(mirror.manifest()).not.toBeNull();
  });
});

describe("DocHandle fast-token blend", () => {
  /** A fake project session with its OWN key space ("s1"/"s2"), a cached
   *  refined slice for segment 1 only, and a fast road that must NOT be
   *  hit when the classifier serves the segment. */
  function makeSession() {
    const calls: string[] = [];
    const manifest = {
      totalLines: 4,
      segments: [
        { key: "s1", ownedFrom: 0 },
        { key: "s2", ownedFrom: 2 },
      ],
    };
    const session = {
      calls,
      configEpoch: () => 0,
      getSegmentManifestDoc: () => manifest,
      getSegmentSemanticTokensDoc: (_doc: number, key: string) => {
        calls.push(`refined:${key}`);
        return key === "s1" ? [tok(0, 7)] : null;
      },
      getSegmentSemanticTokensFastDoc: (_doc: number, key: string) => {
        calls.push(`sessionFast:${key}`);
        return [tok(0, 2)];
      },
      getSegmentLineContextsDoc: (_doc: number, key: string) => {
        calls.push(`ctx:${key}`);
        return [ctx(0)];
      },
      getSemanticTokensDoc: () => {
        calls.push("wholeDoc");
        return [];
      },
      getLineContextsDoc: () => [],
      closeDocument: () => true,
    };
    return { session: session as unknown as EditorSessionHandle, calls };
  }

  it("keeps cached refined colors and serves uncached segments from the classifier", () => {
    const { session, calls } = makeSession();
    const handle = new DocHandle(session, 1, "main.ink", false);
    // Prime the refined cache: a non-fast pull caches s1's refined slice
    // (s2's refined returns null -> whole-doc fallback, nothing cached).
    handle.semanticTokens(false);
    calls.length = 0;

    const fake = makeClassifier(manifestA);
    const mirror = new ClassifierMirror(fake);
    handle.attachClassifier(mirror);

    const tokens = handle.semanticTokens(true);
    // s1 (session key) had a cached refined slice -> refined color kept;
    // s2 was uncached -> served from the classifier's positional pair
    // ("2:0"), never from the session's fast road.
    expect(tokens.map((t) => t.token_type)).toEqual([7, 1]);
    expect(calls).toEqual([]);
    expect(fake.fetches).toEqual(["tok:2:0"]);
  });

  it("prefers the classifier plane for line-context slices", () => {
    const { session, calls } = makeSession();
    const handle = new DocHandle(session, 1, "main.ink", false);
    const fake = makeClassifier(manifestA);
    handle.attachClassifier(new ClassifierMirror(fake));
    const slices = handle.lineContextSlices();
    expect(slices?.map((s) => s.key)).toEqual(["1:0", "2:0"]);
    expect(calls).toEqual([]); // session road untouched
  });
});
