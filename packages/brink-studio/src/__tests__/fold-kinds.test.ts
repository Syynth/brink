/**
 * Fold kinds (#365): `FoldRange.kind` (structural/machinery/narrative), the
 * live-reconfigurable active-kinds set, `foldAllOfKind`/`unfoldAllOfKind`
 * bulk commands, and the JetBrains-style summary pills (machinery/narrative/
 * decl) as class-addressable fold placeholders with zero inline styles.
 */

import { afterEach, describe, expect, it } from "vitest";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { foldable, foldEffect, foldedRanges } from "@codemirror/language";
import type { FoldRange } from "@brink/wasm-types";
import {
  foldingExtension,
  foldAllOfKind,
  unfoldAllOfKind,
  setActiveFoldKinds,
  activeFoldKindsFacet,
  brinkStudio,
  elementTypeField,
} from "@brink-lang/editor";

let view: EditorView | undefined;

afterEach(() => {
  view?.destroy();
  view = undefined;
  document.body.innerHTML = "";
});

function mountFolding(doc: string, getFoldingRanges: (source: string) => FoldRange[]): EditorView {
  view = new EditorView({
    state: EditorState.create({ doc, extensions: [foldingExtension({ getFoldingRanges })] }),
    parent: document.body,
  });
  return view;
}

function foldLine(v: EditorView, line0: number): { from: number; to: number } | null {
  const line = v.state.doc.line(line0 + 1);
  const region = foldable(v.state, line.from, line.to);
  if (!region) return null;
  v.dispatch({ effects: foldEffect.of(region) });
  return region;
}

describe("FoldKind — active-kinds gating", () => {
  const MACHINERY_SRC = "~ temp x = 1\n~ temp y = 2\nHello there.\n";
  const machineryRanges: FoldRange[] = [{ start_line: 0, end_line: 1, kind: "machinery" }];

  it("defaults to all three kinds active — a machinery fold is foldable", () => {
    const v = mountFolding(MACHINERY_SRC, () => machineryRanges);
    expect(foldLine(v, 0)).not.toBeNull();
  });

  it("removing 'machinery' from the active set makes the fold un-foldable", () => {
    const v = mountFolding(MACHINERY_SRC, () => machineryRanges);
    setActiveFoldKinds(v, new Set(["structural", "narrative"]));
    expect(foldLine(v, 0)).toBeNull();
  });

  it("re-adding 'machinery' makes it foldable again (live reconfigure)", () => {
    const v = mountFolding(MACHINERY_SRC, () => machineryRanges);
    setActiveFoldKinds(v, new Set(["structural"]));
    expect(foldLine(v, 0)).toBeNull();
    setActiveFoldKinds(v, new Set(["structural", "machinery", "narrative"]));
    expect(foldLine(v, 0)).not.toBeNull();
  });

  it("activeFoldKindsFacet reads back the currently active set", () => {
    const v = mountFolding(MACHINERY_SRC, () => machineryRanges);
    setActiveFoldKinds(v, new Set(["narrative"]));
    expect(v.state.facet(activeFoldKindsFacet)).toEqual(new Set(["narrative"]));
  });
});

describe("foldAllOfKind / unfoldAllOfKind (#365)", () => {
  const SRC = "~ temp x = 1\n~ temp y = 2\nHello there.\nHow are you?\n// a comment\n";
  const ranges: FoldRange[] = [
    { start_line: 0, end_line: 1, kind: "machinery" },
    { start_line: 2, end_line: 3, kind: "narrative" },
  ];

  it("foldAllOfKind('machinery') folds only the machinery run", () => {
    const v = mountFolding(SRC, () => ranges);
    const applied = foldAllOfKind("machinery")(v);
    expect(applied).toBe(true);
    expect(foldedRanges(v.state).size).toBe(1);
    // The narrative run's anchor line is still unfolded.
    expect(foldable(v.state, v.state.doc.line(3).from, v.state.doc.line(3).to)).not.toBeNull();
  });

  it("foldAllOfKind('narrative') folds only the narrative run", () => {
    const v = mountFolding(SRC, () => ranges);
    foldAllOfKind("narrative")(v);
    expect(foldedRanges(v.state).size).toBe(1);
  });

  it("is a no-op (returns false) when there's nothing of that kind", () => {
    const v = mountFolding(SRC, () => ranges);
    expect(foldAllOfKind("structural")(v)).toBe(false);
  });

  it("unfoldAllOfKind reverses a bulk fold", () => {
    const v = mountFolding(SRC, () => ranges);
    foldAllOfKind("machinery")(v);
    expect(foldedRanges(v.state).size).toBe(1);
    const applied = unfoldAllOfKind("machinery")(v);
    expect(applied).toBe(true);
    expect(foldedRanges(v.state).size).toBe(0);
  });

  it("foldAllOfKind does not force-fold kinds outside the active set", () => {
    const v = mountFolding(SRC, () => ranges);
    setActiveFoldKinds(v, new Set(["structural"]));
    // Machinery is deactivated — bulk-fold must not bypass that gate.
    expect(foldAllOfKind("machinery")(v)).toBe(false);
  });
});

