/**
 * Dialect editor integration (#368 deliverable 2): the TS regex-fallback
 * classification path (no wasm document handle — same setup as
 * headless-theme.test.ts) driven by the dialect option end-to-end —
 * `brinkStudio({ dialect })`, `dialect: null` teardown, `setDialect(view,
 * d)` live reconfigure, and a custom dialect (`extendDialect`) classifying
 * and decorating a new kind. The default-preset byte-identical behavior is
 * covered by dialect-conformance.test.ts (the shared corpus) and the
 * pre-existing screenplay/element-type suites; this file exercises the
 * *editor wiring* around the dialect option specifically.
 */

import { afterEach, describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView, runScopeHandlers } from "@codemirror/view";
import {
  brinkStudio,
  setDialect,
  elementTypeField,
  ElementType,
  extendDialect,
  AT_CUE_DIALECT,
  type DialogueDialect,
  type DocHandle,
} from "@brink-lang/editor";

const minimal = {
  compile: () => ({ ok: true, diagnostics: [] }) as never,
  getSemanticTokens: () => [],
  getTokenTypeNames: () => [],
};

// The helper opts into the at-cue preset unless the caller says otherwise:
// since the 2026-08-30 "no dialect by default" ruling, an ABSENT option is
// none (plain lines) — the pin below covers that — while every other test
// here exercises the wiring WITH a dialect, as it always did.
function mount(doc: string, extra: Partial<Parameters<typeof brinkStudio>[0]> = {}): EditorView {
  const withPreset = "dialect" in extra ? extra : { dialect: AT_CUE_DIALECT, ...extra };
  return new EditorView({
    state: EditorState.create({ doc, extensions: [brinkStudio({ ...minimal, ...withPreset })] }),
    parent: document.body,
  });
}

describe("dialect option — absent means NONE (RULED 2026-08-30)", () => {
  it("no dialect option: a cue line stays narrative and no screenplay class renders", () => {
    const view = new EditorView({
      state: EditorState.create({ doc: "@Alice:<>\nHello there.\n", extensions: [brinkStudio(minimal)] }),
      parent: document.body,
    });
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).not.toBe(ElementType.Character);
    expect(infos[1].type).not.toBe(ElementType.Dialogue);
    const lines = [...view.dom.querySelectorAll(".cm-line")];
    expect(lines[0].className).not.toContain("brink-character");
    view.destroy();
  });
});

afterEach(() => {
  document.body.replaceChildren();
});

describe("dialect option — the at-cue preset", () => {
  it("classifies a character cue and chains following narrative to dialogue", () => {
    const view = mount("@Alice:<>\nHello there.\n");
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).toBe(ElementType.Character);
    expect(infos[1].type).toBe(ElementType.Dialogue);
    view.destroy();
  });

  it("renders the brink-character / brink-dialogue classes", () => {
    const view = mount("@Alice:<>\nHello there.\n");
    const lines = [...view.dom.querySelectorAll(".cm-line")];
    expect(lines[0].className).toContain("brink-character");
    expect(lines[1].className).toContain("brink-dialogue");
    view.destroy();
  });
});

