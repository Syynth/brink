/** The runtime-hover merge helper (W12/#3305) — pure, jsdom-free. */
import { describe, expect, it } from "vitest";
import { augmentHoverWithRuntimeValue, identifierAt } from "../hover-runtime.js";

describe("identifierAt", () => {
  it("expands to word bounds; numbers and gaps are not identifiers", () => {
    const text = "~ gold = gold - 42";
    expect(identifierAt(text, 3)).toEqual({ name: "gold", start: 2, end: 6 });
    expect(identifierAt(text, 16)).toBeNull(); // inside "42"
    expect(identifierAt(text, 1)).toBeNull(); // the space
  });
});

describe("augmentHoverWithRuntimeValue", () => {
  const text = "~ gold = gold - 2";

  it("appends the note to an existing hover", () => {
    const out = augmentHoverWithRuntimeValue(
      text,
      3,
      { content: "**gold** — global" },
      (name) => (name === "gold" ? "`gold = 12` — runtime" : null),
    );
    expect(out?.content).toBe("**gold** — global\n\n`gold = 12` — runtime");
  });

  it("synthesizes a value-only hover anchored to the word when no base exists", () => {
    const out = augmentHoverWithRuntimeValue(text, 3, null, () => "`gold = 12`");
    expect(out).toEqual({ content: "`gold = 12`", start: 2, end: 6 });
  });

  it("no note → the base hover passes through untouched", () => {
    const base = { content: "base" };
    expect(augmentHoverWithRuntimeValue(text, 3, base, () => null)).toBe(base);
    expect(augmentHoverWithRuntimeValue(text, 3, null, () => null)).toBeNull();
  });
});
