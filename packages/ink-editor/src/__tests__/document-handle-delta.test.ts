import { describe, expect, it, vi } from "vitest";
import type { EditorSessionHandle } from "@brink-lang/web";
import { DocHandle } from "../document-handle";

/**
 * The outbound-delta slice cache (#3064 option A + micro): version-keyed
 * reuse, manifest sharing, and the config-epoch invalidation that keeps a
 * dialect swap from serving stale dialect-classified slices under
 * unchanged identity keys.
 */

interface StubState {
  manifest: { totalLines: number; segments: { key: string; ownedFrom: number }[] };
  contexts: Record<string, unknown[]>;
  tokens: Record<string, { line: number; startChar: number }[]>;
  epoch: number;
}

function stubSession(state: StubState) {
  const session = {
    configEpoch: vi.fn(() => state.epoch),
    getSegmentManifestDoc: vi.fn(() => structuredClone(state.manifest)),
    getSegmentLineContextsDoc: vi.fn(
      (_d: number, key: string) => structuredClone(state.contexts[key]) ?? null,
    ),
    getSegmentSemanticTokensDoc: vi.fn(
      (_d: number, key: string) => structuredClone(state.tokens[key]) ?? null,
    ),
    getLineContextsDoc: vi.fn(() => []),
    getSemanticTokensDoc: vi.fn(() => []),
    applyEditsDocument: vi.fn(() => true),
  };
  return session as unknown as EditorSessionHandle & typeof session;
}

function baseState(): StubState {
  return {
    manifest: {
      totalLines: 4,
      segments: [
        { key: "1:0", ownedFrom: 0 },
        { key: "2:0", ownedFrom: 2 },
      ],
    },
    contexts: { "1:0": ["h0", "h1"], "2:0": ["k0", "k1"] },
    tokens: {
      "1:0": [{ line: 0, startChar: 0 }],
      "2:0": [{ line: 1, startChar: 3 }],
    },
    epoch: 0,
  };
}

describe("DocHandle outbound-delta slice cache", () => {
  it("assembles from slices, fetching each segment once and reusing on repeat queries", () => {
    const state = baseState();
    const session = stubSession(state);
    const handle = new DocHandle(session, 1, "main.ink", false);

    expect(handle.lineContexts()).toEqual(["h0", "h1", "k0", "k1"]);
    expect(handle.lineContexts()).toEqual(["h0", "h1", "k0", "k1"]);
    // Two segments fetched exactly once; the manifest fetched once (shared
    // across repeat queries until the doc changes).
    expect(session.getSegmentLineContextsDoc).toHaveBeenCalledTimes(2);
    expect(session.getSegmentManifestDoc).toHaveBeenCalledTimes(1);
  });

  it("re-fetches only the changed segment after an edit; shifted tokens rebase by ownedFrom", () => {
    const state = baseState();
    const session = stubSession(state);
    const handle = new DocHandle(session, 1, "main.ink", false);
    expect(handle.semanticTokens()).toEqual([
      { line: 0, startChar: 0 },
      { line: 3, startChar: 3 },
    ]);

    // Edit inside the header: new key for it; knot shifts down a line but
    // keeps its key — its cached slice must be reused at the new offset.
    handle.applyChanges([{ from: 0, to: 0, insert: "x\n" }]);
    state.manifest = {
      totalLines: 5,
      segments: [
        { key: "3:0", ownedFrom: 0 },
        { key: "2:0", ownedFrom: 3 },
      ],
    };
    state.contexts["3:0"] = ["h0", "h1", "h2"];
    state.tokens["3:0"] = [{ line: 0, startChar: 1 }];

    expect(handle.semanticTokens()).toEqual([
      { line: 0, startChar: 1 },
      { line: 4, startChar: 3 },
    ]);
    // "2:0" was cached — only the new "3:0" slice was fetched.
    const fetched = session.getSegmentSemanticTokensDoc.mock.calls.map((c) => c[1]);
    expect(fetched).toEqual(["1:0", "2:0", "3:0"]);
  });

  it("a config-epoch bump (dialect swap) invalidates cached slices under unchanged keys", () => {
    const state = baseState();
    const session = stubSession(state);
    const handle = new DocHandle(session, 1, "main.ink", false);
    expect(handle.lineContexts()).toEqual(["h0", "h1", "k0", "k1"]);

    // Dialect swap: same keys, different classification content.
    state.epoch = 1;
    state.contexts = { "1:0": ["H0", "H1"], "2:0": ["K0", "K1"] };
    expect(handle.lineContexts()).toEqual(["H0", "H1", "K0", "K1"]);
  });

  it("falls back wholesale when a slice key goes stale mid-assembly", () => {
    const state = baseState();
    const session = stubSession(state);
    delete state.contexts["2:0"]; // manifest names a key the fetch refuses
    const handle = new DocHandle(session, 1, "main.ink", false);
    handle.lineContexts();
    expect(session.getLineContextsDoc).toHaveBeenCalledTimes(1);
  });
});
