/**
 * Extensible inline-markup rules (#367).
 *
 * Exercises both layers of the feature:
 *
 *  - `contentRegions` — the pure content-region-scoping core: rules run only
 *    inside the narrative content text of classified lines, never over ink
 *    syntax (glue `<>`, threads `<-`, divert arrows, choice brackets, choice/
 *    gather sigil prefixes, hidden screenplay sigils).
 *  - `inlineMarkup(rules)` — the CM6 wiring: a real `EditorView` (jsdom) with
 *    host rules, asserting `brink-markup-<name>` marks with `data-*`
 *    attributes from named capture groups, the pair form's content class, and
 *    that zero rules ship by default (the extension is inert without host
 *    registration).
 */

import { describe, it, expect, afterEach } from "vitest";
import { EditorState, type Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import {
  inlineMarkup,
  contentRegions,
  rmmzAngleTagRule,
  ElementType,
  type InlineMarkupRule,
} from "@brink-lang/editor";

// ── Pure scoping core ──────────────────────────────────────────────

function regionTexts(text: string, type: ElementType): string[] {
  return contentRegions(text, type).map((r) => text.slice(r.from, r.to));
}

describe("contentRegions (content-region scoping)", () => {
  it("covers a plain narrative line", () => {
    expect(regionTexts("Hello there.", ElementType.NarrativeText)).toEqual(["Hello there."]);
  });

  it("returns nothing for non-narrative line types", () => {
    for (const [text, type] of [
      ["== knot ==", ElementType.KnotHeader],
      ["= stitch", ElementType.StitchHeader],
      ["-> target", ElementType.Divert],
      ["~ x = 1", ElementType.Logic],
      ["VAR x = 1", ElementType.VarDecl],
      ["// note", ElementType.Comment],
      ["INCLUDE a.ink", ElementType.Include],
      ["EXTERNAL fn(x)", ElementType.External],
      ["# author: b", ElementType.Tag],
      ["", ElementType.Blank],
    ] as const) {
      expect(contentRegions(text, type)).toEqual([]);
    }
  });

  it("skips thread lines entirely", () => {
    expect(contentRegions("<- some_knot", ElementType.NarrativeText)).toEqual([]);
    expect(contentRegions("  <- some_knot", ElementType.ChoiceBody)).toEqual([]);
  });

  it("splits at glue <> without including the token", () => {
    expect(regionTexts("<> glued start", ElementType.NarrativeText)).toEqual([" glued start"]);
    expect(regionTexts("glued end <>", ElementType.NarrativeText)).toEqual(["glued end "]);
    expect(regionTexts("a<>b", ElementType.NarrativeText)).toEqual(["a", "b"]);
  });

  it("splits at a mid-line thread token", () => {
    expect(regionTexts("text <- more", ElementType.NarrativeText)).toEqual(["text ", " more"]);
  });

  it("truncates at a divert arrow", () => {
    expect(regionTexts("He left. -> next", ElementType.NarrativeText)).toEqual(["He left. "]);
    expect(regionTexts("in -> tunnel ->", ElementType.NarrativeText)).toEqual(["in "]);
  });

  it("excludes the choice sigil prefix and splits at brackets", () => {
    expect(regionTexts("* pick [go] now", ElementType.Choice)).toEqual(["pick ", "go", " now"]);
    expect(regionTexts("  * * deep choice", ElementType.Choice)).toEqual(["deep choice"]);
    expect(regionTexts("+ sticky", ElementType.Choice)).toEqual(["sticky"]);
  });

  it("does not split at brackets outside choice lines", () => {
    expect(regionTexts("a [b] c", ElementType.NarrativeText)).toEqual(["a [b] c"]);
  });

  it("excludes the gather sigil prefix", () => {
    expect(regionTexts("- - gathered text", ElementType.Gather)).toEqual(["gathered text"]);
  });

  it("scopes a character line to the name between hidden sigils", () => {
    expect(regionTexts("@Bob:<>", ElementType.Character)).toEqual(["Bob"]);
    expect(contentRegions("@:<>", ElementType.Character)).toEqual([]);
  });

  it("excludes the hidden trailing glue of a parenthetical", () => {
    expect(regionTexts("(whispering)<>", ElementType.Parenthetical)).toEqual(["(whispering)"]);
  });
});

// ── CM6 wiring ─────────────────────────────────────────────────────

let view: EditorView | undefined;

function mount(doc: string, extensions: Extension): EditorView {
  view = new EditorView({
    state: EditorState.create({ doc, extensions }),
    parent: document.body,
  });
  return view;
}

afterEach(() => {
  view?.destroy();
  view = undefined;
  document.body.innerHTML = "";
});

function marks(v: EditorView, selector: string): HTMLElement[] {
  return Array.from(v.dom.querySelectorAll<HTMLElement>(selector));
}

describe("inlineMarkup extension", () => {
  it("ships zero rules by default: inlineMarkup([]) decorates nothing", () => {
    const v = mount("Hello <wave>everyone</wave>!", inlineMarkup([]));
    expect(marks(v, "[class*='brink-markup-']")).toEqual([]);
  });

  it("decorates rmmz preset matches with class + data-* from named groups", () => {
    const v = mount(
      "Hello <wave>everyone</wave>, pick <color=3>this</color>.",
      inlineMarkup([rmmzAngleTagRule]),
    );
    const tags = marks(v, ".brink-markup-rmmz-tag");
    expect(tags.map((el) => el.textContent)).toEqual([
      "<wave>",
      "</wave>",
      "<color=3>",
      "</color>",
    ]);
    expect(tags[0].dataset.tag).toBe("wave");
    expect(tags[0].dataset.value).toBeUndefined();
    expect(tags[2].dataset.tag).toBe("color");
    expect(tags[2].dataset.value).toBe("3");
  });

  it("never matches ink syntax: glue, threads, diverts, logic stay clean", () => {
    const doc = [
      "glue stays <> untouched",
      "<- thread_target",
      "-> divert_target",
      "~ temp x = 1 // <wave> in logic is not content",
      "narrative <wave>match</wave> here -> after_divert <wave>no</wave>",
    ].join("\n");
    const v = mount(doc, inlineMarkup([rmmzAngleTagRule]));
    const tags = marks(v, ".brink-markup-rmmz-tag");
    // Only the two tags before the divert arrow on the narrative line match.
    expect(tags.map((el) => el.textContent)).toEqual(["<wave>", "</wave>"]);
  });

  it("matches inside choice text but never over sigils or brackets", () => {
    const spanRule: InlineMarkupRule = { name: "span", pattern: /pick \[go/g };
    const goRule: InlineMarkupRule = { name: "go", pattern: /go/g };
    const sigilRule: InlineMarkupRule = { name: "sigil", pattern: /[*+]/g };
    const v = mount("* pick [go] now", inlineMarkup([spanRule, goRule, sigilRule]));

    // A match can never span over a bracket…
    expect(marks(v, ".brink-markup-span")).toEqual([]);
    // …but bracket-inner text is still content.
    expect(marks(v, ".brink-markup-go").map((el) => el.textContent)).toEqual(["go"]);
    // The choice sigil prefix is never matchable.
    expect(marks(v, ".brink-markup-sigil")).toEqual([]);
  });

  it("pair form classes open, close, and the wrapped content", () => {
    const pair: InlineMarkupRule = {
      name: "em",
      open: /<i>/g,
      close: /<\/i>/g,
      contentClass: "host-italic",
    };
    const v = mount("He said <i>hello</i> there.", inlineMarkup([pair]));

    expect(marks(v, ".brink-markup-em").map((el) => el.textContent)).toEqual(["<i>", "</i>"]);
    expect(marks(v, ".host-italic").map((el) => el.textContent)).toEqual(["hello"]);
  });

  it("pair form defaults the content class and tolerates an unpaired open", () => {
    const pair: InlineMarkupRule = { name: "em", open: /<i>/g, close: /<\/i>/g };
    const v = mount("both <i>styled</i> and <i>dangling", inlineMarkup([pair]));

    expect(marks(v, ".brink-markup-em-content").map((el) => el.textContent)).toEqual(["styled"]);
    // The dangling open still renders as an inert classed literal.
    expect(marks(v, ".brink-markup-em").map((el) => el.textContent)).toEqual([
      "<i>",
      "</i>",
      "<i>",
    ]);
  });

  it("uses classes and data attributes only — no inline styles (#363)", () => {
    const v = mount("Hello <wave>everyone</wave>!", inlineMarkup([rmmzAngleTagRule]));
    for (const el of marks(v, ".brink-markup-rmmz-tag")) {
      expect(el.getAttribute("style")).toBeNull();
    }
  });

  it("rebuilds on doc changes", () => {
    const v = mount("plain line", inlineMarkup([rmmzAngleTagRule]));
    expect(marks(v, ".brink-markup-rmmz-tag")).toEqual([]);
    v.dispatch({ changes: { from: 0, insert: "<shake>" } });
    expect(marks(v, ".brink-markup-rmmz-tag").map((el) => el.textContent)).toEqual(["<shake>"]);
  });
});
