/**
 * Chromium-88 adoptedStyleSheets shim tests (issue #154): the pure
 * feature-detect, install/no-op logic against a fake document host, and the
 * wrapper's mutation→native-setter sync (the style-mod `push()` case that
 * dies on NW.js / RPG Maker MZ).
 */

import { describe, it, expect } from "vitest";
import {
  adoptedStyleSheetsNeedShim,
  installAdoptedStyleSheetsShim,
} from "../adopted-style-sheets.js";

/** A fake Chromium-88-style document: frozen array, prototype accessors. */
function frozenHost() {
  const setterCalls: unknown[][] = [];
  let stored: unknown[] = Object.freeze([]) as unknown as unknown[];
  const proto = {};
  Object.defineProperty(proto, "adoptedStyleSheets", {
    configurable: true,
    get: () => stored,
    set: (value: unknown[]) => {
      setterCalls.push([...value]);
      stored = Object.freeze([...value]) as unknown as unknown[];
    },
  });
  const host = Object.create(proto) as { adoptedStyleSheets: unknown[] };
  return { host, setterCalls, native: () => stored };
}

describe("adoptedStyleSheetsNeedShim (detector)", () => {
  it("detects the frozen Chromium-88 shape", () => {
    expect(adoptedStyleSheetsNeedShim(Object.freeze([]))).toBe(true);
    expect(adoptedStyleSheetsNeedShim(Object.freeze([{}]))).toBe(true);
  });

  it("passes modern mutable arrays and missing support through untouched", () => {
    expect(adoptedStyleSheetsNeedShim([])).toBe(false); // mutable: modern
    expect(adoptedStyleSheetsNeedShim(undefined)).toBe(false); // unsupported (jsdom)
    expect(adoptedStyleSheetsNeedShim(null)).toBe(false);
    expect(adoptedStyleSheetsNeedShim({ length: 0 })).toBe(false); // not an array
  });
});

describe("installAdoptedStyleSheetsShim", () => {
  it("is a zero-overhead no-op on modern hosts", () => {
    const host = { adoptedStyleSheets: [] as unknown[] };
    expect(installAdoptedStyleSheetsShim(host)).toBe(false);
    expect(Object.getOwnPropertyDescriptor(host, "adoptedStyleSheets")?.get).toBeUndefined();
  });

  it("is a no-op when adoptedStyleSheets is unsupported (jsdom)", () => {
    expect(installAdoptedStyleSheetsShim(document)).toBe(false);
  });

  it("installs a mutable wrapper that syncs push() through the native setter", () => {
    const { host, setterCalls, native } = frozenHost();
    expect(installAdoptedStyleSheetsShim(host)).toBe(true);

    const sheetA = { name: "a" };
    const sheets = host.adoptedStyleSheets as unknown[];
    // The style-mod failure mode: in-place push must not throw…
    expect(() => sheets.push(sheetA)).not.toThrow();
    // …and must reach the engine through the native assignment setter.
    expect(setterCalls.at(-1)).toEqual([sheetA]);
    expect(native()).toEqual([sheetA]);
    // The wrapper keeps serving the mutated content.
    expect(host.adoptedStyleSheets).toContain(sheetA);
  });

  it("keeps whole-array assignment working through the wrapper", () => {
    const { host, native } = frozenHost();
    installAdoptedStyleSheetsShim(host);

    const sheetA = { name: "a" };
    const sheetB = { name: "b" };
    host.adoptedStyleSheets = [sheetA, sheetB] as unknown[];
    expect(native()).toEqual([sheetA, sheetB]);
    // Mutation still works after assignment, on the same wrapper.
    (host.adoptedStyleSheets as unknown[]).pop();
    expect(native()).toEqual([sheetA]);
  });

  it("preserves pre-existing sheets at install time", () => {
    const { host, native } = frozenHost();
    const existing = { name: "existing" };
    host.adoptedStyleSheets = [existing] as unknown[]; // native frozen path
    installAdoptedStyleSheetsShim(host);
    (host.adoptedStyleSheets as unknown[]).push({ name: "new" });
    expect(native()).toEqual([existing, { name: "new" }]);
  });
});
