import { describe, expect, it } from "vitest";
import {
  archiveUrl,
  buildIndexEntry,
  mergeIntoIndex,
  sha256Hex,
  MAX_INDEX_ENTRIES,
} from "../../scripts/build-bundle-index.mjs";

const descriptor = { version: "0.0.2", minShellVersion: "0.8.0", channel: "stable" };
const archive = new Uint8Array([1, 2, 3]);
const base = {
  descriptor,
  archive,
  filename: "bundle-0.0.2.tar.gz",
  signature: "c2ln",
  repo: "Syynth/brink",
  tag: "bundle-v0.0.2",
  now: () => new Date("2026-09-15T00:00:00Z"),
};

describe("buildIndexEntry", () => {
  it("carries everything verify_and_unpack needs, plus the channel", () => {
    const entry = buildIndexEntry(base);
    expect(entry).toMatchObject({
      version: "0.0.2",
      channel: "stable",
      minShellVersion: "0.8.0",
      url: archiveUrl("Syynth/brink", "bundle-v0.0.2", "bundle-0.0.2.tar.gz"),
      sha256: sha256Hex(archive),
      signature: "c2ln",
    });
  });

  /**
   * A typo'd channel parses on the Rust side as the serde default — stable —
   * so it would not fail, it would ship a beta to every stable install. The
   * closed list is the only place that can catch it.
   */
  it("refuses a channel that is not on the closed list", () => {
    for (const channel of ["Stable", "nightly", ""]) {
      expect(
        () => buildIndexEntry({ ...base, descriptor: { ...descriptor, channel } }),
        String(channel),
      ).toThrow(/channel/);
    }
  });

  /**
   * Absent — including an explicit null from a hand-edit — falls back to
   * stable. That is the SAFE direction and the reason it is a default rather
   * than an error: the hazard this validation exists to stop is a beta
   * reaching stable installs, and defaulting to stable can never cause it.
   * An empty string is a different thing and still throws.
   */
  it("defaults an absent channel to stable rather than guessing", () => {
    const { channel: _omitted, ...withoutChannel } = descriptor;
    expect(buildIndexEntry({ ...base, descriptor: withoutChannel }).channel).toBe("stable");
    expect(
      buildIndexEntry({ ...base, descriptor: { ...descriptor, channel: null } }).channel,
    ).toBe("stable");
  });

  /** An unsigned bundle is refused by every install, and a refusal looks
   *  like any other failed check — so this fails the release job instead. */
  it("refuses an empty signature", () => {
    expect(() => buildIndexEntry({ ...base, signature: "   " })).toThrow(/signature is empty/);
  });
});

describe("mergeIntoIndex", () => {
  const entry = (version: string) => ({ ...buildIndexEntry(base), version });

  it("puts the new entry first and keeps the rest", () => {
    const merged = mergeIntoIndex({ entries: [entry("0.0.1")] }, entry("0.0.2"));
    expect(merged.entries.map((e: { version: string }) => e.version)).toEqual(["0.0.2", "0.0.1"]);
  });

  /** Two entries for one version would make "latest" depend on scan order,
   *  and the shell resolves the FIRST match. */
  it("replaces a re-published version instead of duplicating it", () => {
    const merged = mergeIntoIndex(
      { entries: [entry("0.0.2"), entry("0.0.1")] },
      { ...entry("0.0.2"), signature: "replaced" },
    );
    expect(merged.entries).toHaveLength(2);
    expect(merged.entries[0]).toMatchObject({ version: "0.0.2", signature: "replaced" });
  });

  /** Merging into damage would either throw on every future release or
   *  carry it forward. Losing history is recoverable; losing the channel
   *  is not. */
  it("treats a malformed existing index as empty rather than inheriting it", () => {
    for (const broken of [null, undefined, {}, { entries: "nope" }]) {
      const merged = mergeIntoIndex(broken, entry("0.0.2"));
      expect(merged.entries, JSON.stringify(broken)).toHaveLength(1);
    }
  });

  /** The cap drops the oldest PUBLISHED, not the lowest version: a hotfix
   *  to an old line must not evict something newer. */
  it("caps the index by publication order", () => {
    const existing = {
      entries: Array.from({ length: MAX_INDEX_ENTRIES }, (_unused, i) => entry(`0.0.${i + 1}`)),
    };
    const merged = mergeIntoIndex(existing, entry("9.9.9"));
    expect(merged.entries).toHaveLength(MAX_INDEX_ENTRIES);
    expect(merged.entries[0].version).toBe("9.9.9");
    expect(merged.entries.map((e: { version: string }) => e.version)).not.toContain(
      `0.0.${MAX_INDEX_ENTRIES}`,
    );
  });
});
