import { describe, it, expect, vi } from "vitest";

// Regression coverage for the PR #534 review finding (#672 lane G): the
// `set_semantic_type_check` wasm lever existed on `WasmEditorSession`, but
// `EditorSessionHandle` — the only surface `@brink-lang/web` consumers ever
// touch — had no passthrough method, so the lever was unreachable from the
// published package. The fix added `setSemanticTypeCheck` mirroring
// `setExternalCheck`'s shape exactly, including the `bump()`
// cache-invalidation.
//
// Issue #693 found the same gap for `set_language_dialect` (#611) and
// `set_type_policy` (#660): the raw `#[wasm_bindgen]` methods existed on
// `WasmEditorSession`, but `EditorSessionHandle` exposed neither, so no JS
// caller of `@brink-lang/web` could enable the brink dialect or the typed
// mode policy at all. `setLanguageDialect`/`setTypePolicy` follow the same
// forward + bump pattern.
//
// `brink-web` (the wasm-pack output) is replaced with a call-recording stub
// so this stays a pure wrapper-layer test: it pins the passthrough wiring
// (method exists, forwards its argument, bumps `generation`), not wasm
// behavior — the Rust side of the lever is covered in `crates/brink-web`.

const hoisted = vi.hoisted(() => {
  const calls: Array<{ method: string; args: unknown[] }> = [];
  class EditorSessionStub {
    set_semantic_type_check(level: unknown): void {
      calls.push({ method: "set_semantic_type_check", args: [level] });
    }
    set_external_check(level: unknown): void {
      calls.push({ method: "set_external_check", args: [level] });
    }
    set_language_dialect(value: unknown): void {
      calls.push({ method: "set_language_dialect", args: [value] });
    }
    set_type_policy(value: unknown): void {
      calls.push({ method: "set_type_policy", args: [value] });
    }
  }
  return { calls, EditorSessionStub };
});

vi.mock("brink-web", () => ({
  default: async () => undefined,
  compile: () => "",
  compile_fragment: () => "",
  program_checksum: () => "",
  token_type_names: () => "[]",
  token_modifier_names: () => "[]",
  EditorSession: hoisted.EditorSessionStub,
  StoryRunner: class {},
  WebSpeculation: class {},
  WebSession: class {},
  diffSnapshots: () => "",
}));

import { EditorSessionHandle } from "./index";

describe("EditorSessionHandle wasm-lever passthroughs", () => {
  it("exposes setSemanticTypeCheck (PR #534 review: the lever was unreachable without it)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.setSemanticTypeCheck).toBe("function");
  });

  it("forwards setSemanticTypeCheck to set_semantic_type_check and bumps generation", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    handle.setSemanticTypeCheck("error");

    expect(hoisted.calls).toEqual([
      { method: "set_semantic_type_check", args: ["error"] },
    ]);
    expect(handle.generation).toBe(before + 1);
  });

  it("keeps the setExternalCheck pattern it mirrors: forward + bump", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    handle.setExternalCheck("off");

    expect(hoisted.calls).toEqual([
      { method: "set_external_check", args: ["off"] },
    ]);
    expect(handle.generation).toBe(before + 1);
  });

  it("exposes setLanguageDialect (#693: the lever was unreachable without it)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.setLanguageDialect).toBe("function");
  });

  it("forwards setLanguageDialect to set_language_dialect and bumps generation", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    handle.setLanguageDialect("brink");

    expect(hoisted.calls).toEqual([
      { method: "set_language_dialect", args: ["brink"] },
    ]);
    expect(handle.generation).toBe(before + 1);
  });

  it("exposes setTypePolicy (#693: the lever was unreachable without it)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.setTypePolicy).toBe("function");
  });

  it("forwards setTypePolicy to set_type_policy and bumps generation", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    handle.setTypePolicy("strict");

    expect(hoisted.calls).toEqual([
      { method: "set_type_policy", args: ["strict"] },
    ]);
    expect(handle.generation).toBe(before + 1);
  });
});
