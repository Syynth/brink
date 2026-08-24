/**
 * #395 — generalize the two remaining at-cue-hardcoded sites so custom
 * dialects are functionally complete: a dialect must handle everything the
 * hardcoded path does (hiding/inserting sigils, converting).
 *
 *  1. `extractLineContent`/`CONVERTIBLE_TYPES` (@brink/ink-operations):
 *     `executeDialectRow`'s `convert` action (transitions.ts) now extracts
 *     content via the resolved dialect's OWN declared shapes
 *     (`ResolvedDialect.convertibleShapes()`), not the hardcoded
 *     `@name:<>`/`(text)<>` regexes — proven end-to-end with a custom
 *     non-at-cue dialect's `transitions` row below.
 *  2. `inline-markup.ts` content-region widths: `contentRegions` derives a
 *     Character/Parenthetical-shaped kind's content bounds from the line's
 *     cached dialect geometry (`LineInfo.dialect.contentSpan`) when given,
 *     instead of the fixed at-cue affix-length constants.
 *
 * Byte-parity for the default (at-cue) preset is the acceptance gate — both
 * suites below have a "default preset unchanged" companion case.
 */

import { afterEach, describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import {
  brinkStudio,
  extendDialect,
  AT_CUE_DIALECT,
  extractLineContent,
  contentRegions,
  elementTypeField,
  ResolvedDialect,
  type DialogueDialect,
  type ConvertibleShape,
} from "@brink-lang/editor";

const minimal = {
  compile: () => ({ ok: true, diagnostics: [] }) as never,
  getSemanticTokens: () => [],
  getTokenTypeNames: () => [],
};

function mount(doc: string, extra: Partial<Parameters<typeof brinkStudio>[0]> = {}): EditorView {
  return new EditorView({
    state: EditorState.create({ doc, extensions: [brinkStudio({ ...minimal, ...extra })] }),
    parent: document.body,
  });
}

afterEach(() => {
  document.body.replaceChildren();
});

// ── Site 1: extractLineContent / CONVERTIBLE_TYPES ──────────────────

describe("extractLineContent — custom shapes (#395)", () => {
  it("default preset (no shapes arg) stays byte-identical: @Name:<> and (text)<> extract as before", () => {
    expect(extractLineContent("@Alice:<>")).toBe("Alice");
    expect(extractLineContent("(whispering)<>")).toBe("whispering");
    expect(extractLineContent("~ x = 1")).toBe("x = 1"); // prefix-sigil fallback unaffected
  });

  it("extracts via a custom (non-at-cue) shape passed explicitly", () => {
    const shapes: ConvertibleShape[] = [
      { pattern: "^<<(?<content>[^>]*)>>$", contentGroup: "content" },
    ];
    expect(extractLineContent("<<radio>>", shapes)).toBe("radio");
  });

  it("custom shapes are tried before falling back to the at-cue defaults", () => {
    const shapes: ConvertibleShape[] = [
      { pattern: "^\\[\\[(?<content>[^\\]]*)\\]\\]$", contentGroup: "content" },
    ];
    // A custom-shaped line extracts via the custom shape...
    expect(extractLineContent("[[aside text]]", shapes)).toBe("aside text");
    // ...while an at-cue-shaped line still falls through to the built-in
    // shapes when the custom list doesn't match.
    expect(extractLineContent("@Bob:<>", shapes)).toBe("Bob");
  });
});

describe("dialect convert-row generalization: executeDialectRow (#395)", () => {
  // A custom dialect with NO at-cue shapes at all: `<<name>>` (channel) and
  // `[[text]]` (aside), plus a `transitions` row converting channel → aside
  // on Tab. This is the concrete failing consumer named in the issue: before
  // the fix, `executeDialectRow`'s `convert` action called the hardcoded
  // `extractLineContent(line.text)` (no shapes), which cannot match
  // `<<radio>>` at all and would fall through to prefix-sigil stripping
  // (empty content, since `<<radio>>` has no recognized prefix sigil) —
  // producing `[[]]` instead of `[[radio]]`.
  const CUSTOM_DIALECT: DialogueDialect = extendDialect(AT_CUE_DIALECT, {
    elements: [
      {
        kind: "channel",
        nature: "narrative",
        source: {
          pattern: "^(?<lead><<)(?<name>[^>]*)(?<tail>>>)$",
          content_group: "name",
          hidden: ["lead", "tail"],
          template: "<<${name}>>",
        },
      },
      {
        kind: "aside",
        nature: "narrative",
        source: {
          pattern: "^(?<lead>\\[\\[)(?<text>[^\\]]*)(?<tail>\\]\\])$",
          content_group: "text",
          hidden: ["lead", "tail"],
          template: "[[${text}]]",
        },
      },
    ],
    transitions: [{ on: "channel", key: "Tab", action: { action: "convert", kind: "aside" } }],
  });

  it("converts a custom dialect's channel line to aside via Tab, extracting the non-at-cue content correctly", () => {
    const view = mount("<<radio>>\n", { dialect: CUSTOM_DIALECT });
    expect(view.state.field(elementTypeField)[0].type).toBe("channel");

    view.dispatch({ selection: { anchor: view.state.doc.line(1).to } });
    const handled = runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Tab" }), "editor");

    expect(handled).toBe(true);
    // The bug: extraction via the hardcoded at-cue regexes would produce ""
    // (no @...:<> / (...)<> match, no recognized prefix sigil), yielding
    // "[[]]" — the fix threads the dialect's own `channel` shape through so
    // "radio" is extracted and the target `aside` template is filled.
    expect(view.state.doc.toString()).toBe("[[radio]]\n");
    expect(view.state.field(elementTypeField)[0].type).toBe("aside");
    view.destroy();
  });

  it("preserves indentation across the dialect convert row (indentation-preserved contract)", () => {
    const view = mount("  <<radio>>\n", { dialect: CUSTOM_DIALECT });
    view.dispatch({ selection: { anchor: view.state.doc.line(1).to } });
    runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Tab" }), "editor");
    expect(view.state.doc.toString()).toBe("  [[radio]]\n");
    view.destroy();
  });

  it("default preset is unaffected: at-cue ships transitions: [], so Tab on a Character line INDENTS (built-in rows stripped, ruled 2026-08-24)", () => {
    const view = mount("@Alice:<>\n");
    view.dispatch({ selection: { anchor: view.state.doc.line(1).to } });
    const handled = runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Tab" }), "editor");
    expect(handled).toBe(true);
    // No dialect row claims Tab and the built-in conversion cycle is
    // stripped — NO CONVERSION happens (the pin this test exists for).
    // Whether the indent lands is the screenplay sigil edit-guard's call
    // on a Character line (the atomic `@` sits at line start), so accept
    // an indented or unchanged line — never a converted one.
    expect(view.state.doc.toString()).toMatch(/^\s*@Alice:<>\n$/);
    view.destroy();
  });

  // `strip` is the SAME switch as `convert` in `executeDialectRow` — before
  // this fix it still called the bare `extractLineContent(line.text)` (no
  // shapes), so a custom non-at-cue dialect's `strip` row would fail to
  // extract content identically to the pre-#395 `convert` bug: no
  // `@...:<>`/`(...)<>` match and no recognized prefix sigil, so the line
  // would strip to empty instead of its actual content.
  it("strips a custom dialect's channel line to its bare content via Shift-Tab, extracting the non-at-cue content correctly", () => {
    const STRIP_DIALECT: DialogueDialect = extendDialect(AT_CUE_DIALECT, {
      elements: [
        {
          kind: "channel",
          nature: "narrative",
          source: {
            pattern: "^(?<lead><<)(?<name>[^>]*)(?<tail>>>)$",
            content_group: "name",
            hidden: ["lead", "tail"],
            template: "<<${name}>>",
          },
        },
      ],
      transitions: [{ on: "channel", key: "Shift-Tab", action: { action: "strip" } }],
    });
    const view = mount("<<radio>>\n", { dialect: STRIP_DIALECT });
    expect(view.state.field(elementTypeField)[0].type).toBe("channel");

    view.dispatch({ selection: { anchor: view.state.doc.line(1).to } });
    const handled = runScopeHandlers(
      view,
      new KeyboardEvent("keydown", { key: "Tab", shiftKey: true }),
      "editor",
    );

    expect(handled).toBe(true);
    // The bug: extraction via the hardcoded at-cue regexes would produce ""
    // (no @...:<> / (...)<> match, no recognized prefix sigil) — the fix
    // threads the dialect's own `channel` shape through so "radio" survives
    // the strip.
    expect(view.state.doc.toString()).toBe("radio\n");
    view.destroy();
  });

  // #406 — the at-cue preset's `parenthetical` element has a wrap-inclusive
  // `content_group` ("content" spans "(text)", parens included — needed so
  // `content_span`/markup geometry keeps the parens as visible/editable
  // content). Before this fix, `content_group` doubled as the convert/strip
  // round-trip's fill group too, so a `convert` row targeting `parenthetical`
  // from a bare-content source kind rendered `template` ("${content}<>")
  // with the bare extracted text, producing "radio<>" — missing the opening
  // paren entirely, since the literal "(" only ever appeared inside the
  // (never-hit-for-this-direction) `content_group` capture, not in
  // `template`. `template_group` (new, additive) names a separate bare-text
  // group for exactly this fill purpose, so `convert` re-wraps correctly.
  it("converts a custom dialect's channel line to the built-in parenthetical kind via Tab, correctly wrapping bare content in parens (round-trip)", () => {
    const DIALECT: DialogueDialect = extendDialect(AT_CUE_DIALECT, {
      elements: [
        {
          kind: "channel",
          nature: "narrative",
          source: {
            pattern: "^(?<lead><<)(?<name>[^>]*)(?<tail>>>)$",
            content_group: "name",
            hidden: ["lead", "tail"],
            template: "<<${name}>>",
          },
        },
      ],
      transitions: [{ on: "channel", key: "Tab", action: { action: "convert", kind: "parenthetical" } }],
    });
    const view = mount("<<radio>>\n", { dialect: DIALECT });
    expect(view.state.field(elementTypeField)[0].type).toBe("channel");

    view.dispatch({ selection: { anchor: view.state.doc.line(1).to } });
    const handled = runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Tab" }), "editor");

    expect(handled).toBe(true);
    // The bug: rendering `${content}<>` with the bare "radio" would produce
    // "radio<>" (no opening paren). The fix's `template_group` ("content_inner")
    // is filled instead, against `template` "(${content_inner})<>" — correctly
    // wrapped.
    expect(view.state.doc.toString()).toBe("(radio)<>\n");
    expect(view.state.field(elementTypeField)[0].type).toBe("parenthetical");
    view.destroy();
  });

  // The reverse direction — stripping a real Parenthetical line — must stay
  // byte-identical: bare content, no parens (matches the built-in
  // `stripToNarrative`/`DEFAULT_CONVERTIBLE_SHAPES` convention this fix
  // reconciles `parenthetical`'s round-trip semantics with).
  it("strips a real Parenthetical line to its bare content via a dialect strip row (parens dropped, round-trip byte-identical)", () => {
    const DIALECT: DialogueDialect = extendDialect(AT_CUE_DIALECT, {
      transitions: [{ on: "parenthetical", key: "Shift-Tab", action: { action: "strip" } }],
    });
    const view = mount("(warmly)<>\n", { dialect: DIALECT });
    expect(view.state.field(elementTypeField)[0].type).toBe("parenthetical");

    view.dispatch({ selection: { anchor: view.state.doc.line(1).to } });
    const handled = runScopeHandlers(
      view,
      new KeyboardEvent("keydown", { key: "Tab", shiftKey: true }),
      "editor",
    );

    expect(handled).toBe(true);
    expect(view.state.doc.toString()).toBe("warmly\n");
    view.destroy();
  });

  // #406 — `template_group` is additive and must not disturb the OTHER thing
  // `parenthetical`'s `content_group` drives: `content_span` geometry (markup
  // scoping) and classification attrs. Byte-identical to before this fix.
  it("parenthetical classification geometry/attrs stay byte-identical (content_group unchanged, template_group is additive)", () => {
    const resolved = ResolvedDialect.compile(AT_CUE_DIALECT);
    const match = resolved.classify("(warmly)<>", 0);
    expect(match?.kind).toBe("parenthetical");
    // contentSpan still spans the parens-inclusive outer group — unchanged.
    expect(match?.contentSpan).toEqual([0, 8]);
    // No new attr for the inner `content_inner` group.
    expect(match?.attrs).toEqual([["content", "(warmly)"]]);
  });
});

// ── Site 2: inline-markup.ts content-region widths ──────────────────

describe("contentRegions — dialect-derived geometry (#395)", () => {
  it("default preset (no geometry arg) stays byte-identical to the at-cue constants", () => {
    expect(contentRegions("@Bob:<>", "character").map((r) => "@Bob:<>".slice(r.from, r.to))).toEqual([
      "Bob",
    ]);
    expect(
      contentRegions("(whispering)<>", "parenthetical").map((r) =>
        "(whispering)<>".slice(r.from, r.to),
      ),
    ).toEqual(["(whispering)"]);
  });

  it("a custom dialect's differently-sized affixes scope correctly via geometry.contentSpan", () => {
    // A `<<name>>` shape has different affix widths (2/2) than the at-cue
    // `@`/`:<>`  (1/3) — passing the cached dialect geometry (as
    // `element-type.ts`'s LineInfo.dialect would) must use those widths
    // instead of the hardcoded AT_CUE_CHAR_SUFFIX_LEN/GLUE_LEN constants.
    const text = "<<radio>>";
    const geometry = {
      kind: "channel",
      attrs: [],
      hiddenSpans: [
        [0, 2],
        [7, 9],
      ] as const,
      contentSpan: [2, 7] as const,
    };
    // Reuse the Character branch's contentSpan-driven path: a custom kind
    // classified as "character" (via a dialect override) with non-at-cue
    // geometry must scope to the geometry's contentSpan, not the fixed
    // 1-char-prefix/3-char-suffix at-cue shape.
    const regions = contentRegions(text, "character", geometry);
    expect(regions.map((r) => text.slice(r.from, r.to))).toEqual(["radio"]);
  });

  it("a parenthetical with a wider glue (custom dialect override) scopes via contentSpan, not the fixed 2-char glue", () => {
    // A dialect that overrides the built-in `parenthetical` kind with a
    // 4-char glue (`<<>>` instead of at-cue's `<>`) — AT_CUE_GLUE_LEN (2)
    // would wrongly leave "e>" attached to the content; the geometry's
    // contentSpan gives the correct 0..9 bound.
    const text = "(aside)<<>>";
    const geometry = {
      kind: "parenthetical",
      attrs: [],
      hiddenSpans: [[7, 11]] as const,
      contentSpan: [0, 7] as const,
    };
    const regions = contentRegions(text, "parenthetical", geometry);
    expect(regions.map((r) => text.slice(r.from, r.to))).toEqual(["(aside)"]);
  });

  it("end-to-end: a live custom dialect overriding the character affix widths scopes markup content via cached LineInfo.dialect geometry", () => {
    // Override the built-in `character` kind's shape with wider affixes
    // (`<<Name>>` instead of at-cue's `@Name:<>`) — still the SAME declared
    // kind name (`character`), so `NARRATIVE_TYPES`/element-type classify it
    // as `ElementType.Character` as usual; only the affix widths differ.
    const CUSTOM_DIALECT: DialogueDialect = extendDialect(AT_CUE_DIALECT, {
      elements: [
        {
          kind: "character",
          nature: "narrative",
          source: {
            pattern: "^(?<lead><<)(?<speaker>[^>]*)(?<tail>>>)$",
            content_group: "speaker",
            hidden: ["lead", "tail"],
            template: "<<${speaker}>>",
          },
        },
      ],
    });
    const view = mount("<<Alice>>\n", { dialect: CUSTOM_DIALECT });
    const info = view.state.field(elementTypeField)[0];
    expect(info.type).toBe("character");
    const line = view.state.doc.line(1);
    // AT_CUE_CHAR_SUFFIX_LEN (3) / the 1-char '@' prefix would misalign
    // against this dialect's 2-char '<<'/'>>' affixes — the fix uses
    // info.dialect.contentSpan instead.
    const regions = contentRegions(line.text, info.type, info.dialect);
    expect(regions.map((r) => line.text.slice(r.from, r.to))).toEqual(["Alice"]);
    view.destroy();
  });
});
