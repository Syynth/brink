/**
 * wasmLocation passthrough (issue #154): `MountStudioOptions.wasmLocation`
 * forwards to `initWasm`, making the IIFE-plugin case (no usable
 * `import.meta.url`, e.g. an RPG Maker MZ plugin bundle) intentional
 * instead of relying on a host pre-call and the double-init guard.
 *
 * initWasm is stubbed to reject after recording its argument — the mount
 * aborts right there, so this exercises exactly the passthrough without
 * bootstrapping the full studio in jsdom.
 */

import { describe, it, expect, beforeEach, vi } from "vitest";
import { mountStudio } from "../mount.js";

const { initWasmSpy } = vi.hoisted(() => ({
  initWasmSpy: vi.fn((_location?: unknown) => Promise.reject(new Error("stop-after-init"))),
}));

vi.mock("@brink-lang/web", async (importOriginal) => {
  const original = await importOriginal<typeof import("@brink-lang/web")>();
  return { ...original, initWasm: initWasmSpy };
});

const OPTIONS = { files: { "main.ink": "-> END\n" }, entryFile: "main.ink" };

beforeEach(() => {
  initWasmSpy.mockClear();
});

describe("MountStudioOptions.wasmLocation", () => {
  it("forwards the location to initWasm", async () => {
    const location = "https://cdn.example/brink_web_bg.wasm";
    await expect(
      mountStudio(document.createElement("div"), { ...OPTIONS, wasmLocation: location }),
    ).rejects.toThrow("stop-after-init");
    expect(initWasmSpy).toHaveBeenCalledExactlyOnceWith(location);
  });

  it("defaults to module-relative resolution (undefined) when omitted", async () => {
    await expect(mountStudio(document.createElement("div"), OPTIONS)).rejects.toThrow(
      "stop-after-init",
    );
    expect(initWasmSpy).toHaveBeenCalledExactlyOnceWith(undefined);
  });
});
