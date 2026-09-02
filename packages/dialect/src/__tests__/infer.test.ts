import { describe, expect, it } from "vitest";
import { DialectParser, inferDialect, toDialogueConfig, validateDialect } from "../index.js";
import { CORPUS } from "./infer-corpus.js";

describe("inferDialect — golden corpus", () => {
  for (const c of CORPUS) {
    it(c.id, () => {
      const r = inferDialect(c.lines);
      const sentences = r.learned.map((l) => l.sentence);
      for (const [i, needle] of c.learned.entries()) {
        expect(sentences[i], `learned[${i}] of ${JSON.stringify(sentences)}`).toContain(needle);
      }
      expect(sentences.length, JSON.stringify(sentences)).toBe(c.learned.length);
      expect(r.decisions.map((d) => d.id)).toEqual(c.decisions);
      if (c.kinds === null) {
        expect(r.dialect).toBeNull();
        return;
      }
      expect(r.dialect).not.toBeNull();
      const d = r.dialect!;
      expect((d.elements ?? []).map((e) => e.kind).sort()).toEqual(c.kinds);
      expect(validateDialect(d), "the proposed dialect must validate").toEqual([]);
      expect(toDialogueConfig(d) !== null).toBe(c.tableForm);
    });
  }

  it("support counts are re-parse results: every supported line reproduces its mark", () => {
    const c = CORPUS[0];
    const r = inferDialect(c.lines);
    const parsed = new DialectParser(r.dialect!).parseSource(c.lines.map((l) => l.text).join("\n"));
    for (const l of r.learned) {
      expect(l.support.length).toBeGreaterThan(0);
      expect(l.support.length).toBe(l.total);
      for (const i of l.support) {
        const mark = c.lines[i].mark;
        const expectKind = mark === "cue" ? "character" : mark === "narration" ? null : mark;
        expect(parsed[i].kind).toBe(expectKind);
      }
    }
  });

  it("the ambiguous-colon decision names the offending line", () => {
    const c = CORPUS.find((x) => x.id.includes("colon mid-sentence"))!;
    const r = inferDialect(c.lines);
    expect(r.decisions[0].lines).toEqual([2]);
    expect(r.decisions[0].message).toContain("marked differently");
  });
});
