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
  let configuredEntryStub: string | undefined;
  let fixAllReportStub = "{}";
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
    configured_entry(): string | undefined {
      calls.push({ method: "configured_entry", args: [] });
      return configuredEntryStub;
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
    is_read_only(path: unknown): boolean {
      calls.push({ method: "is_read_only", args: [path] });
      return path === "std/core.brink";
    }
    remove_file(path: unknown): boolean {
      calls.push({ method: "remove_file", args: [path] });
      // Mirrors the real Rust-side refusal (issue #2306/#2343): a mounted
      // path's remove is refused (returns false), everything else succeeds.
      return path !== "std/core.brink";
    }
    fix_all(json: unknown): string {
      calls.push({ method: "fix_all", args: [json] });
      return fixAllReportStub;
    }
  }
  return {
    calls,
    EditorSessionStub,
    setConfiguredEntry: (value: string | undefined) => {
      configuredEntryStub = value;
    },
    setFixAllReport: (report: string) => {
      fixAllReportStub = report;
    },
  };
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

import { EditorSessionHandle } from "../index";

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

  // Issue #2331: "[project] entry" beats mountStudio's entryFile — the
  // read side hosts poll after discovery to learn whether brink.toml named
  // an entry file.

  it("exposes getConfiguredEntry (#2331)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.getConfiguredEntry).toBe("function");
  });

  it("forwards getConfiguredEntry to configured_entry, without bumping generation (a read, not a mutation)", () => {
    hoisted.setConfiguredEntry("story.ink");
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const entry = handle.getConfiguredEntry();

    expect(hoisted.calls).toEqual([{ method: "configured_entry", args: [] }]);
    expect(entry).toBe("story.ink");
    expect(handle.generation).toBe(before);
  });

  it("returns null (not undefined) when the wasm side has no configured entry", () => {
    hoisted.setConfiguredEntry(undefined);
    const handle = new EditorSessionHandle();
    expect(handle.getConfiguredEntry()).toBeNull();
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

  it("forwards setLintOverrides with an info/hint level (#1162: the advisory tier below Warning)", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const warnings = handle.setLintOverrides({ E014: "hint" });

    expect(hoisted.calls).toEqual([
      { method: "set_lint_overrides", args: ['{"E014":"hint"}'] },
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

  // Issue #2306 (ruled 2026-08-06 "Mounted stdlib presents as a read-only
  // library node"): the session-level read-only query — the primitive
  // `ProjectSession.applyEdit` consults before writing through the bulk
  // edit seam (search/replace, results-buffer edits, binder undo).

  it("exposes isReadOnly (#2306: the session-level read-only query)", () => {
    const handle = new EditorSessionHandle();
    expect(typeof handle.isReadOnly).toBe("function");
  });

  it("forwards isReadOnly to is_read_only and does NOT bump generation (a query, not a mutation)", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const readOnly = handle.isReadOnly("std/core.brink");
    const notReadOnly = handle.isReadOnly("main.ink");

    expect(hoisted.calls).toEqual([
      { method: "is_read_only", args: ["std/core.brink"] },
      { method: "is_read_only", args: ["main.ink"] },
    ]);
    expect(readOnly).toBe(true);
    expect(notReadOnly).toBe(false);
    expect(handle.generation).toBe(before);
  });

  // Issue #2306/#2343: `remove_file` gained a boolean return (refused for a
  // mounted path) alongside the listed-but-marked flag flip on `listFiles`/
  // `getProjectOutline`/`getStoryGraph` — the delete-route gap #2343's
  // review comment named. `removeFile` must forward that boolean, not
  // swallow it back to `void`.

  it("forwards remove_file's boolean return (issue #2306/#2343)", () => {
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const refused = handle.removeFile("std/core.brink");
    const applied = handle.removeFile("main.ink");

    expect(hoisted.calls).toEqual([
      { method: "remove_file", args: ["std/core.brink"] },
      { method: "remove_file", args: ["main.ink"] },
    ]);
    expect(refused).toBe(false);
    expect(applied).toBe(true);
    // `removeFile` still bumps generation unconditionally — a refusal is a
    // property of what the write attempted, not evidence nothing needs
    // re-analyzing (mirrors `updateFile`'s bump-regardless contract).
    expect(handle.generation).toBe(before + 2);
  });

  // Adversarial review on PR #3454 (finding 1): `fixAll` bumped `generation`
  // unconditionally even though `fix_all`'s decision 3 (`docs/autofix-spec.md`
  // §5) rolls the session back to exactly what it was. `mutationCount` is
  // the cache key `ProjectSession.compileProject()` keys on
  // (`packages/ink-editor/src/project-session.ts`), so an unconditional
  // bump forces a full project recompile on every `fixAll` call — including
  // the common `runFixOnSave` path, where today's all-Suggested fixer
  // roster makes the batch a no-op at the "safe" ceiling on every save.

  it("does NOT bump generation when fixAll's report applies no files (a no-op batch)", () => {
    hoisted.setFixAllReport(
      JSON.stringify({
        applied: [],
        skipped_overlap: 0,
        remaining: [],
        rounds: 0,
        cap_hit: false,
        files: [],
      }),
    );
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const report = handle.fixAll({ tiers: ["safe"] });

    expect(hoisted.calls).toEqual([
      { method: "fix_all", args: [JSON.stringify({ tiers: ["safe"] })] },
    ]);
    expect(report.files).toEqual([]);
    expect(handle.generation).toBe(before);
  });

  it("bumps generation when fixAll's report actually rewrites a file", () => {
    hoisted.setFixAllReport(
      JSON.stringify({
        applied: [{ code: "E025", path: "main.brink" }],
        skipped_overlap: 0,
        remaining: [],
        rounds: 1,
        cap_hit: false,
        files: [{ path: "main.brink", new_source: "use a::b;\n" }],
      }),
    );
    hoisted.calls.length = 0;
    const handle = new EditorSessionHandle();
    const before = handle.generation;

    const report = handle.fixAll({});

    expect(report.files).toHaveLength(1);
    expect(handle.generation).toBe(before + 1);
  });
});
