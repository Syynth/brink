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
    apply_project_config(toml: unknown): string {
      calls.push({ method: "apply_project_config", args: [toml] });
      return "[\"unknown key `project.future_key` in brink.toml (ignored)\"]";
    }
    discover_project_config(entry: unknown): string {
      calls.push({ method: "discover_project_config", args: [entry] });
      return "[]";
    }
    set_lint_overrides(json: unknown): string {
      calls.push({ method: "set_lint_overrides", args: [json] });
      return "[]";
    }
    set_deny_warnings_override(deny: unknown): void {
      calls.push({ method: "set_deny_warnings_override", args: [deny] });
    }
    clear_deny_warnings_override(): void {
      calls.push({ method: "clear_deny_warnings_override", args: [] });
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

  it("exposes applyProjectConfig (#1005: the brink.toml editor-mount wiring)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.applyProjectConfig).toBe("function");
  });

  it("forwards applyProjectConfig to apply_project_config, parses the warning JSON, and bumps generation", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const toml = '[project]\ndialect = "brink"\nfuture_key = "x"\n';
    const warnings = handle.applyProjectConfig(toml);

    expect(hoisted.calls).toEqual([
      { method: "apply_project_config", args: [toml] },
    ]);
    expect(warnings).toEqual([
      "unknown key `project.future_key` in brink.toml (ignored)",
    ]);
    expect(handle.generation).toBe(before + 1);
  });

  it("exposes discoverProjectConfig (#1414: SourceTree-seam brink.toml discovery for the virtual web mount)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.discoverProjectConfig).toBe("function");
  });

  it("forwards discoverProjectConfig to discover_project_config, parses the warning JSON, and bumps generation", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const warnings = handle.discoverProjectConfig("main.ink");

    expect(hoisted.calls).toEqual([
      { method: "discover_project_config", args: ["main.ink"] },
    ]);
    expect(warnings).toEqual([]);
    expect(handle.generation).toBe(before + 1);
  });

  // Issue #1417: extends the CLI/API `[lints]`/`deny-warnings` override
  // tier (`brink compile`'s `--deny`/`--warn`/`--allow`/`-D warnings`,
  // #1373) to the wasm editor session. Same passthrough shape as every
  // other lever above: method exists on `EditorSessionHandle`, forwards
  // its argument(s) to the raw wasm binding, bumps `generation`.

  it("exposes setLintOverrides (#1417: the lever was unreachable without it)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.setLintOverrides).toBe("function");
  });

  it("forwards setLintOverrides to set_lint_overrides as JSON, parses the warning JSON, and bumps generation", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const warnings = handle.setLintOverrides({ E014: "deny" });

    expect(hoisted.calls).toEqual([
      { method: "set_lint_overrides", args: ['{"E014":"deny"}'] },
    ]);
    expect(warnings).toEqual([]);
    expect(handle.generation).toBe(before + 1);
  });

  it("exposes setDenyWarningsOverride/clearDenyWarningsOverride (#1417: the levers were unreachable without them)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.setDenyWarningsOverride).toBe("function");
    expect(typeof handle.clearDenyWarningsOverride).toBe("function");
  });

  it("forwards setDenyWarningsOverride to set_deny_warnings_override and bumps generation", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    handle.setDenyWarningsOverride(true);

    expect(hoisted.calls).toEqual([
      { method: "set_deny_warnings_override", args: [true] },
    ]);
    expect(handle.generation).toBe(before + 1);
  });

  it("forwards clearDenyWarningsOverride to clear_deny_warnings_override and bumps generation", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    handle.clearDenyWarningsOverride();

    expect(hoisted.calls).toEqual([
      { method: "clear_deny_warnings_override", args: [] },
    ]);
    expect(handle.generation).toBe(before + 1);
  });
});
