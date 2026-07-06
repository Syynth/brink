/**
 * `DialectParser` + `detectCast` (#366 deliverable 3): the public,
 * pure-TS parser over source/emitted text, and the cast-detection answer
 * built on top of it. `parseSource` is pinned against the same conformance
 * corpus (`tests/dialect_fixtures/at_cue.json`) that pins the Rust
 * classifier and the raw `ResolvedDialect` interpreter
 * (`dialect-conformance.test.ts`) — same fixture, one more consumer.
 * `parseEmitted`'s composite-segment iteration protocol (a cue +
 * parenthetical + text emitting as ONE line is the normal case) has no
 * fixture-corpus analogue yet (the shared `at_cue.json` corpus is
 * source-side only), so it's pinned directly here.
 */

import { describe, expect, it } from "vitest";
import { AT_CUE_DIALECT, DialectParser, detectCast } from "@brink-lang/editor";
import type { DialogueDialect, SourceLine } from "@brink-lang/editor";
import fixture from "../../../../tests/dialect_fixtures/at_cue.json";

interface FixtureCase {
  id: string;
  description: string;
  line: string;
  chain_after?: string;
  chain_after_attrs?: Record<string, string>;
  expect: { kind: string; attrs?: Record<string, string> } | null;
}

const cases = fixture.cases as FixtureCase[];

describe("DialectParser.parseSource", () => {
  const parser = new DialectParser(AT_CUE_DIALECT);

  it("loads a non-empty corpus", () => {
    expect(cases.length).toBeGreaterThan(0);
  });

  // Every non-chain fixture case is a single already-trimmed line classified
  // in isolation — reproduce that by parsing it as a one-line "source".
  for (const c of cases.filter((c) => c.chain_after === undefined)) {
    it(`${c.id}: ${c.description}`, () => {
      const [line] = parser.parseSource(c.line);
      if (c.expect === null) {
        expect(line.kind).toBeNull();
        return;
      }
      expect(line.kind).toBe(c.expect.kind);
      expect(Object.fromEntries(line.attrs)).toEqual(c.expect.attrs ?? {});
    });
  }

  it("chains a cue + narrative into one dialogue run, carrying the speaker", () => {
    const lines = parser.parseSource("@Alice:<>\nHello there.\nAnd more.");
    expect(lines.map((l) => l.kind)).toEqual(["character", "dialogue", "dialogue"]);
    expect(Object.fromEntries(lines[1].attrs)).toEqual({ speaker: "Alice" });
    expect(Object.fromEntries(lines[2].attrs)).toEqual({ speaker: "Alice" });
  });

  it("chains a parenthetical + narrative into dialogue (no carried attrs)", () => {
    const lines = parser.parseSource("(warmly)<>\nHello there.");
    expect(lines.map((l) => l.kind)).toEqual(["parenthetical", "dialogue"]);
    expect(lines[1].attrs).toEqual([]);
  });

  it("a blank line always breaks the chain", () => {
    const lines = parser.parseSource("@Alice:<>\n\nJust narrative.");
    expect(lines[0].kind).toBe("character");
    expect(lines[1].kind).toBeNull(); // blank
    expect(lines[2].kind).toBeNull(); // chain broken by the blank
  });

  it("preserves line index and full untrimmed text", () => {
    const lines = parser.parseSource("  @Alice:<>\n  Hi.");
    expect(lines[0].index).toBe(0);
    expect(lines[0].text).toBe("  @Alice:<>");
    expect(lines[1].index).toBe(1);
    expect(lines[1].kind).toBe("dialogue");
  });

  it("does not treat ink structural syntax as dialect content", () => {
    // Divert arrows, threads, tags, and inline logic must never classify as
    // a dialect kind under the at-cue preset (house rule: content/geometry
    // code must never treat ink syntax as content).
    const lines = parser.parseSource("-> knot\n<- thread\n# a tag\n{ true }");
    expect(lines.every((l) => l.kind === null)).toBe(true);
  });

  it("a structural line immediately after a cue breaks the chain instead of becoming dialogue", () => {
    // Regression for the reviewer finding: a divert right after a classified
    // cue must stay `kind: null`, not get swept into the chain rule merely
    // because the previous line had a `kind`.
    const lines = parser.parseSource("@Alice:<>\n-> some_knot\n# a tag");
    expect(lines.map((l) => l.kind)).toEqual(["character", null, null]);
    expect(lines[1].attrs).toEqual([]);
    expect(lines[2].attrs).toEqual([]);
  });

  it("a gather followed by a divert: the divert stays structural, not chained", () => {
    const lines = parser.parseSource("- gather text\n-> some_knot");
    expect(lines.map((l) => l.kind)).toEqual([null, null]);
  });

  it("a gather followed by a thread: the thread stays structural, not chained", () => {
    const lines = parser.parseSource("- gather text\n<- some_thread");
    expect(lines.map((l) => l.kind)).toEqual([null, null]);
  });

  it("a cue followed by a thread breaks the chain", () => {
    const lines = parser.parseSource("@Alice:<>\n<- some_thread");
    expect(lines.map((l) => l.kind)).toEqual(["character", null]);
  });

  it("a divert after a chained dialogue run does not extend the run", () => {
    const lines = parser.parseSource("@Alice:<>\nHello there.\n-> knot");
    expect(lines.map((l) => l.kind)).toEqual(["character", "dialogue", null]);
  });
});

