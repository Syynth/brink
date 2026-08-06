import { describe, it, expect, vi } from "vitest";

// #1598: `StoryRunnerHandle.compileFragment`, `evaluate()`'s Tier-1
// fragment-compile step, hardcoded ink-only wrap syntax
// (`=== function NAME() ===` / `=== NAME ===`) — a native parse error when
// appended to a `.brink` entry, so `evaluate()`'s Tier-1 path could never
// reach a native project. With `brink-web` (the wasm-pack output) replaced
// by a call-recording stub (same pattern as `story-runner-flow.test.ts`),
// this pins two things end-to-end through the real exported `evaluate()`:
//   1. the synthetic source handed to `compile_fragment` for a `.brink`
//      entry is native `fn`/`flow` syntax, never ink `=== ===` syntax.
//   2. `evaluate()` completes with a returned `value`, not a diagnostic —
//      i.e. the fragment is reachable, not merely compiled in isolation.
// Real wrap-syntax parsing behavior is pinned by
// `crates/brink-web/src/compile.rs`'s `native_entry_expression_wrap_...` /
// `native_entry_content_wrap_...` tests; this file only proves the TS
// caller wires the right spelling through.

const hoisted = vi.hoisted(() => {
  const compileFragmentCalls: Array<{ entry: string; synthetic: string }> = [];
  // Queue of canned `compile_fragment` responses, consumed one per call —
  // lets a test force the expression-wrap attempt to fail so `compileFragment`
  // falls through to the content-wrap attempt. Empty means "always succeed"
  // (the default single-call behavior the other tests rely on).
  const compileFragmentResponses: Array<{ ok: boolean; warnings?: string[] }> = [];

  class StoryRunnerStub {
    constructor(_bytes: unknown) {}
    checksum(): string {
      return "chk";
    }
    save(): string {
      return "{}";
    }
    load(_json: string): string {
      return "{}";
    }
    lenient_unbound(): boolean {
      return false;
    }
    binding_names(): string[] {
      return [];
    }
    set_lenient_unbound(_v: boolean): void {}
    speculate(_opts: string): unknown {
      return {
        eval_function: (_name: string, _args: unknown[]): string =>
          JSON.stringify({ type: "returned", value: { type: "int", value: 42 } }),
        transcript: (): string => "[]",
        externals_report: (): string => JSON.stringify({ live: [], fallback: [] }),
        go_to_path(_path: string): void {},
        choose(_index: number): void {},
        advance: (): string => JSON.stringify({ type: "done", text: "", tags: [] }),
        take_pending_promise: (): undefined => undefined,
        resolve_external(_v: unknown): void {},
        pending_external_name: (): undefined => undefined,
        free(): void {},
      };
    }
    free(): void {}
  }

  return { compileFragmentCalls, compileFragmentResponses, StoryRunnerStub };
});

vi.mock("brink-web", () => ({
  default: async () => undefined,
  compile: () => "",
  compile_fragment: (entry: string, _sourcesJson: string, synthetic: string): string => {
    hoisted.compileFragmentCalls.push({ entry, synthetic });
    const queued = hoisted.compileFragmentResponses.shift();
    if (queued && !queued.ok) {
      return JSON.stringify({ ok: false, story_bytes: null, warnings: queued.warnings ?? [] });
    }
    return JSON.stringify({ ok: true, story_bytes: [1, 2, 3] });
  },
  program_checksum: () => "",
  token_type_names: () => "[]",
  token_modifier_names: () => "[]",
  EditorSession: class {},
  StoryRunner: hoisted.StoryRunnerStub,
  WebSpeculation: class {},
  WebSession: class {},
  diffSnapshots: () => "",
}));

import { StoryRunnerHandle } from "./index";

describe("evaluate() Tier-1 fragment path uses native wrap syntax for a .brink entry (#1598)", () => {
  it("compiles the fragment as a native `fn` expression wrap, not ink's === ===", async () => {
    hoisted.compileFragmentCalls.length = 0;
    const runner = new StoryRunnerHandle(new Uint8Array());

    const result = await runner.evaluate("gold + 1", {
      projectSource: {
        entry: "main.brink",
        files: { "main.brink": "var gold = 5\n\nflow main() {\n  Hi. -> END\n}\n" },
      },
    });

    expect(hoisted.compileFragmentCalls).toHaveLength(1);
    const call = hoisted.compileFragmentCalls[0];
    expect(call?.entry).toBe("main.brink");
    // Native expression wrap, not ink's `=== function NAME() ===`.
    expect(call?.synthetic).toMatch(/^fn __eval_[0-9a-f]{8}\(\) \{\n {2}return \(gold \+ 1\);\n\}\n$/);
    expect(call?.synthetic).not.toContain("===");

    // Reaches evaluate() end-to-end: a returned value, not a diagnostic.
    expect(result.diagnostics).toEqual([]);
    expect(result.value).toEqual({ type: "int", value: 42 });
  });

  it("still uses ink wrap syntax for an .ink entry", async () => {
    hoisted.compileFragmentCalls.length = 0;
    const runner = new StoryRunnerHandle(new Uint8Array());

    await runner.evaluate("gold + 1", {
      projectSource: {
        entry: "main.ink",
        files: { "main.ink": "VAR gold = 5\n-> start\n\n=== start ===\nHi.\n-> END\n" },
      },
    });

    expect(hoisted.compileFragmentCalls).toHaveLength(1);
    const call = hoisted.compileFragmentCalls[0];
    expect(call?.synthetic).toMatch(/^=== function __eval_[0-9a-f]{8}\(\) ===\n~ return \(gold \+ 1\)\n$/);
  });

  it("falls back to a native content wrap when the expression wrap fails to compile", async () => {
    hoisted.compileFragmentCalls.length = 0;
    // First (expression) attempt fails; second (content) attempt succeeds —
    // exercises the `kind === "content"` -> `goToPath` branch of
    // `evaluateFragment`, which the other tests never reach because their
    // stub always succeeds on the first call.
    hoisted.compileFragmentResponses.length = 0;
    hoisted.compileFragmentResponses.push({ ok: false, warnings: [] }, { ok: true });
    const runner = new StoryRunnerHandle(new Uint8Array());

    const result = await runner.evaluate("You have {gold} gold.", {
      projectSource: {
        entry: "main.brink",
        files: { "main.brink": "var gold = 5\n\nflow main() {\n  Hi. -> END\n}\n" },
      },
    });

    expect(hoisted.compileFragmentCalls).toHaveLength(2);
    const [exprCall, contentCall] = hoisted.compileFragmentCalls;
    expect(exprCall?.synthetic).toMatch(/^fn __eval_[0-9a-f]{8}\(\) \{\n {2}return \(You have \{gold\} gold\.\);\n\}\n$/);
    // Native content wrap, not ink's `=== NAME ===`.
    expect(contentCall?.synthetic).toMatch(/^flow __eval_[0-9a-f]{8}\(\) \{\nYou have \{gold\} gold\.\n\}\n$/);
    expect(contentCall?.synthetic).not.toContain("===");

    // Reaches evaluate() end-to-end via the content path: a transcript, not
    // a diagnostic.
    expect(result.diagnostics).toEqual([]);
    expect(result.value).toBeUndefined();
    expect(result.transcript).toEqual([]);
  });
});