describe("machinery/narrative summary pills — class-addressable, zero inline styles", () => {
  const SRC = "~ change_party_member(2, false)\n~ leave = true\nHello there, friend.\nHow are you?\n";
  const ranges: FoldRange[] = [
    { start_line: 0, end_line: 1, kind: "machinery" },
    { start_line: 2, end_line: 3, kind: "narrative" },
  ];

  it("machinery pill: brink-fold-pill + kind class, no inline style attribute", () => {
    const v = mountFolding(SRC, () => ranges);
    foldLine(v, 0);
    const pill = v.dom.querySelector<HTMLElement>(".brink-fold-pill-machinery");
    expect(pill).not.toBeNull();
    expect(pill?.classList.contains("brink-fold-pill")).toBe(true);
    expect(pill?.getAttribute("style")).toBeNull();
    expect(pill?.querySelector(".brink-fold-pill-icon")).not.toBeNull();
    expect(pill?.querySelector(".brink-fold-pill-summary")).not.toBeNull();
    expect(pill?.querySelector(".brink-fold-pill-count")).not.toBeNull();
  });

  it("machinery pill summary surfaces the call target, not just a count", () => {
    const v = mountFolding(SRC, () => ranges);
    foldLine(v, 0);
    const summary = v.dom.querySelector(".brink-fold-pill-summary")?.textContent ?? "";
    expect(summary).toContain("change_party_member");
  });

  it("narrative pill: brink-fold-pill + kind class, no inline style attribute", () => {
    const v = mountFolding(SRC, () => ranges);
    foldLine(v, 2);
    const pill = v.dom.querySelector<HTMLElement>(".brink-fold-pill-narrative");
    expect(pill).not.toBeNull();
    expect(pill?.classList.contains("brink-fold-pill")).toBe(true);
    expect(pill?.getAttribute("style")).toBeNull();
  });

  it("narrative pill shows the first-line snippet and line count", () => {
    const v = mountFolding(SRC, () => ranges);
    foldLine(v, 2);
    const summary = v.dom.querySelector(".brink-fold-pill-summary")?.textContent ?? "";
    expect(summary).toContain("Hello there, friend.");
    const count = v.dom.querySelector(".brink-fold-pill-count")?.textContent ?? "";
    expect(count).toContain("2");
  });

  it("pills are clickable (aria-label present, onclick wired)", () => {
    const v = mountFolding(SRC, () => ranges);
    foldLine(v, 0);
    const pill = v.dom.querySelector<HTMLElement>(".brink-fold-pill-machinery");
    expect(pill?.getAttribute("aria-label")).toBeTruthy();
    expect(typeof pill?.onclick).toBe("function");
  });
});

