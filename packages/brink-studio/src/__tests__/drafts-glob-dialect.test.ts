/**
 * The `[project] drafts` glob dialect has TWO implementations (#3145):
 * `crates/internal/brink-project-config/src/globs.rs` — the real one — and
 * the studio mock's `matchesDraftGlob`, which exists because studio tests
 * drive draft status through the mocked wasm session.
 *
 * Two implementations of one dialect is exactly the drift hazard the
 * `draft_paths` design otherwise avoids by computing the conjunction in
 * Rust. This file is the pin: every case below is asserted verbatim by a
 * `globs.rs` unit test of the same name, so whichever side changes first
 * turns the other side red instead of quietly disagreeing.
 *
 * If you change the dialect, change BOTH and keep this table equal to the
 * Rust one. If you delete a case here, delete it there.
 */

import { describe, expect, it } from "vitest";
import { matchesDraftGlob } from "../__mocks__/brink-web.js";

/** `matchesDraftGlob` takes a list; these cases are all single-pattern. */
const m = (path: string, pattern: string): boolean => matchesDraftGlob(path, [pattern]);

describe("the drafts glob dialect (mirrors globs.rs)", () => {
  it("a literal pattern matches only itself", () => {
    expect(m("scratch/cut.ink", "scratch/cut.ink")).toBe(true);
    expect(m("scratch/cut.ink", "scratch/cut.brink")).toBe(false);
    expect(m("scratch/cut.ink", "scratch/cut")).toBe(false);
    expect(m("a/scratch/cut.ink", "scratch/cut.ink")).toBe(false);
  });

  it("a star stops at a separator", () => {
    expect(m("scratch/cut.ink", "scratch/*.ink")).toBe(true);
    expect(m("scratch/cut.ink", "*/cut.ink")).toBe(true);
    expect(m("scratch/deep/cut.ink", "scratch/*.ink")).toBe(false);
    expect(m("scratch/cut.ink", "*.ink")).toBe(false);
  });

  it("a double star crosses separators", () => {
    expect(m("scratch/deep/cut.ink", "scratch/**")).toBe(true);
    expect(m("scratch/deep/cut.ink", "**/cut.ink")).toBe(true);
    expect(m("scratch/deep/cut.ink", "**.ink")).toBe(true);
    expect(m("scratch/cut.ink", "scratch/**/cut.ink")).toBe(true);
    expect(m("scratch/cut.ink", "**/scratch/cut.ink")).toBe(true);
    expect(m("notes/cut.ink", "scratch/**")).toBe(false);
  });

  it("a trailing slash is sugar for everything under it", () => {
    expect(m("scratch/cut.ink", "scratch/")).toBe(true);
    expect(m("scratch/deep/cut.ink", "scratch/")).toBe(true);
    expect(m("scratch", "scratch/")).toBe(false);
    expect(m("scratchpad/cut.ink", "scratch/")).toBe(false);
  });

  it("a bare directory name does not cover its contents", () => {
    // The documented departure from gitignore, on both sides.
    expect(m("scratch/cut.ink", "scratch")).toBe(false);
    expect(m("scratch", "scratch")).toBe(true);
  });

  it("a question mark is one non-separator character", () => {
    expect(m("act1.ink", "act?.ink")).toBe(true);
    expect(m("act10.ink", "act?.ink")).toBe(false);
    expect(m("a/b", "a?b")).toBe(false);
  });

  it("matching is case-sensitive", () => {
    expect(m("Scratch/cut.ink", "scratch/**")).toBe(false);
  });

  it("an empty pattern matches nothing", () => {
    expect(m("", "")).toBe(false);
    expect(m("cut.ink", "")).toBe(false);
  });

  it("matches any is the or of its patterns and tolerates a dot slash", () => {
    const pats = ["scratch/**", "*.draft.ink"];
    expect(matchesDraftGlob("scratch/cut.ink", pats)).toBe(true);
    expect(matchesDraftGlob("./scratch/cut.ink", pats)).toBe(true);
    expect(matchesDraftGlob("aside.draft.ink", pats)).toBe(true);
    expect(matchesDraftGlob("main.ink", pats)).toBe(false);
    expect(matchesDraftGlob("main.ink", [])).toBe(false);
  });

  it("a regex metacharacter in a pattern is literal, not a metacharacter", () => {
    // Only the mock compiles to a regex, so only the mock can get this
    // wrong; the Rust matcher walks bytes and has no such failure mode.
    // Asserted here rather than in globs.rs for that reason.
    expect(m("a.ink", "a.ink")).toBe(true);
    expect(m("axink", "a.ink")).toBe(false);
    expect(m("notes(1).ink", "notes(1).ink")).toBe(true);
    expect(m("a+b.ink", "a+b.ink")).toBe(true);
    expect(m("aab.ink", "a+b.ink")).toBe(false);
  });
});
