/**
 * Fix-on-save must persist EVERY file a cross-file fix batch touched, not
 * only the focused path (issue #3462).
 *
 * `file.save` (⌘S) narrows its host-save write to the single focused path —
 * the correct default for an ordinary edit. But fix-on-save
 * (`docs/autofix-spec.md` §7) runs INSIDE that same command, before the
 * write, and a fix batch can rewrite files other than the one the author
 * asked to save (`brink-studio/src/file-commands.ts`'s `runFixOnSave`
 * already reports every path it wrote — `packages/studio-ui/src/fixActions.ts`).
 * Before #3462, `file.save`'s host-save branch always called
 * `project.save([path])`, so those other files stayed staged (dirty) and
 * silently unpersisted — exactly the gap
 * `packages/brink-desktop/src/tauri-provider.ts`'s own doc comment names
 * ("⌘S narrowed to the focused path, saveAll/autosave unnarrowed").
 *
 * This suite drives the REAL `registerFileCommands` (not a reimplementation
 * of its logic) against a REAL `TauriFileProvider` with a mocked Tauri
 * `invoke`, so a reverted fix goes red here: `provider`'s own
 * `write_file`/`read_file` calls are what confirm persistence, the same
 * calls the desktop shell makes.
 *
 * `registerFileCommands` is not part of `@brink-lang/studio`'s public
 * embedding surface (`docs/embedder-api.md` "What is deliberately NOT
 * exposed" — hosts dispatch by command id, they don't get registries), so
 * this reaches it the same way other cross-package pinning suites already
 * do (e.g. `playground-alias-parity.test.ts` importing
 * `../../../brink-studio/vite.config.js`, or brink-studio's own
 * `headless-theme.test.ts` importing `ink-editor`'s internal
 * `color-widget.js`): a relative import of the sibling package's source,
 * not a new public export.
 */
import { describe, expect, it, vi } from "vitest";
import type { NotificationInput } from "@brink/studio-shell";
import { CommandRegistry } from "@brink/studio-shell";
import type { DocumentSessions, ProjectSession } from "@brink/studio-store";
import {
  registerFileCommands,
  FILE_SAVE_COMMAND_ID,
} from "../../../brink-studio/src/file-commands.js";

const invoke = vi.fn();
const listen = vi.fn();

vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...args: unknown[]) => listen(...args) }));

const { TauriFileProvider } = await import("../tauri-provider.js");
type TauriFileProviderInstance = InstanceType<typeof TauriFileProvider>;

/** One `FixReport`-shaped stub, minimal to what `runFixOnSave` reads. */
interface FakeFixReport {
  files: Array<{ path: string; new_source: string }>;
  error?: string;
}

/**
 * A minimal `ProjectSession` wired to a REAL `TauriFileProvider`: edits and
 * saves cross the exact same seams `ProjectSession` really uses
 * (`applyEdit` stages via `provider.onFileChanged`; `save` writes via
 * `provider.requestSave`; `readProviderFile` reads via `provider.readFile`)
 * — everything else (buffer/baseline bookkeeping) is the minimum a fake
 * needs to answer `getFiles`/`dirtyPaths`/`markFilesSaved` honestly.
 */
function makeFakeProject(
  provider: TauriFileProviderInstance,
  initial: Record<string, string>,
  fixAll: () => FakeFixReport,
): ProjectSession {
  const buffer = new Map(Object.entries(initial));
  const baseline = new Map(Object.entries(initial));
  return {
    applyEdit: (path: string, source: string): boolean => {
      buffer.set(path, source);
      provider.onFileChanged(path, source);
      return true;
    },
    getFiles: (): Record<string, string> => Object.fromEntries(buffer),
    dirtyPaths: (): string[] =>
      [...buffer.keys()].filter((p) => buffer.get(p) !== baseline.get(p)).sort(),
    flushFileChanges: (): unknown[] => [],
    hasHostSave: (): boolean => true,
    save: (paths?: string[]): Promise<void> => provider.requestSave(paths),
    readProviderFile: (path: string): Promise<string> => provider.readFile(path),
    markFilesSaved: (paths: Iterable<string>): void => {
      for (const p of paths) baseline.set(p, buffer.get(p) ?? "");
    },
    markAllSaved: (): void => {
      for (const p of buffer.keys()) baseline.set(p, buffer.get(p) ?? "");
    },
    getSession: () => ({ fixAll }),
  } as unknown as ProjectSession;
}

