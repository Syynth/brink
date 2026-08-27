/**
 * `brink.toml` opens the Settings takeover, in every view (#3166, ruled
 * 2026-08-27).
 *
 * Continuous view renders the project's MANUSCRIPT, and `binderOrderedFiles`
 * deliberately filters `brink.toml` out of it — so before this, clicking the
 * config file there did nothing at all. Routing answers that once for every
 * view rather than per-view.
 *
 * This pins the ROUTE. That the Settings document then carries the whole
 * config document (form plus raw text) is a separate claim, asserted in
 * `SettingsDocument`'s own coverage — the two fail for different reasons.
 */

import { describe, expect, it } from "vitest";
import { isConfigPath } from "@brink/studio-ui";

describe("isConfigPath — the predicate the route keys off (#3166)", () => {
  it("matches a root brink.toml", () => {
    expect(isConfigPath("brink.toml")).toBe(true);
  });

  it("matches a nested brink.toml", () => {
    // A project can carry one below its root; the route must not be
    // spelled as an equality check against "brink.toml".
    expect(isConfigPath("sub/project/brink.toml")).toBe(true);
  });

  it("does not match a story file", () => {
    expect(isConfigPath("main.ink")).toBe(false);
    expect(isConfigPath("scratch/cut.ink")).toBe(false);
  });

  it("does not match a file merely NAMED like the config", () => {
    // The failure mode worth pinning: a suffix check without the separator
    // would route `notbrink.toml` — and, worse, `my.brink.toml` — into
    // Settings, where the author would find their story file missing.
    expect(isConfigPath("notbrink.toml")).toBe(false);
  });
});
