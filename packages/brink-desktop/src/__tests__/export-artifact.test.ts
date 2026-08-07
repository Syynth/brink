/**
 * Export-artifact verification (#2391 "Export Story (.inkb)", D3 slice 1).
 *
 * The issue's acceptance bar is "prove the artifact is real": this drives
 * `ProjectSession.compileProject()` — the exact production compile surface
 * `export.ts`'s `exportStoryToInkb` reaches via `dispatch("compile.run")`,
 * the same command the Player's Run button dispatches — against a small
 * fixture project, through the REAL wasm module (see `init-real-wasm.ts`;
 * not the studio test suite's `brink-web` mock), then loads the resulting
 * bytes into a fresh `StoryRunnerHandle` and plays it to a choice and past
 * it. A `.inkb` export that doesn't decode/run here would mean
 * `save_bytes_dialog` wrote a corrupt or empty file, not a story.
 *
 * This does not exercise `save_bytes_dialog` itself (a native save dialog
 * isn't automatable) — that Rust command is a native picker + `fs::write`,
 * covered by `cargo test` in `src-tauri` for its own unit-testable pieces,
 * plus the manual drive-rig step below for the full round-trip:
 *
 *   pnpm --filter @brink/desktop tauri dev
 *   → open a project → File → Export Story (.inkb)… → save it
 *   → load the saved file's bytes into a `StoryRunnerHandle` (or
 *     `brink play <path>.inkb`, if built) to confirm it plays.
 */
import { describe, expect, it, beforeAll } from "vitest";
import { InMemoryFileProvider, ProjectSession } from "@brink-lang/editor";
import { StoryRunnerHandle } from "@brink-lang/web";
import { initRealWasmOnce } from "./init-real-wasm.js";

const FIXTURE = {
  "main.ink": `Hello, exported world!
* [Take the lantern] -> lantern
* [Take the coin] -> coin
== lantern ==
You take the lantern.
-> DONE
== coin ==
You take the coin.
-> DONE
`,
};

beforeAll(async () => {
  await initRealWasmOnce();
});

describe("Export Story (.inkb) artifact", () => {
  it("compileProject's bytes decode and play through StoryRunnerHandle", async () => {
    const provider = new InMemoryFileProvider(FIXTURE);
    const project = new ProjectSession({ provider, entryFile: "main.ink" });
    await project.initialize();

    // The exact call `dispatch("compile.run")` reaches, transitively, in
    // the real app (`triggerCompile` → `ProjectSession.compileProject`).
    const result = project.compileProject();
    expect(result.ok).toBe(true);
    expect(result.story_bytes).toBeDefined();
    const bytes = new Uint8Array(result.story_bytes as number[]);
    expect(bytes.length).toBeGreaterThan(0);

    // This is exactly what `save_bytes_dialog` would write to disk — prove
    // it decodes as a genuine, playable story, not an opaque blob.
    const runner = new StoryRunnerHandle(bytes);
    const opening = runner.continueStory();
    expect(opening.map((l) => l.text).join("")).toContain("Hello, exported world!");

    const choicesLine = opening.at(-1);
    expect(choicesLine?.type).toBe("choices");
    expect(choicesLine?.choices).toHaveLength(2);

    runner.choose(0);
    const afterChoice = runner.continueStory();
    expect(afterChoice.map((l) => l.text).join("")).toContain("You take the lantern.");
    expect(afterChoice.at(-1)?.type).toBe("done");
  });
});
