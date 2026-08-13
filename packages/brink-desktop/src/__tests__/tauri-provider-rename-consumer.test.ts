/**
 * Consumer-level regression test for the `TauriFileProvider.renameFile`
 * self-rename suppression gap (#2416).
 *
 * `tauri-provider.test.ts`'s self-rename tests exercise `TauriFileProvider`
 * directly against a hand-mirrored `FileChangeHub`, the same pattern
 * #2404's delete-suppression test used — useful, but it hand-mirrors
 * `ProjectSession`'s real `onExternalChange` handler (project-session.ts)
 * rather than driving it. This test instead drives a REAL `ProjectSession`
 * (through the REAL wasm module, matching `export-artifact.test.ts`'s
 * `initRealWasmOnce` rather than `@brink-lang/studio`'s hand-written
 * `brink-web` mock) over a real `TauriFileProvider` — `ProjectSession.
 * renameFile` → provider write-through → the shell's simulated watcher
 * echo → `ProjectSession`'s own external-change handler — so a future
 * divergence between that handler and any hand-mirror would be caught here.
 *
 * The assertion is what the pending-egress queue DELIVERS
 * (`flushFileChanges()`, the `onFilesChanged` host callback), not that an
 * internal marker set flipped — the house discipline the #2404 test also
 * follows, extended to `renameFile`.
 */
import { describe, expect, it, beforeAll, vi } from "vitest";
import { ProjectSession, type FileChange } from "@brink-lang/editor";
import { initRealWasmOnce } from "./init-real-wasm.js";

const invoke = vi.fn();
const listen = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...args: unknown[]) => listen(...args) }));

const { TauriFileProvider } = await import("../tauri-provider.js");

type WatcherCallback = (event: { payload: { path: string; content: string | null } }) => void;

/** Capture the `fs:external-change` handler `onExternalChange` registers —
 *  the seam `ProjectSession.initialize()` itself subscribes through. */
function captureWatcherCallback(): { get: () => WatcherCallback } {
  let captured: WatcherCallback | undefined;
  listen.mockImplementation((_event: string, cb: WatcherCallback) => {
    captured = cb;
    return Promise.resolve(() => {});
  });
  return {
    get: () => {
      if (captured === undefined) {
        throw new Error("listen() was not called — onExternalChange did not subscribe");
      }
      return captured;
    },
  };
}

const SCENE = "Hello.\n-> END\n";

beforeAll(async () => {
  await initRealWasmOnce();
});

describe("ProjectSession over TauriFileProvider: self-rename suppression (#2416)", () => {
  it("delivers both the 'deleted' and 'created' egress records once the rename's own watcher echoes come back", async () => {
    const watcher = captureWatcherCallback();
    const files = new Map<string, string>([["scene.ink", SCENE]]);
    invoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
      switch (cmd) {
        case "list_files":
          return Promise.resolve([...files.keys()]);
        case "read_file": {
          const content = files.get(args?.["rel"] as string);
          return content === undefined
            ? Promise.reject(new Error("not found"))
            : Promise.resolve(content);
        }
        case "rename_file": {
          const from = args?.["from"] as string;
          const to = args?.["to"] as string;
          const content = files.get(from);
          files.delete(from);
          if (content !== undefined) files.set(to, content);
          return Promise.resolve(undefined);
        }
        default:
          return Promise.resolve(undefined);
      }
    });

    const provider = new TauriFileProvider("/proj");
    const delivered: FileChange[][] = [];
    const project = new ProjectSession({
      provider,
      entryFile: "scene.ink",
      onFilesChanged: (changes) => delivered.push(changes),
    });
    await project.initialize();

    // `ProjectSession.renameFile`'s real call sequence: session re-key,
    // provider write-through (the real `TauriFileProvider.renameFile`,
    // exercised end to end here — no mock of it), then host-egress record.
    const referrers = await project.renameFile("scene.ink", "renamed.ink");
    expect(referrers).toEqual([]);

    // The shell's fs watcher observes the native `rename_file` call and —
    // after its debounce — echoes it back as a deletion of the old path plus
    // a creation of the new path, exactly like a real `notify` watcher
    // would. This is the SAME callback `ProjectSession.initialize()`
    // registered — no hand-mirrored handler in this test.
    watcher.get()({ payload: { path: "scene.ink", content: null } });
    watcher.get()({ payload: { path: "renamed.ink", content: SCENE } });

    // Drive the real pending-egress queue to flush instead of asserting an
    // internal marker: without suppression, the two echoes above would have
    // reached `ProjectSession`'s external-change handler and wiped the
    // pending "deleted"/"created" records via `FileChangeHub.applyExternal`
    // before this flush ever ran, delivering nothing (or a partial batch).
    const flushed = project.flushFileChanges();
    expect(flushed).toEqual([
      { path: "renamed.ink", type: "created", content: SCENE },
      { path: "scene.ink", type: "deleted" },
    ]);
    expect(delivered).toEqual([flushed]);

    // The session itself reflects the rename regardless of the egress
    // outcome — the two are independent, and only the egress half is what
    // this issue's gap silently broke.
    const session = project.getSession();
    expect(session.getFileSource("renamed.ink")).toBe(SCENE);
    expect(session.getFileSource("scene.ink")).toBeNull();
  });
});
