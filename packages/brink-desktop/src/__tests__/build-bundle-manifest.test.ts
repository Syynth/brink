import { describe, expect, it } from "vitest";
import { archiveUrl, buildBundleManifest, sha256Hex } from "../../scripts/build-bundle-manifest.mjs";

const descriptor = { version: "0.0.2", minShellVersion: "0.7.0" };
const archive = new Uint8Array([1, 2, 3, 4]);
const base = {
  descriptor,
  archive,
  filename: "bundle-0.0.2.tar.gz",
  signature: "c2lnbmF0dXJl",
  repo: "Syynth/brink",
  tag: "bundle-v0.0.2",
  now: () => new Date("2026-09-14T12:00:00.000Z"),
};

describe("sha256Hex", () => {
  /** The shell compares this against `bundle_update::hash_matches`, which
   * produces lowercase hex — a mismatch in encoding would reject every
   * bundle. */
  it("is lowercase hex of the right length", () => {
    const digest = sha256Hex(archive);
    expect(digest).toMatch(/^[0-9a-f]{64}$/);
  });
});

describe("buildBundleManifest", () => {
  it("produces the documented shape", () => {
    const manifest = buildBundleManifest(base);
    expect(manifest).toEqual({
      version: "0.0.2",
      minShellVersion: "0.7.0",
      url: "https://github.com/Syynth/brink/releases/download/bundle-v0.0.2/bundle-0.0.2.tar.gz",
      sha256: sha256Hex(archive),
      signature: "c2lnbmF0dXJl",
      pubDate: "2026-09-14T12:00:00.000Z",
    });
  });

  /**
   * The failure this exists to prevent: with no signing key in the
   * environment, the signer writes nothing and an unsigned manifest would
   * publish happily — then be refused by every install, silently, since a
   * refusal looks like any other failed check. Fail the release job instead.
   */
  it("refuses to build a manifest with no signature", () => {
    for (const signature of ["", "   ", undefined, null]) {
      // @ts-expect-error -- deliberately invalid: the point is that the
      // RUNTIME guard catches what a caller outside TypeScript (the release
      // workflow, reading a file that may be empty) can still produce.
      expect(() => buildBundleManifest({ ...base, signature }), `signature=${signature}`).toThrow(
        /signature/i,
      );
    }
  });

  it("refuses an empty archive", () => {
    expect(() => buildBundleManifest({ ...base, archive: new Uint8Array() })).toThrow(/archive/i);
  });

  /** `minShellVersion` is mandatory at the shell's parse too — a manifest
   * without one is refused rather than defaulted, so producing one would
   * ship a bundle nothing can install. */
  it("refuses a descriptor missing either hand-maintained field", () => {
    expect(() =>
      // @ts-expect-error -- deliberately incomplete; ota-bundle.json is a
      // hand-edited file, so the runtime check is the one that matters.
      buildBundleManifest({ ...base, descriptor: { version: "0.0.2" } }),
    ).toThrow(/minShellVersion/);
    expect(() =>
      // @ts-expect-error -- as above, the other half missing.
      buildBundleManifest({ ...base, descriptor: { minShellVersion: "0.7.0" } }),
    ).toThrow(/version/);
  });

  it("carries the commit when one is supplied, and omits the key otherwise", () => {
    expect(buildBundleManifest({ ...base, commit: "deadbeef" }).commit).toBe("deadbeef");
    expect("commit" in buildBundleManifest(base)).toBe(false);
  });
});

describe("archiveUrl", () => {
  /** The archive keeps an immutable per-release URL while the MANIFEST is
   * republished to the `desktop-latest` alias — the same split latest.json
   * already uses. */
  it("points at the bundle's own release, not the alias", () => {
    expect(archiveUrl("Syynth/brink", "bundle-v0.0.3", "bundle-0.0.3.tar.gz")).toBe(
      "https://github.com/Syynth/brink/releases/download/bundle-v0.0.3/bundle-0.0.3.tar.gz",
    );
  });
});
