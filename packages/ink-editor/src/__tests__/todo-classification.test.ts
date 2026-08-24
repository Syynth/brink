/**
 * TODO author-note classification (#3050): `TODO` opening a line is the
 * parser's `AUTHOR_WARNING` (colon optional), and the editor mirrors that
 * as the `todo` element kind so the `brink-todo` line class — the host's
 * amber-band styling hook — lands exactly where the compiler sees a note.
 */
import { describe, expect, it } from "vitest";
import { classifyLine, ElementType, elementClass } from "../element-type.js";

describe("TODO line classification", () => {
  it("classifies TODO: lines as todo", () => {
    expect(classifyLine("TODO: tighten the opening").type).toBe(ElementType.Todo);
  });
  it("colon is optional, matching the parser", () => {
    expect(classifyLine("TODO tighten the opening").type).toBe(ElementType.Todo);
    expect(classifyLine("TODO").type).toBe(ElementType.Todo);
  });
  it("leading whitespace is fine", () => {
    expect(classifyLine("   TODO: indented note").type).toBe(ElementType.Todo);
  });
  it("does not swallow identifiers that merely start with TODO", () => {
    expect(classifyLine("TODOS are great").type).toBe(ElementType.NarrativeText);
  });
  it("mid-line TODO stays whatever the line already is", () => {
    expect(classifyLine("* TODO inside a choice").type).toBe(ElementType.Choice);
    expect(classifyLine("remember the TODO: here").type).toBe(ElementType.NarrativeText);
  });
  it("derives the brink-todo class", () => {
    expect(elementClass(ElementType.Todo)).toBe("brink-todo");
  });
});
