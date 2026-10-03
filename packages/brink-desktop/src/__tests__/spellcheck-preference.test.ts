/**
 * The spelling preference and the checker's response to it.
 *
 * The interesting failures are all "the switch says one thing and the
 * checker does another", so these assert the two together rather than the
 * storage round-trip alone.
 */
import { describe, expect, it, vi } from "vitest";
import type { ProseChecker, ProseLint } from "@brink-lang/studio";
import type { SpellcheckOutcome } from "../tauri-provider.js";
import { desktopProseChecker } from "../desktop-prose-checker.js";
import {
  SYSTEM_SPELLCHECK_DEFAULT,
  SYSTEM_SPELLCHECK_KEY,
  readUseSystemSpellcheck,
  writeUseSystemSpellcheck,
} from "../spellcheck-preference.js";

/** A `Storage` backed by a Map, or one that throws on every accessor. */
function fakeStorage({ throws = false }: { throws?: boolean } = {}): Storage {
  const map = new Map<string, string>();
  const boom = (): never => {
    throw new Error("site data blocked");
  };
  return {
    get length() {
      return map.size;
    },
    clear: () => map.clear(),
    key: (i: number) => [...map.keys()][i] ?? null,
    getItem: throws ? boom : (k: string) => map.get(k) ?? null,
    setItem: throws ? boom : (k: string, v: string) => void map.set(k, v),
    removeItem: (k: string) => void map.delete(k),
  } as Storage;
}

function lint(kind: string): ProseLint {
  return { start: 0, end: 1, kind, message: kind, suggestions: [] };
}

describe("the preference", () => {
  it("defaults on, so the better checker is not hidden behind a setting", () => {
    // The platform checker knows the author's own words; a bundled
    // dictionary starts from zero on every machine.
    expect(SYSTEM_SPELLCHECK_DEFAULT).toBe(true);
    expect(readUseSystemSpellcheck(fakeStorage())).toBe(true);
  });

  it("round-trips both ways", () => {
    const storage = fakeStorage();
    writeUseSystemSpellcheck(storage, false);
    expect(storage.getItem(SYSTEM_SPELLCHECK_KEY)).toBe("false");
    expect(readUseSystemSpellcheck(storage)).toBe(false);
    writeUseSystemSpellcheck(storage, true);
    expect(readUseSystemSpellcheck(storage)).toBe(true);
  });

  it("reads a corrupted or absent value as the default rather than as off", () => {
    // Defaulting a junk value to "off" would silently downgrade spelling
    // with nothing on screen to say why.
    const storage = fakeStorage();
    storage.setItem(SYSTEM_SPELLCHECK_KEY, "yes please");
    expect(readUseSystemSpellcheck(storage)).toBe(true);
    expect(readUseSystemSpellcheck(undefined)).toBe(true);
  });

  it("survives storage that throws on every accessor", () => {
    // Private window, or site data blocked. Neither is a reason to break
    // the settings pane.
    const storage = fakeStorage({ throws: true });
    expect(readUseSystemSpellcheck(storage)).toBe(true);
    expect(() => writeUseSystemSpellcheck(storage, false)).not.toThrow();
  });
});

describe("the checker honours it", () => {
  const builtin: ProseChecker = {
    check: () => Promise.resolve([lint("Spelling"), lint("Agreement")]),
  };

  it("keeps Harper's spelling and never calls the platform when off", async () => {
    // Short-circuited BEFORE the IPC, not after: a preference that still
    // pays for the call on every keystroke only half works.
    const spellcheck = vi.fn(() =>
      Promise.resolve<SpellcheckOutcome>({ kind: "checked", misspellings: [] }),
    );
    const checker = desktopProseChecker(builtin, spellcheck, () => false);
    const lints = await checker.check({ text: "hi", spans: [], dictionary: [], dialect: "" });

    expect(spellcheck).not.toHaveBeenCalled();
    expect(lints.map((l) => l.kind)).toEqual(["Spelling", "Agreement"]);
  });

  it("re-reads the preference on every check, not once at construction", async () => {
    // The settings toggle has to take effect on the next check rather than
    // the next launch.
    let on = false;
    const spellcheck = vi.fn(() =>
      Promise.resolve<SpellcheckOutcome>({ kind: "checked", misspellings: [] }),
    );
    const checker = desktopProseChecker(builtin, spellcheck, () => on);
    const request = { text: "hi", spans: [], dictionary: [], dialect: "" };

    await checker.check(request);
    expect(spellcheck).not.toHaveBeenCalled();

    on = true;
    await checker.check(request);
    expect(spellcheck).toHaveBeenCalledTimes(1);
  });

  it("defaults to on when no getter is supplied", async () => {
    const spellcheck = vi.fn(() =>
      Promise.resolve<SpellcheckOutcome>({ kind: "checked", misspellings: [] }),
    );
    await desktopProseChecker(builtin, spellcheck).check({
      text: "hi",
      spans: [],
      dictionary: [],
      dialect: "",
    });
    expect(spellcheck).toHaveBeenCalledTimes(1);
  });
});
