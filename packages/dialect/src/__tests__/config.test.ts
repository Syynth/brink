import { describe, expect, it } from "vitest";
import {
  AT_CUE_DIALECT,
  affixElement,
  canonicalDialectJson,
  dialectFromConfig,
  emittedForAffix,
  toDialogueConfig,
} from "../index.js";

describe("[dialogue] table resolver mirror", () => {
  it("emittedForAffix matches the Rust derivation (dialect.rs emitted_for_affix)", () => {
    expect(emittedForAffix({ prefix: "@", suffix: ":", glued: true, content_role: "speaker" })).toEqual({
      pattern: "^@\\s*(?<speaker>[^:]*):\\s*",
      content_group: "speaker",
      reserved_prefix: true,
    });
    expect(emittedForAffix({ prefix: "> ", content_role: "content" })).toEqual({
      pattern: "^>\\ \\s*(?<content>.*)$".replace("\\ ", " "),
      content_group: "content",
      reserved_prefix: true,
    });
    expect(emittedForAffix({ suffix: ")", prefix: "(", content_role: "content" }).reserved_prefix).toBe(true);
    expect(emittedForAffix({ suffix: ":", content_role: "speaker" }).reserved_prefix).toBe(false);
  });

  it("a preset with no overlays resolves to the preset itself", () => {
    expect(canonicalDialectJson(dialectFromConfig({ preset: "at-cue" }))).toBe(
      canonicalDialectJson(AT_CUE_DIALECT),
    );
  });

  it("overlays replace by kind, run-ends-at lands on every chain rule, and the table round-trips", () => {
    const d = dialectFromConfig({
      preset: "at-cue",
      runEndsAt: ["action", "choices"],
      elements: [{ kind: "action", prefix: "> " }],
    });
    expect(d.elements?.map((e) => e.kind)).toEqual(["character", "parenthetical", "dialogue", "action"]);
    expect(d.chain?.[0]?.run_ends_at).toEqual(["action", "choices"]);
    expect(toDialogueConfig(d)).toEqual({
      preset: "at-cue",
      elements: [{ kind: "action", prefix: "> " }],
      runEndsAt: ["action", "choices"],
    });
  });

  it("run-ends-at without a chain rule is the resolver's error", () => {
    expect(() => dialectFromConfig({ runEndsAt: ["action"], elements: [{ kind: "action", prefix: ">" }] })).toThrow(
      /needs a chain rule/,
    );
  });

  it("a dialect the table cannot express projects to null", () => {
    const d = dialectFromConfig({ preset: "at-cue" });
    d.chain = [{ ...d.chain![0], after: [...d.chain![0].after, "action"] }];
    d.elements = [...(d.elements ?? []), affixElement("action", "narrative", { prefix: ">", content_role: "content" })];
    expect(toDialogueConfig(d)).toBeNull();
  });
});
