import { describe, expect, it } from "vitest";
import {
  ProjectTitle,
  configTextIn,
  readProjectName,
  windowTitle,
} from "../project-title.js";

describe("windowTitle", () => {
  it("names the project by its [project] name, else by its folder", () => {
    expect(windowTitle("Harbour Lights", "/Users/b/stories/harbour")).toBe(
      "Harbour Lights — Brink Studio",
    );
    expect(windowTitle(null, "/Users/b/stories/harbour")).toBe("harbour — Brink Studio");
  });
});

describe("configTextIn", () => {
  it("answers brink.toml's newest text, null for a delete, undefined when untouched", () => {
    expect(configTextIn([{ path: "main.ink", type: "modified", content: "x" }])).toBeUndefined();
    expect(
      configTextIn([
        { path: "brink.toml", type: "modified", content: "old" },
        { path: "main.ink", type: "modified", content: "x" },
        { path: "brink.toml", type: "modified", content: "new" },
      ]),
    ).toBe("new");
    expect(configTextIn([{ path: "brink.toml", type: "deleted" }])).toBeNull();
    // Only the root config: a nested one is not the project's.
    expect(
      configTextIn([{ path: "sub/brink.toml", type: "modified", content: "x" }]),
    ).toBeUndefined();
  });
});

/** A shell stand-in whose answers the test releases by hand. */
function deferredNames() {
  const pending: Array<{ text: string; resolve: (name: string | null) => void }> = [];
  const nameOf = (text: string) =>
    new Promise<string | null>((resolve) => pending.push({ text, resolve }));
  return { pending, nameOf };
}

describe("ProjectTitle", () => {
  it("titles from the config, and the latest ask wins over a slower older one", async () => {
    const { pending, nameOf } = deferredNames();
    const titles: string[] = [];
    const title = new ProjectTitle("/p/harbour", nameOf, (t) => titles.push(t));

    const first = title.update("name = 'Harbour'");
    const second = title.update("name = 'Lanterns'");
    pending[1]?.resolve("Lanterns");
    await second;
    pending[0]?.resolve("Harbour");
    await first;
    expect(titles).toEqual(["Lanterns — Brink Studio"]);

    await title.update(null);
    expect(titles.at(-1)).toBe("harbour — Brink Studio");
  });

  it("falls back to the folder when the config sets no name", async () => {
    const titles: string[] = [];
    const title = new ProjectTitle(
      "/p/harbour",
      () => Promise.resolve(null),
      (t) => titles.push(t),
    );
    await title.update("[project]\nentry = \"main.ink\"\n");
    expect(titles).toEqual(["harbour — Brink Studio"]);
  });

  it("applies nothing once disposed — the project closed while the shell answered", async () => {
    const { pending, nameOf } = deferredNames();
    const titles: string[] = [];
    const title = new ProjectTitle("/p/harbour", nameOf, (t) => titles.push(t));
    const asked = title.update("name = 'Harbour'");
    title.dispose();
    pending[0]?.resolve("Harbour");
    await asked;
    expect(titles).toEqual([]);
  });
});

describe("readProjectName", () => {
  it("reads the config beside its folder and parses it", async () => {
    const reads: Array<[string, string]> = [];
    const name = await readProjectName(
      "/Users/b/stories/harbour/brink.toml",
      (root, rel) => {
        reads.push([root, rel]);
        return Promise.resolve("[project]\nname = \"Harbour Lights\"\n");
      },
      (text) => Promise.resolve(text.includes("Harbour Lights") ? "Harbour Lights" : null),
    );
    expect(name).toBe("Harbour Lights");
    expect(reads).toEqual([["/Users/b/stories/harbour", "brink.toml"]]);
  });

  it("is null when the config is gone", async () => {
    const name = await readProjectName(
      "/gone/brink.toml",
      () => Promise.reject(new Error("no such file")),
      () => Promise.resolve("never asked"),
    );
    expect(name).toBeNull();
  });
});