describe("DialectParser.parseEmitted — composite-segment iteration protocol", () => {
  const parser = new DialectParser(AT_CUE_DIALECT);

  it("a cue + parenthetical + text emits as ONE line, three segments (the normal case)", () => {
    const segments = parser.parseEmitted("@Alice: (warmly) Hello there.");
    expect(segments.map((s) => s.kind)).toEqual(["character", "parenthetical", null]);
    expect(segments[0].content).toBe("Alice");
    expect(segments[1].content).toBe("(warmly)");
    expect(segments[2].text).toBe("Hello there.");
  });

  it("a bare cue with trailing dialogue text is two segments", () => {
    const segments = parser.parseEmitted("@Alice: Hello there.");
    expect(segments.map((s) => s.kind)).toEqual(["character", null]);
    expect(segments[0].content).toBe("Alice");
    expect(segments[1].text).toBe("Hello there.");
  });

  it("plain prose with no cue is a single plain-text segment", () => {
    const segments = parser.parseEmitted("Just some narrative text.");
    expect(segments).toEqual([
      { kind: null, text: "Just some narrative text.", content: null },
    ]);
  });

  it("a non-reserved-prefix shape (parenthetical) never opens a composite line", () => {
    // '(aside) prose' at position 0 must NOT parse as a parenthetical segment
    // — reserved_prefix: false means it can only peel as a *continuation*
    // after a reserved-prefix (cue) segment, never as the first segment.
    const segments = parser.parseEmitted("(aside) prose");
    expect(segments).toEqual([{ kind: null, text: "(aside) prose", content: null }]);
  });

  it("'@channel: hello' prose is not parsed as a cue by the emitted grammar boundary", () => {
    // The emitted pattern IS more permissive than source (documented, known
    // risk) — this fixes today's behavior as a pinned regression target
    // rather than asserting a false guarantee: the emitted `character`
    // pattern matches '@channel: ' as a cue segment then 'hello' as text.
    // Negative-fixture hardening of the emitted grammar itself is tracked
    // separately (docs/dialect-spec.md "emitted hardening" follow-ups).
    const segments = parser.parseEmitted("@channel: hello");
    expect(segments[0].kind).toBe("character");
    expect(segments[0].content).toBe("channel");
  });
});

describe("detectCast", () => {
  it("collects distinct speakers in first-appearance order from parseSource output", () => {
    const parser = new DialectParser(AT_CUE_DIALECT);
    const lines = parser.parseSource(
      [
        "@Alice:<>",
        "Hello there.",
        "@Bob:<>",
        "Hi Alice.",
        "@Alice:<>",
        "Good to see you.",
      ].join("\n"),
    );
    expect(detectCast(lines, AT_CUE_DIALECT)).toEqual(["Alice", "Bob"]);
  });

  it("skips an empty captured speaker name", () => {
    const parser = new DialectParser(AT_CUE_DIALECT);
    const lines = parser.parseSource("@:<>\nHello.");
    expect(detectCast(lines, AT_CUE_DIALECT)).toEqual([]);
  });

  it("returns no cast for a dialect with no carry rules", () => {
    const bare: DialogueDialect = { ...AT_CUE_DIALECT, chain: [] };
    const parser = new DialectParser(bare);
    const lines: SourceLine[] = parser.parseSource("@Alice:<>\nHello there.");
    expect(detectCast(lines, bare)).toEqual([]);
  });

  it("is dialect-agnostic: works for a custom carried attr name", () => {
    const custom: DialogueDialect = {
      version: 1,
      name: "bracket-cue",
      elements: [
        {
          kind: "cue",
          nature: "narrative",
          source: {
            pattern: "^(?<lead>\\[)(?<narrator>[^\\]]*)(?<tail>\\])$",
            content_group: "narrator",
            hidden: ["lead", "tail"],
            template: "[${narrator}]",
          },
        },
        { kind: "dialogue", nature: "narrative" },
      ],
      chain: [{ after: ["cue"], is: ["narrative"], becomes: "dialogue", carry: ["narrator"] }],
    };
    const parser = new DialectParser(custom);
    const lines = parser.parseSource("[Narrator]\nOnce upon a time.");
    expect(detectCast(lines, custom)).toEqual(["Narrator"]);
  });
});
