/**
 * `[project] unprune-dirs` — the last `brink.toml` key with no Settings
 * surface (audited 2026-08-30).
 *
 * The interesting property is that its value set is CLOSED. The walker
 * skips exactly the directories in `brink_source_tree::IGNORED_DIR_NAMES`,
 * and naming anything else un-prunes nothing — `brink-project-config`
 * answers such an entry with "it was never pruned, so this has no effect".
 * A text field could therefore only produce one of three right answers or
 * a silent typo, which is why the surface is three checkboxes.
 *
 * That closed set is a Rust constant restated in TypeScript, so the first
 * test READS THE RUST rather than repeating the three names here: a
 * literal copy would agree with itself forever while the walker moved on.
 */
import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import {
  PRUNABLE_DIRS,
  isUnpruned,
  unprunedDirs,
  withUnprunedDir,
  withoutUnprunedDir,
} from "@brink/studio-store";

const REPO = resolve(fileURLToPath(import.meta.url), "../../../../../");

const CONFIG = `[project]
entry = "main.ink"
unprune-dirs = ["node_modules"]
`;

describe("the closed value set", () => {
  it("matches brink_source_tree::IGNORED_DIR_NAMES, read from the Rust source", () => {
    const source = readFileSync(
      join(REPO, "crates/internal/brink-source-tree/src/lib.rs"),
      "utf8",
    );
    const decl = /pub const IGNORED_DIR_NAMES:\s*&\[&str\]\s*=\s*&\[([^\]]*)\]/.exec(source);
    expect(decl, "IGNORED_DIR_NAMES moved or changed shape — re-point this test").not.toBeNull();

    // Entries are string literals or consts (`GIT_DIR_NAME`); resolve the
    // named ones out of the same file so the check survives either spelling.
    const names = decl![1]
      .split(",")
      .map((piece) => piece.trim())
      .filter((piece) => piece !== "")
      .map((piece) => {
        const literal = /^"(.*)"$/.exec(piece);
        if (literal !== null) return literal[1];
        const named = new RegExp(`pub const ${piece}:\\s*&str\\s*=\\s*"([^"]*)"`).exec(source);
        expect(named, `could not resolve ${piece} in brink-source-tree`).not.toBeNull();
        return named![1];
      });

    expect([...PRUNABLE_DIRS].sort()).toEqual([...names].sort());
  });
});

describe("the pure edits", () => {
  it("reads what the config un-prunes", () => {
    expect(unprunedDirs(CONFIG)).toEqual(["node_modules"]);
    expect(isUnpruned(CONFIG, "node_modules")).toBe(true);
    expect(isUnpruned(CONFIG, "target")).toBe(false);
  });

  it("reads an empty list when the key is absent", () => {
    expect(unprunedDirs('[project]\nentry = "main.ink"\n')).toEqual([]);
  });

  it("adds in the canonical order, not click order", () => {
    // Two authors ticking the same boxes in different sequences should
    // produce the same file rather than a diff.
    const a = withUnprunedDir(withUnprunedDir(CONFIG, "target")!, ".git")!;
    const b = withUnprunedDir(withUnprunedDir(CONFIG, ".git")!, "target")!;
    expect(unprunedDirs(a)).toEqual(unprunedDirs(b));
    expect(unprunedDirs(a)).toEqual(["target", ".git", "node_modules"]);
  });

  it("returns null rather than an equal string for a no-op", () => {
    // An applied no-op still dirties the file and triggers a recompile.
    expect(withUnprunedDir(CONFIG, "node_modules")).toBeNull();
    expect(withoutUnprunedDir(CONFIG, "target")).toBeNull();
  });

  it("removes, leaving an empty list that reads as the standing policy", () => {
    const next = withoutUnprunedDir(CONFIG, "node_modules");
    expect(next).not.toBeNull();
    expect(unprunedDirs(next!)).toEqual([]);
  });

  it("keeps an entry outside the set instead of silently dropping it", () => {
    // Already the subject of a config warning; dropping it here would make
    // a typo look like an applied fix.
    const typo = '[project]\nunprune-dirs = ["node_moduls"]\n';
    expect(unprunedDirs(typo)).toEqual(["node_moduls"]);
    expect(unprunedDirs(withUnprunedDir(typo, "target")!)).toContain("node_moduls");
  });
});