describe("dialect: null — headless teardown", () => {
  it("never classifies a cue line as Character", () => {
    const view = mount("@Alice:<>\n", { dialect: null });
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).not.toBe(ElementType.Character);
    view.destroy();
  });

  it("emits no brink-character class and no hidden-sigil decorations", () => {
    const view = mount("@Alice:<>\n", { dialect: null });
    const line = view.dom.querySelector(".cm-line");
    expect(line?.className ?? "").not.toContain("brink-character");
    expect(view.dom.querySelectorAll(".brink-hidden-sigil")).toHaveLength(0);
    view.destroy();
  });

  it("re-enables classification via setDialect(view, AT_CUE_DIALECT)", () => {
    const view = mount("@Alice:<>\n", { dialect: null });
    expect(view.state.field(elementTypeField)[0].type).not.toBe(ElementType.Character);

    setDialect(view, AT_CUE_DIALECT);
    // setDialect dispatches a reclassify effect without a doc change.
    expect(view.state.field(elementTypeField)[0].type).toBe(ElementType.Character);
    view.destroy();
  });

  // Regression (#368 review): the STRUCTURAL weave keymap must survive
  // `dialect: null` — structural transition rows are interpreter-owned per
  // the dialect spec; only the dialect-specific layer is torn down. An
  // earlier draft gated the whole `brinkKeymap()` inside the screenplay
  // compartment, killing Choice/Gather/Narrative Tab/Enter handling in
  // headless mode.
  it("dialect: null keeps the structural keymap: Enter on a choice inserts a new sibling", () => {
    const view = mount("* Option A", { dialect: null });
    view.dispatch({ selection: { anchor: view.state.doc.line(1).to } });
    const handled = runScopeHandlers(
      view,
      new KeyboardEvent("keydown", { key: "Enter" }),
      "editor",
    );
    expect(handled).toBe(true);
    expect(view.state.doc.toString()).toBe("* Option A\n* ");
    view.destroy();
  });

  it("dialect: null — Tab on a choice INDENTS (built-in conversion cycle stripped, ruled 2026-08-24)", () => {
    // Tab used to run the convertToIndentedNarrative transition. The
    // built-in conversion rows are stripped; Tab now indents the line by
    // the indent unit, and the document handle's convertElement is NOT
    // called.
    const doc = "* Option A";
    const convertCalls: Array<{ offset: number; target: string }> = [];
    const fakeHandle = {
      pushSource: () => {},
      lineContexts: () => [],
      setDialect: () => {},
      clearDialect: () => {},
      convertElement: (offset: number, target: string) => {
        convertCalls.push({ offset, target });
        return null;
      },
    };
    const view = mount(doc, {
      getDocumentHandle: () => fakeHandle,
    } as never);
    view.dispatch({ selection: { anchor: 0 } });
    runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Tab" }), "editor");
    expect(convertCalls).toEqual([]);
    expect(view.state.doc.line(1).text).toMatch(/^\s+\* Option A$/);
  })

  it("dialect: null disables the blank-tab template insert (a dialect behavior, not a structural row)", () => {
    const view = mount("\n\n", { dialect: null });
    view.dispatch({ selection: { anchor: view.state.doc.line(2).from } });
    runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Tab" }), "editor");
    expect(view.state.doc.toString()).not.toContain("@:<>");
    view.destroy();
  });
});

describe("custom dialect via extendDialect", () => {
  // A minimal custom kind: `<<channel>>` cues (double-angle-bracket affix,
  // no glue) — doesn't collide with any built-in ink sigil (a leading `<`
  // only means something as `<-` thread syntax) — added on top of the
  // at-cue preset without forking it.
  const CHANNEL_DIALECT: DialogueDialect = extendDialect(AT_CUE_DIALECT, {
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
  });

  it("classifies the custom kind and derives its CSS class as brink-<kind>", () => {
    const view = mount("<<radio>>\n", { dialect: CHANNEL_DIALECT });
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).toBe("channel");
    const line = view.dom.querySelector(".cm-line");
    expect(line?.className).toContain("brink-channel");
    view.destroy();
  });

  it("hides the custom kind's affix geometry (<< >> sigils) as zero-width decorations", () => {
    const view = mount("<<radio>>\n", { dialect: CHANNEL_DIALECT });
    expect(view.dom.querySelectorAll(".brink-hidden-sigil")).toHaveLength(2);
    view.destroy();
  });

  it("still classifies the at-cue preset's own kinds (extendDialect adds, does not replace)", () => {
    const view = mount("@Alice:<>\n", { dialect: CHANNEL_DIALECT });
    expect(view.state.field(elementTypeField)[0].type).toBe(ElementType.Character);
    view.destroy();
  });

  it("setDialect swaps to the custom dialect live and reclassifies", () => {
    const view = mount("<<radio>>\n");
    expect(view.state.field(elementTypeField)[0].type).not.toBe("channel");
    setDialect(view, CHANNEL_DIALECT);
    expect(view.state.field(elementTypeField)[0].type).toBe("channel");
    view.destroy();
  });
});

