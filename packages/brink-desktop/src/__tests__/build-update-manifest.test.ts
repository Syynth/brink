/**
 * Updater-manifest assembly (D4). The filename→platform-key mapping fails
 * SILENTLY when wrong — a missing key means the app reports "up to date"
 * forever, a wrong one hands a client the other architecture's payload — so
 * it is pinned here rather than trusted.
 */

import { describe, it, expect } from "vitest";
import { buildManifest, platformKeyFor } from "../../scripts/build-update-manifest.mjs";

describe("platformKeyFor", () => {
  it("maps each updater payload to its Tauri platform key", () => {
    expect(platformKeyFor("Brink Studio.app.tar.gz")).toBe("darwin-aarch64");
    expect(platformKeyFor("Brink Studio_x64.app.tar.gz")).toBe("darwin-x86_64");
    // Real names, taken from run 32585401516's Linux job and a local macOS
    // build — not from the docs. Both carry a SPACE, which the manifest has
    // to percent-encode; the guessed names in the first draft of this test
    // did not, so the encoding path was never actually exercised.
    expect(platformKeyFor("Brink Studio_0.1.0_amd64.AppImage")).toBe("linux-x86_64");
    expect(platformKeyFor("Brink Studio_0.1.0_x64-setup.nsis.zip")).toBe("windows-x86_64");
  });

  it("REJECTS installers — a .dmg/.deb is not an update payload", () => {
    // Including one would produce a manifest the updater cannot apply.
    expect(platformKeyFor("Brink Studio_0.1.0_aarch64.dmg")).toBeNull();
    // Tauri emits a .deb.sig alongside the .deb, so "it has a signature"
    // is NOT what makes something an update payload. A deb-installed app
    // cannot rewrite itself under /usr, so AppImage is the Linux payload
    // and the .deb stays a download-only installer.
    expect(platformKeyFor("Brink Studio_0.1.0_amd64.deb")).toBeNull();
    expect(platformKeyFor("Brink Studio.app.tar.gz.sig")).toBeNull();
  });
});

describe("buildManifest", () => {
  const base = {
    dir: "/bundle",
    version: "0.2.0",
    tag: "desktop-v0.2.0",
    repo: "Syynth/brink",
    pubDate: "2026-08-22T00:00:00.000Z",
  };

  it("pairs each payload with its signature and a release download URL", () => {
    const m = buildManifest({
      ...base,
      readDir: () => ["Brink Studio.app.tar.gz", "Brink Studio.app.tar.gz.sig", "ignored.dmg"],
      readFile: () => "SIGNATURE\n",
    });
    expect(Object.keys(m.platforms)).toEqual(["darwin-aarch64"]);
    expect(m.platforms["darwin-aarch64"].signature).toBe("SIGNATURE");
    // Spaces in the filename must be encoded or the client 404s.
    expect(m.platforms["darwin-aarch64"].url).toBe(
      "https://github.com/Syynth/brink/releases/download/desktop-v0.2.0/Brink%20Studio.app.tar.gz",
    );
    expect(m.version).toBe("0.2.0");
  });

  it("THROWS on a payload with no signature rather than shipping an entry every client rejects", () => {
    expect(() =>
      buildManifest({
        ...base,
        readDir: () => ["Brink Studio.app.tar.gz"],
        readFile: () => { throw new Error("ENOENT"); },
      }),
    ).toThrow(/has no adjacent .*\.sig/);
  });

  it("THROWS rather than emitting an empty manifest", () => {
    expect(() =>
      buildManifest({ ...base, readDir: () => ["Brink Studio_0.1.0.dmg"], readFile: () => "x" }),
    ).toThrow(/refusing to emit an empty manifest/);
  });
});
