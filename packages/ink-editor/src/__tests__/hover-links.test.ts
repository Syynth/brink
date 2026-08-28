/**
 * Navigable references in the hover card (#3255, decision 5).
 *
 * The card names things — the cells an `effects` row touches, the file a
 * symbol is defined in — and naming them without letting an author reach
 * them makes it a readout rather than a way to move.
 *
 * Targets travel as an INDEX into a side-channel (`HoverInfo.links`) rather
 * than as a path inside the markdown, because a path in a link target would
 * have to survive `)` and `:` inside it. That choice moves the risk here:
 * an index that resolves to the wrong entry navigates somewhere plausible
 * and wrong, which is worse than not navigating. Hence the alignment and
 * degradation cases below.
 */
import { describe, expect, it, vi } from "vitest";
import { renderHoverContent } from "../hover.js";
import type { HoverInfo, Location } from "@brink/wasm-types";

const loc = (file: string, start: number, end: number): Location => ({ file, start, end });

function render(info: Partial<HoverInfo>, onNavigate?: (t: Location) => void) {
  return renderHoverContent({ content: "", ...info } as HoverInfo, onNavigate);
}

describe("hover links", () => {
  it("renders a reference as a link and navigates to its target", () => {
    const nav = vi.fn();
    const dom = render(
      { content: "**effects** writes: [`torch`](#0)", links: [loc("story.ink", 12, 17)] },
      nav,
    );
    const a = dom.querySelector("a.brink-hover-link");
    expect(a?.textContent).toBe("torch");
    // The label keeps its code styling inside the link.
    expect(a?.querySelector("code")?.textContent).toBe("torch");
    (a as HTMLElement).click();
    expect(nav).toHaveBeenCalledWith(loc("story.ink", 12, 17));
  });

  it("resolves each index to its own target", () => {
    // The failure this rules out: two links both navigating to the first
    // target, which looks like it works until you use the second one.
    const nav = vi.fn();
    const dom = render(
      {
        content: "**effects** reads: [`a`](#0), [`b`](#1)",
        links: [loc("a.ink", 1, 2), loc("b.ink", 3, 4)],
      },
      nav,
    );
    const links = [...dom.querySelectorAll("a.brink-hover-link")] as HTMLElement[];
    expect(links.map((l) => l.textContent)).toEqual(["a", "b"]);
    links[1]!.click();
    expect(nav).toHaveBeenCalledWith(loc("b.ink", 3, 4));
  });

  it("degrades to plain text when the embedder cannot navigate", () => {
    // Same rule "Add to dictionary" follows: a control that silently does
    // nothing is worse than no control.
    const dom = render({ content: "writes: [`torch`](#0)", links: [loc("story.ink", 1, 2)] });
    expect(dom.querySelector("a")).toBeNull();
    expect(dom.textContent).toContain("torch");
  });

  it("degrades to plain text for an unresolved target", () => {
    // An empty `file` is the compiler saying it could not place this
    // target. The entry still occupies its index — dropping it would shift
    // every later link — so the renderer, not the producer, declines.
    const nav = vi.fn();
    const dom = render({ content: "writes: [`torch`](#0)", links: [loc("", 0, 0)] }, nav);
    expect(dom.querySelector("a")).toBeNull();
    expect(dom.textContent).toContain("torch");
  });

  it("degrades to plain text when the index has no entry", () => {
    const nav = vi.fn();
    const dom = render({ content: "writes: [`torch`](#7)", links: [loc("a.ink", 1, 2)] }, nav);
    expect(dom.querySelector("a")).toBeNull();
    expect(dom.textContent).toContain("torch");
  });

  it("does not treat a link's code label as a bare code span", () => {
    // Ordering trap: with the code alternative matched first, `[`torch`](#0)`
    // renders as a stray bracket, a code chip and a literal `(#0)`.
    const nav = vi.fn();
    const dom = render({ content: "[`torch`](#0)", links: [loc("a.ink", 1, 2)] }, nav);
    expect(dom.textContent).toBe("torch");
    expect(dom.textContent).not.toContain("#0");
  });

  it("leaves ordinary markdown alone", () => {
    const dom = render({ content: "**knot** `roll(lo, hi)` *[function]*" });
    expect(dom.querySelector("strong")?.textContent).toBe("knot");
    expect(dom.querySelector("code")?.textContent).toBe("roll(lo, hi)");
    expect(dom.querySelector("em")?.textContent).toBe("[function]");
  });

  it("is keyboard reachable", () => {
    const nav = vi.fn();
    const dom = render({ content: "[`torch`](#0)", links: [loc("a.ink", 1, 2)] }, nav);
    const a = dom.querySelector("a.brink-hover-link") as HTMLElement;
    expect(a.tabIndex).toBe(0);
    a.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", bubbles: true }));
    expect(nav).toHaveBeenCalledTimes(1);
  });
});