describe("wasm-handle path: byte → UTF-16 span conversion (astral characters)", () => {
  // The Rust classifier reports dialect geometry in UTF-8 BYTE offsets;
  // `element-type.ts`'s `toGeometry`/`makeByteToUtf16` convert them to the
  // UTF-16 offsets CodeMirror uses. An astral-plane speaker name (😀 =
  // U+1F600: 4 UTF-8 bytes but 2 UTF-16 code units) exercises the
  // code-point-iteration fix — an implementation that walked UTF-16 code
  // units would split the surrogate pair and corrupt the span table. The
  // byte spans below are exactly what Rust's `ResolvedDialect::classify`
  // emits for "@😀:<>": lead (0,1), speaker (1,5), tail (5,8). This suite
  // runs against the wasm MOCK (vitest aliases brink-web), so the wasm-
  // handle branch is fed via a fake handle returning the Rust-shaped JSON;
  // the REAL wasm end-to-end path is covered by the Playwright suite
  // (e2e/character.spec.ts's astral case).
  it("converts Rust byte spans to correct UTF-16 spans for an emoji speaker", () => {
    const doc = "@😀:<>"; // 8 UTF-8 bytes, 6 UTF-16 code units
    const fakeHandle = {
      pushSource: () => {},
      setDialect: () => {},
      clearDialect: () => {},
      lineContexts: () => [
        {
          element: "narrative",
          weave: { depth: 0, element: "top_level" },
          has_tags: false,
          block_comment: false,
          dialect: {
            kind: "character",
            attrs: [["speaker", "😀"]],
            hidden_spans: [
              [0, 1], // '@' — 1 byte
              [5, 8], // ':<>' — after the 4-byte emoji
            ],
            content_span: [1, 5], // the emoji, 4 bytes
          },
        },
      ],
    } as unknown as DocHandle;

    const view = mount(doc, { handleSlot: { handle: fakeHandle } });
    const info = view.state.field(elementTypeField)[0];
    expect(info.type).toBe(ElementType.Character);
    // UTF-16: '@' (0,1); ':<>' starts AFTER the 2-code-unit emoji → (3,6);
    // content is the emoji itself → (1,3). If the byte→UTF-16 table were
    // corrupted (lone-surrogate encodes) or the lookup fell through to its
    // `?? text.length` fallback, the interior offsets 1/3/5 would come back
    // as 6 instead.
    expect(info.dialect?.hiddenSpans).toEqual([
      [0, 1],
      [3, 6],
    ]);
    expect(info.dialect?.contentSpan).toEqual([1, 3]);

    // And the rendered hidden-sigil decorations land on those spans: both
    // sigils concealed, only the emoji visibly remains (plus the widgets'
    // zero-width anchors).
    expect(view.dom.querySelectorAll(".brink-hidden-sigil")).toHaveLength(2);
    const lineEl = view.dom.querySelector(".cm-line");
    expect(lineEl?.textContent?.replace(/​/g, "")).toBe("😀");
    view.destroy();
  });
});

describe("per-view dialect isolation (regression: no shared module-global state)", () => {
  // A minimal custom kind, disjoint from the at-cue preset — if the active
  // dialect were tracked as module-level state (rather than a CM6 Facet
  // scoped to each EditorState), mounting/reconfiguring one view here would
  // silently reclassify every other live view too.
  const CHANNEL_DIALECT: DialogueDialect = extendDialect(AT_CUE_DIALECT, {
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
  });

  it("two views mounted with different dialects classify independently", () => {
    const a = mount("<<radio>>\n", { dialect: CHANNEL_DIALECT });
    const b = mount("@Alice:<>\n"); // default preset, mounted AFTER `a`

    expect(a.state.field(elementTypeField)[0].type).toBe("channel");
    expect(b.state.field(elementTypeField)[0].type).toBe(ElementType.Character);
    // `b`'s narrower default dialect must not see the custom kind, and `a`'s
    // custom dialect must not have been clobbered by mounting `b` after it.
    expect(a.state.field(elementTypeField)[0].type).toBe("channel");

    a.destroy();
    b.destroy();
  });

  it("dialect: null on one view does not disable classification on a sibling view", () => {
    const headless = mount("@Alice:<>\n", { dialect: null });
    const normal = mount("@Bob:<>\n");

    expect(headless.state.field(elementTypeField)[0].type).not.toBe(ElementType.Character);
    expect(normal.state.field(elementTypeField)[0].type).toBe(ElementType.Character);

    headless.destroy();
    normal.destroy();
  });

  it("setDialect(view, d) on one view does not reclassify a sibling view", () => {
    const a = mount("<<radio>>\n");
    const b = mount("<<radio>>\n");
    expect(a.state.field(elementTypeField)[0].type).not.toBe("channel");
    expect(b.state.field(elementTypeField)[0].type).not.toBe("channel");

    setDialect(a, CHANNEL_DIALECT);
    expect(a.state.field(elementTypeField)[0].type).toBe("channel");
    // `b` was never reconfigured — it must still be on the default preset.
    expect(b.state.field(elementTypeField)[0].type).not.toBe("channel");

    a.destroy();
    b.destroy();
  });
});

