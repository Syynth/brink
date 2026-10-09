/**
 * `ProjectSession` follows the files `brink.toml` reads (#3671; decision
 * log 2026-10-09): the host manifest and `[dialogue]`'s file. After a config
 * apply it fetches the ones the session lacks, tells the host to watch them,
 * and applies the config again when one changes — exactly as it does for
 * `brink.toml` itself.
 *
 * Uses the hand-built stub `session` pattern of
 * `entry-is-explicit.test.ts` (no built wasm pkg needed): what the config
 * resolves to is the Rust session's job, pinned by brink-web's own test;
 * what is under test here is `ProjectSession`'s bookkeeping around it.
 */

import { describe, it, expect } from "vitest";
import { ProjectSession } from "../project-session.js";
import { InMemoryFileProvider } from "../provider.js";

const MANIFEST = "build/host.json";

/** A session whose config reads `build/host.json`, and which says the
 *  manifest is missing until a document by that name is loaded. */
function makeStubSession() {
  const docs = new Map<string, string>();
  const stub = {
    applies: 0,
    generation: 0,
    updateFile: (path: string, content: string) => void docs.set(path, content),
    removeFile: (path: string) => void docs.delete(path),
    getFileSource: (path: string) => docs.get(path) ?? null,
    discoverProjectConfig: () => {
      stub.applies += 1;
      return [];
    },
    getConfiguredEntry: () => null,
    getConfiguredConfigReads: () => [MANIFEST],
    getConfiguredHostManifestError: () =>
      docs.has(MANIFEST) ? null : `host manifest \`${MANIFEST}\` could not be read`,
    getConfiguredWarnings: () =>
      docs.has(MANIFEST) ? [] : [`host manifest \`${MANIFEST}\` could not be read`],
    getFileIncludes: () => [],
    listFiles: () => [...docs.keys()],
    compileProject: () => {},
    free: () => {},
  };
  return stub;
}

/** A host that lists only story files and the config — the desktop shell's
 *  shape — but serves any file on request, and records what it is told to
 *  watch. */
class StoryOnlyProvider extends InMemoryFileProvider {
  watched: string[][] = [];
  requested: string[] = [];
  async listFiles(): Promise<string[]> {
    return (await super.listFiles()).filter((p) => !p.endsWith(".json"));
  }
  async requestFile(path: string): Promise<string | null> {
    this.requested.push(path);
    return super.requestFile(path);
  }
  watchConfigFiles(paths: readonly string[]): void {
    this.watched.push([...paths]);
  }
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

async function open(files: Record<string, string>) {
  const provider = new StoryOnlyProvider(files);
  const session = makeStubSession();
  const project = new ProjectSession({
    provider,
    entryFile: "story.brink",
    // eslint-disable-next-line @typescript-eslint/no-explicit-any -- hand stub, see makeStubSession's doc
    session: session as any,
  });
  await project.initialize();
  await settle();
  return { provider, session, project };
}

describe("ProjectSession follows the files brink.toml reads (#3671)", () => {
  it("fetches a config read the host did not list, has it watched, and applies again", async () => {
    const { provider, session, project } = await open({
      "brink.toml": '[host]\nmanifest = "build/host.json"\n',
      "story.brink": "flow a() {}\n",
      [MANIFEST]: '{ "markup": [] }',
    });
    expect(provider.requested).toContain(MANIFEST);
    expect(provider.watched.at(-1)).toEqual([MANIFEST]);
    expect(session.applies).toBe(2); // initialize, then again once it arrived
    expect(project.getConfiguredHostManifestError()).toBeNull();
    expect(project.getConfiguredWarnings()).toEqual([]);
    project.destroy();
  });

  it("a config read that is not there stays missing, and says so", async () => {
    const { session, project } = await open({
      "brink.toml": '[host]\nmanifest = "build/host.json"\n',
      "story.brink": "flow a() {}\n",
    });
    expect(session.applies).toBe(1); // nothing arrived: no second apply
    expect(project.getConfiguredHostManifestError()).toContain(MANIFEST);
    expect(project.getConfiguredWarnings()).toHaveLength(1);
    project.destroy();
  });

  it("applies the config again when a file it reads changes on disk, and only then", async () => {
    const { provider, session, project } = await open({
      "brink.toml": '[host]\nmanifest = "build/host.json"\n',
      "story.brink": "flow a() {}\n",
    });
    const before = session.applies;
    provider.pushExternalChange("story.brink", "flow b() {}\n");
    expect(session.applies).toBe(before); // a story edit is not the config's
    provider.pushExternalChange(MANIFEST, '{ "markup": [] }');
    expect(session.applies).toBe(before + 1);
    expect(project.getConfiguredHostManifestError()).toBeNull();
    provider.pushExternalChange(MANIFEST, null);
    expect(session.applies).toBe(before + 2);
    expect(project.getConfiguredHostManifestError()).toContain(MANIFEST);
    project.destroy();
  });
});
