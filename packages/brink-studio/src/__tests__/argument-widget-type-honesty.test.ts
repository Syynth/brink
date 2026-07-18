/**
 * Argument-widget surface honesty for an unregistered semantic type
 * (issue #1053) — extends #1027's hover/signature-help fix to the Form's
 * per-parameter label. `typeName` stays the bare written name (widget-kind
 * matching depends on it, `matchHostWidget`'s fallback); the label renders
 * `typeDisplay` when present, which carries the same warning marker and
 * E040 cross-reference hover/signature-help use for an unregistered type.
 */

import { afterEach, beforeAll, describe, expect, it } from "vitest";
import { openArgumentForm, setHostWidgets, type FormField } from "@brink-lang/editor";

beforeAll(() => {
  if (typeof globalThis.ResizeObserver === "undefined") {
    class ResizeObserverStub {
      observe(): void {}
      unobserve(): void {}
      disconnect(): void {}
    }
    (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = ResizeObserverStub;
  }
});

afterEach(() => {
  setHostWidgets([]);
  document.body.replaceChildren();
});

function anchor(): HTMLElement {
  const el = document.createElement("div");
  document.body.appendChild(el);
  return el;
}

function labelText(): string | null {
  return document.querySelector(".brink-arg-form-label")?.textContent ?? null;
}

describe("argument Form label honesty for semantic types (#1027/#1053)", () => {
  it("an unregistered semantic type's label carries the warning marker, not a bare name", () => {
    const fields: FormField[] = [
      {
        paramName: "id",
        typeName: "var_id",
        typeDisplay: "var_id ⚠ unregistered semantic type — E040",
      },
    ];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    const text = labelText();
    expect(text).not.toBe("id: var_id");
    expect(text).toContain("var_id");
    expect(text).toContain("⚠");
    expect(text).toContain("E040");
  });

  it("a registered semantic type's label renders exactly as before — no warning", () => {
    const fields: FormField[] = [
      { paramName: "id", typeName: "actor_id", typeDisplay: "actor_id" },
    ];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(labelText()).toBe("id: actor_id");
  });

  it("no typeDisplay (older/degraded producer) falls back to the bare typeName label", () => {
    const fields: FormField[] = [{ paramName: "id", typeName: "actor_id" }];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(labelText()).toBe("id: actor_id");
  });

  it("an untyped field still renders just the param name", () => {
    const fields: FormField[] = [{ paramName: "id" }];
    openArgumentForm(anchor(), { fields, onApply: () => {}, onCancel: () => {} });

    expect(labelText()).toBe("id");
  });
});
