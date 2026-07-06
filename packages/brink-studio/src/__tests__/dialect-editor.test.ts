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
import { EditorView } from "@codemirror/view";
import {
  brinkStudio,
  setDialect,
  elementTypeField,
  ElementType,
  extendDialect,
  AT_CUE_DIALECT,
  type DialogueDialect,
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
