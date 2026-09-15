/**
 * Harper for grammar, the OS for spelling.
 *
 * The line between the two checkers is the whole design, so these pin where
 * it falls — and the mask, which is what lets the platform check only the
 * prose without any offset arithmetic to get wrong.
 */
import { describe, expect, it, vi } from "vitest";
import type { ProseChecker, ProseLint } from "@brink-lang/studio";
import type { SpellcheckOutcome } from "../tauri-provider.js";
import {
  desktopProseChecker,
  isReplacedBySpellchecker,
  maskToProse,
  mergeLints,
} from "../desktop-prose-checker.js";

function lint(kind: string, over: Partial<ProseLint> = {}): ProseLint {
  return { start: 0, end: 1, kind, message: kind, suggestions: [], ...over };
}

describe("the mask", () => {
  it("keeps the prose and blanks the machinery, at the same width", () => {
    // Width is the point: offsets come back in the document's own
    // coordinates and need no mapping, which removes the bug class rather
    // than testing around it.
    const text = "You have {gold} pieces";
    const spans = [
      { start: 0, end: 9 },
      { start: 15, end: 22 },
    ];
    const masked = maskToProse(text, spans);
    expect(masked).toHaveLength(text.length);
    expect(masked).toBe("You have        pieces");
    // The blanked region must not JOIN the words on either side of it.
    expect(masked.split(/\s+/).filter((w) => w !== "")).toEqual(["You", "have", "pieces"]);
  });

  it("preserves line breaks", () => {
    // Collapsing them would run a scene into one paragraph — a different
    // document to check, even with the same words.
    expect(maskToProse("a\n-> b\nc", [{ start: 0, end: 1 }, { start: 7, end: 8 }])).toBe(
      "a\n    \nc",
    );
  });

  it("survives spans that are out of order, overlapping or out of range", () => {
    // They come from an HIR projection through a subtraction; a caller
    // should not have to trust their shape.
    const text = "hello world";
    expect(
      maskToProse(text, [
        { start: 6, end: 99 },
        { start: 0, end: 3 },
        { start: 2, end: 5 },
        { start: -4, end: 1 },
      ]),
    ).toBe("hello world");
    expect(maskToProse(text, [])).toBe("           ");
  });
});

describe("which findings each checker owns", () => {
  it("replaces Spelling and keeps Typo", () => {
    // Harper's own doc: `Spelling` is "only ... used by linters doing
    // spellcheck on individual words" — exactly what the platform does
    // better. `Typo` is a real word in the wrong place ("can be seem"),
    // which a spell checker cannot see by construction.
    expect(isReplacedBySpellchecker(lint("Spelling"))).toBe(true);
    expect(isReplacedBySpellchecker(lint("Typo"))).toBe(false);
    expect(isReplacedBySpellchecker(lint("Repetition"))).toBe(false);
    expect(isReplacedBySpellchecker(lint("Agreement"))).toBe(false);
  });

  it("drops Harper's spelling and adds the platform's, keeping everything else", () => {
    const harper = [lint("Spelling"), lint("Repetition"), lint("Typo")];
    const outcome: SpellcheckOutcome = {
      kind: "checked",
      misspellings: [{ start: 4, end: 10, word: "Kaelen", suggestions: ["Kalen"] }],
    };
    const merged = mergeLints(harper, outcome);
    expect(merged.map((l) => l.kind)).toEqual(["Repetition", "Typo", "Spelling"]);
    const spelling = merged[2];
    expect(spelling.start).toBe(4);
    expect(spelling.end).toBe(10);
    expect(spelling.message).toContain("Kaelen");
    // The same `kind` string Harper used, so consumers that style or filter
    // by it keep working unchanged.
    expect(spelling.kind).toBe("Spelling");
    expect(spelling.suggestions).toEqual([{ kind: "replace", text: "Kalen" }]);
  });

  it("leaves Harper's spelling standing where the platform has no checker", () => {
    // `unavailable` is an ANSWER — Linux and Windows — not a failure. The
    // author sees no difference there.
    const harper = [lint("Spelling"), lint("Repetition")];
    const merged = mergeLints(harper, { kind: "unavailable", reason: "no native checker" });
    expect(merged.map((l) => l.kind)).toEqual(["Spelling", "Repetition"]);
  });
});

describe("the composed checker", () => {
  const builtin: ProseChecker = {
    check: () => Promise.resolve([lint("Spelling"), lint("Agreement")]),
  };

  it("sends the masked text and the project dictionary, and not the dialect", async () => {
    // `dialect` is Harper's vocabulary ("british"), not a BCP-47 tag —
    // passing it through would ask macOS for a language by that name.
    const spellcheck = vi.fn(() =>
      Promise.resolve<SpellcheckOutcome>({ kind: "checked", misspellings: [] }),
    );
    const checker = desktopProseChecker(builtin, spellcheck);
    await checker.check({
      text: "hi {x} there",
      spans: [
        { start: 0, end: 3 },
        { start: 6, end: 12 },
      ],
      dictionary: ["Kaelen"],
      dialect: "british",
    });
    expect(spellcheck).toHaveBeenCalledWith("hi     there", null, ["Kaelen"]);
  });

  it("falls back to Harper's spelling when the platform call throws", async () => {
    // A rejection would leave the PREVIOUS squiggles standing, per the
    // seam's contract — right for a transient fault, wrong as an answer.
    const checker = desktopProseChecker(builtin, () => Promise.reject(new Error("ipc died")));
    const lints = await checker.check({ text: "hi", spans: [], dictionary: [], dialect: "" });
    expect(lints.map((l) => l.kind)).toEqual(["Spelling", "Agreement"]);
  });

  it("runs both checks rather than serialising them", async () => {
    // Independent work; serialising would stack the slower one's latency on
    // the faster one for no gain.
    let released: (() => void) | undefined;
    const slowHarper: ProseChecker = {
      check: () =>
        new Promise((resolve) => {
          released = () => resolve([lint("Agreement")]);
        }),
    };
    const spellcheck = vi.fn(() =>
      Promise.resolve<SpellcheckOutcome>({ kind: "checked", misspellings: [] }),
    );
    const pending = desktopProseChecker(slowHarper, spellcheck).check({
      text: "hi",
      spans: [{ start: 0, end: 2 }],
      dictionary: [],
      dialect: "",
    });
    await Promise.resolve();
    // The platform call went out while Harper was still working.
    expect(spellcheck).toHaveBeenCalledTimes(1);
    released?.();
    expect((await pending).map((l) => l.kind)).toEqual(["Agreement"]);
  });
});
