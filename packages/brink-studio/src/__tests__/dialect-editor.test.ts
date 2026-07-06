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

function mount(doc: string, extra: Partial<Parameters<typeof brinkStudio>[0]> = {}): EditorView {
  return new EditorView({
    state: EditorState.create({ doc, extensions: [brinkStudio({ ...minimal, ...extra })] }),
    parent: document.body,
  });
}

afterEach(() => {
  document.body.replaceChildren();
});

describe("dialect option — default preset", () => {
  it("classifies a character cue and chains following narrative to dialogue (no dialect option = at-cue preset)", () => {
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

  it("dialect: null keeps the structural keymap: Tab on a choice still routes to convertElement", () => {
    // Tab on a Choice line = the `convertToIndentedNarrative` transition,
    // which converts via the document handle's `convertElement`. A minimal
    // fake handle proves the routing still happens under dialect: null.
    const doc = "* Option A";
    const convertCalls: Array<{ offset: number; target: string }> = [];
    const fakeHandle = {
      pushSource: () => {},
      lineContexts: () => [],
      setDialect: () => {},
      clearDialect: () => {},
      convertElement: (offset: number, target: string) => {
        convertCalls.push({ offset, target });
        return { from: 0, to: doc.length, insert: "  Option A" };
      },
    } as unknown as DocHandle;

    const view = mount(doc, { dialect: null, handleSlot: { handle: fakeHandle } });
    view.dispatch({ selection: { anchor: view.state.doc.line(1).to } });
    const handled = runScopeHandlers(
      view,
      new KeyboardEvent("keydown", { key: "Tab" }),
      "editor",
    );
    expect(handled).toBe(true);
    expect(convertCalls).toEqual([{ offset: doc.length, target: "choice_body" }]);
    expect(view.state.doc.toString()).toBe("  Option A");
    view.destroy();
  });

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