describe("narrative pill cast — via dialect speaker attr, not characterName()", () => {
  const minimal = {
    compile: () => ({ ok: true, diagnostics: [] }) as never,
    getSemanticTokens: () => [],
    getTokenTypeNames: () => [],
  };

  it("cast comes from the carried dialect speaker attr", () => {
    const doc = "@Alice:<>\nHello there.\nHow are you?\n";
    const ranges: FoldRange[] = [{ start_line: 1, end_line: 2, kind: "narrative" }];
    view = new EditorView({
      state: EditorState.create({
        doc,
        extensions: [brinkStudio({ ...minimal, getFoldingRanges: () => ranges })],
      }),
      parent: document.body,
    });
    // Sanity: the regex-fallback dialect classification carried the speaker.
    const infos = view.state.field(elementTypeField);
    expect(infos[1].dialect?.attrs.some(([k, val]) => k === "speaker" && val === "Alice")).toBe(
      true,
    );

    foldLine(view, 1);
    const summary = view.dom.querySelector(".brink-fold-pill-summary")?.textContent ?? "";
    expect(summary).toContain("Alice");
  });
});

describe("decl pill — data-decl-kind + icon slot (#365 deliverable)", () => {
  it("tags a knot fold with data-decl-kind='knot'", () => {
    const v = mountFolding("== hub ==\ntext\n", () => [
      { start_line: 0, end_line: 1, from_line_start: true, kind: "structural" },
    ]);
    foldLine(v, 0);
    const el = v.dom.querySelector<HTMLElement>(".brink-fold-decl");
    expect(el?.getAttribute("data-decl-kind")).toBe("knot");
    expect(el?.querySelector(".brink-fold-decl-icon")).not.toBeNull();
  });

  it("tags a stitch fold with data-decl-kind='stitch'", () => {
    const v = mountFolding("= stitch_name\ntext\n", () => [
      { start_line: 0, end_line: 1, from_line_start: true, kind: "structural" },
    ]);
    foldLine(v, 0);
    const el = v.dom.querySelector<HTMLElement>(".brink-fold-decl");
    expect(el?.getAttribute("data-decl-kind")).toBe("stitch");
  });

  it("tags a function knot fold with data-decl-kind='function'", () => {
    const v = mountFolding("== function damage(weapon) ==\n~ return 1\n", () => [
      { start_line: 0, end_line: 1, from_line_start: true, kind: "structural" },
    ]);
    foldLine(v, 0);
    const el = v.dom.querySelector<HTMLElement>(".brink-fold-decl");
    expect(el?.getAttribute("data-decl-kind")).toBe("function");
  });
});

describe("exact-span tie-break — structural vs machinery/narrative (#405)", () => {
  // A structural fold and a machinery fold that resolve to the IDENTICAL
  // `{from, to}` span is a rare edge case, but `preparePlaceholder` must
  // pick a kind deliberately, not by accident of which FoldRange the host's
  // getFoldingRanges() happened to push first. Status quo (and the pinned
  // precedence) is: structural wins.
  const SRC = "== hub ==\n~ temp x = 1\n";

  it("structural wins over machinery when both resolve to the same span", () => {
    const v = mountFolding(SRC, () => [
      // Machinery pushed FIRST here — the opposite of production push order
      // (`folding_ranges_impl` always pushes structural before machinery/
      // narrative) — to prove the tie-break is order-independent, not an
      // accident of iteration order. `from_line_start: true` on both is what
      // makes the resolved spans genuinely identical (resolveFold uses it to
      // pick `line.from` vs `line.to`) — this is the tie the issue describes.
      { start_line: 0, end_line: 1, from_line_start: true, kind: "machinery" },
      { start_line: 0, end_line: 1, from_line_start: true, kind: "structural" },
    ]);
    foldLine(v, 0);
    const decl = v.dom.querySelector(".brink-fold-decl");
    expect(decl).not.toBeNull();
    expect(decl?.getAttribute("data-decl-kind")).toBe("knot");
    expect(v.dom.querySelector(".brink-fold-pill-machinery")).toBeNull();
  });

  it("structural still wins when pushed first (matches production push order)", () => {
    const v = mountFolding(SRC, () => [
      { start_line: 0, end_line: 1, from_line_start: true, kind: "structural" },
      { start_line: 0, end_line: 1, from_line_start: true, kind: "machinery" },
    ]);
    foldLine(v, 0);
    const decl = v.dom.querySelector(".brink-fold-decl");
    expect(decl).not.toBeNull();
    expect(decl?.getAttribute("data-decl-kind")).toBe("knot");
    expect(v.dom.querySelector(".brink-fold-pill-machinery")).toBeNull();
  });
});