// ── #413 regression tests (regex-fallback path) ──────────────────────
// Two classification gaps broke screenplay mode (celeris repro, reproduced
// against published 0.8.0): a `~`-sigil line after dialogue got swallowed
// into the cue→dialogue chain, and lines in/around conditional blocks got
// NO classes at all. These pin the TS regex-fallback mirror
// (`applyConditionalScaffoldFallback` in `element-type.ts`) of the Rust fix
// in `line_context.rs`.
describe("dialect option — #413 conditional scaffold + sigil-wins-chain (fallback)", () => {
  it("a '~' sigil line after chained dialogue classifies as logic, not dialogue", () => {
    const view = mount(
      "@Solstice:<>\nAwwww... I have to get going now, Minnie. Sorry!\n~ change_party_member(2, false)\n-> END\n",
    );
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).toBe(ElementType.Character);
    expect(infos[1].type).toBe(ElementType.Dialogue);
    expect(infos[2].type).toBe(ElementType.Logic);
    expect(infos[2].dialect).toBeUndefined();
    view.destroy();
  });

  it("renders brink-logic (not brink-dialogue) on the sigil line's .cm-line", () => {
    const view = mount(
      "@Solstice:<>\nAwwww... I have to get going now, Minnie. Sorry!\n~ change_party_member(2, false)\n-> END\n",
    );
    const lines = [...view.dom.querySelectorAll(".cm-line")];
    const sigilLine = lines.find((l) => (l.textContent ?? "").includes("change_party_member"));
    expect(sigilLine?.className).toContain("brink-logic");
    expect(sigilLine?.className).not.toContain("brink-dialogue");
    view.destroy();
  });

  it("conditional routing-block braces and if/else headers classify as logic", () => {
    const view = mount("{\n    - get_variable(16) == 2: -> leave\n    - else: -> busy\n}\n");
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).toBe(ElementType.Logic); // {
    expect(infos[3].type).toBe(ElementType.Logic); // }
    view.destroy();
  });

  it("cue/dialogue lines inside a conditional arm classify and chain", () => {
    const view = mount(
      "{ get_variable(17) >= 1:\n    @Solstice:<>\n    Hello, this is Sols.\n- else:\n    @Solstice:<>\n    Hello?\n}\n-> END\n",
    );
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).toBe(ElementType.Logic); // { get_variable(17) >= 1:
    expect(infos[1].type).toBe(ElementType.Character); //     @Solstice:<>
    expect(infos[2].type).toBe(ElementType.Dialogue); //     Hello, this is Sols.
    expect(infos[3].type).toBe(ElementType.Logic); // - else:
    expect(infos[4].type).toBe(ElementType.Character); //     @Solstice:<>
    expect(infos[5].type).toBe(ElementType.Dialogue); //     Hello?
    expect(infos[6].type).toBe(ElementType.Logic); // }
    view.destroy();
  });

  it("renders brink-character/brink-dialogue classes for cues inside a conditional arm", () => {
    const view = mount("{ get_variable(17) >= 1:\n    @Solstice:<>\n    Hello there.\n}\n");
    const lines = [...view.dom.querySelectorAll(".cm-line")];
    const cue = lines.find((l) => (l.textContent ?? "").includes("Solstice"));
    const dialogue = lines.find((l) => (l.textContent ?? "").includes("Hello there"));
    expect(cue?.className).toContain("brink-character");
    expect(dialogue?.className).toContain("brink-dialogue");
    view.destroy();
  });

  // Regression guard (conditional-scaffold pass follow-up): ordinary
  // narrative containing inline logic that starts/ends with a brace must
  // NOT be swept into conditional-scaffold `Logic` classification. Only a
  // block's own recorded opening/closing brace is scaffold.
  it("a standalone inline conditional used as narrative keeps NarrativeText, not Logic", () => {
    const view = mount("{visited: You were here before.}\nNext.\n");
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).toBe(ElementType.NarrativeText);
    expect(infos[1].type).toBe(ElementType.NarrativeText);
    view.destroy();
  });

  it("narrative ending in a value interpolation keeps NarrativeText, not Logic", () => {
    const view = mount("You have {gold}\nMore text.\n");
    const infos = view.state.field(elementTypeField);
    expect(infos[0].type).toBe(ElementType.NarrativeText);
    view.destroy();
  });

  it("renders brink-narrative (not brink-logic) for narrative lines with inline logic", () => {
    const view = mount("{visited: You were here before.}\nYou have {gold}\n");
    const lines = [...view.dom.querySelectorAll(".cm-line")];
    const inlineCondLine = lines.find((l) => (l.textContent ?? "").includes("were here before"));
    const interpLine = lines.find((l) => (l.textContent ?? "").includes("You have"));
    expect(inlineCondLine?.className).toContain("brink-narrative");
    expect(inlineCondLine?.className).not.toContain("brink-logic");
    expect(interpLine?.className).toContain("brink-narrative");
    expect(interpLine?.className).not.toContain("brink-logic");
    view.destroy();
  });
});