function makeFakeDocuments(focused: string | null): DocumentSessions {
  return {
    flushFocused: () => focused,
    flushAll: () => [],
    invalidateFile: () => {},
  } as unknown as DocumentSessions;
}

/** A fresh `TauriFileProvider` whose `invoke` mock persists writes into an
 *  in-memory "disk" and answers `read_file` from it — real enough for
 *  `requestSave`'s own staging/serialization to run unmodified. */
function makeProviderWithDisk(): { provider: TauriFileProviderInstance; disk: Map<string, string> } {
  const disk = new Map<string, string>();
  invoke.mockReset();
  invoke.mockImplementation((cmd: string, args?: Record<string, unknown>) => {
    if (cmd === "write_file") {
      disk.set(args?.["rel"] as string, args?.["content"] as string);
      return Promise.resolve(undefined);
    }
    if (cmd === "read_file") {
      return Promise.resolve(disk.get(args?.["rel"] as string) ?? "");
    }
    return Promise.resolve(undefined);
  });
  const provider = new TauriFileProvider("/proj");
  return { provider, disk };
}

function dispatchSave(project: ProjectSession, focused: string | null): NotificationInput[] {
  const notifications: NotificationInput[] = [];
  const commands = new CommandRegistry();
  const dispose = registerFileCommands(commands, {
    project,
    documents: makeFakeDocuments(focused),
    notify: (n) => notifications.push(n),
    fixOnSave: () => "safe",
  });
  commands.dispatch(FILE_SAVE_COMMAND_ID);
  dispose();
  return notifications;
}

async function settle(): Promise<void> {
  for (let i = 0; i < 10; i += 1) await Promise.resolve();
}

describe("fix-on-save cross-file save routing (#3462)", () => {
  it("a two-file fix batch: both files are written to disk, both go clean, and a toast names the second file", async () => {
    const { provider, disk } = makeProviderWithDisk();
    const project = makeFakeProject(
      provider,
      { "a.brink": "orig a", "b.brink": "orig b" },
      () => ({
        files: [
          { path: "a.brink", new_source: "fixed a" },
          { path: "b.brink", new_source: "fixed b" },
        ],
      }),
    );

    const notifications = dispatchSave(project, "a.brink");
    await settle();

    // Both files actually reached the provider's own write_file — the real
    // narrowing this issue is about, not a reimplemented stand-in for it.
    expect(disk.get("a.brink")).toBe("fixed a");
    expect(disk.get("b.brink")).toBe("fixed b");

    // Both go clean — a save that "succeeded" while quietly leaving the
    // second file dirty is exactly the bug.
    expect(project.dirtyPaths()).toEqual([]);

    const messages = notifications.map((n) => n.message);
    expect(messages).toContain("Saved a.brink");
    // A toast NAMES the other file the batch touched (fix-on-save still
    // raises no toast of its own for the focused file beyond the ordinary
    // "Saved" notice above).
    expect(messages.some((m) => m.includes("b.brink"))).toBe(true);
    expect(messages.some((m) => m.includes("still unsaved"))).toBe(false);
  });

  it("a single-file fix batch (or none): only the focused file is written, and no extra toast names another file", async () => {
    const { provider, disk } = makeProviderWithDisk();
    const project = makeFakeProject(
      provider,
      { "a.brink": "orig a", "b.brink": "orig b" },
      () => ({
        // The fix only touched its own file — the ordinary, non-cross-file
        // case `runFixOnSave` reports today (no registered fixer produces a
        // cross-file edit yet).
        files: [{ path: "a.brink", new_source: "fixed a" }],
      }),
    );

    const notifications = dispatchSave(project, "a.brink");
    await settle();

    expect(disk.get("a.brink")).toBe("fixed a");
    expect(disk.has("b.brink")).toBe(false); // untouched file never written
    expect(project.dirtyPaths()).toEqual([]);

    const messages = notifications.map((n) => n.message);
    expect(messages).toEqual(["Saved a.brink"]); // no second, "also wrote" toast
  });
});
