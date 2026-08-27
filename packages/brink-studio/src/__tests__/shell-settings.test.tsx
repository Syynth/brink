/**
 * Settings document tests (issue #93, spec §4 / §7.8).
 *
 * Covers: the settings.open command opening/focusing the singleton tab,
 * strict override-JSON validation (parseKeymapOverridesText), the
 * KeymapOverridesService persistence + change event, the ShellProvider
 * rebuilding the live keymap when overrides change, the three sections of
 * the rendered document (theme reflects/drives ThemeService, keymap apply
 * validates + rebuilds, diagnostics flag dispatches the store action and
 * persists), and the diagnostics bootstrap restore (initialize applies a
 * pre-seeded level to the wasm session before the first compile).
 */

import { describe, expect, it, vi, afterEach } from "vitest";
import { act, createElement, type ReactNode } from "react";
import { createRoot, type Root } from "react-dom/client";
import {
  BUILTIN_THEMES,
  CommandRegistry,
  KEYMAP_STORAGE_KEY,
  KeymapOverridesService,
  ShellProvider,
  ThemeService,
  createEditorGroupsStore,
  createShellLayoutStore,
  documentKey,
  findTab,
  parseKeymapOverridesText,
  useShell,
  type Keymap,
} from "@brink/studio-shell";
import { createStudioStore, type StudioStore } from "@brink/studio-store";
import {
  DEFAULT_SETTINGS_SECTION,
  DIAGNOSTICS_STORAGE_KEY,
  OPEN_SETTINGS_COMMAND_ID,
  SETTINGS_TYPE_ID,
  SettingsDocument,
  StoreProvider,
  loadDiagnosticsSettings,
  registerSettingsCommand,
  saveDiagnosticsSettings,
  settingsRef,
} from "@brink/studio-ui";

(globalThis as Record<string, unknown>).IS_REACT_ACT_ENVIRONMENT = true;

function memoryStorage(initial: Record<string, string> = {}) {
  const map = new Map(Object.entries(initial));
  return {
    getItem: (k: string) => map.get(k) ?? null,
    setItem: (k: string, v: string) => void map.set(k, v),
    dump: (k: string) => map.get(k) ?? null,
  };
}

// ── Command wiring ──────────────────────────────────────────────────

describe("settings.open", () => {
  it("registers palette-discoverable with the Mod-, binding", () => {
    const commands = new CommandRegistry();
    registerSettingsCommand(commands, () => {});
    const command = commands.get(OPEN_SETTINGS_COMMAND_ID);
    expect(command?.title).toBe("Settings: Open");
    expect(command?.keybinding).toBe("Mod-,");
  });

  it("opens the modal rather than taking over the editor area", () => {
    // Ruled 2026-08-27 (#3174). Settings was a takeover, which was right
    // while it was small — a tab is unreachable from a view without tabs.
    // The brink.toml interface made it a surface you consult, and taking
    // over the editor cost you the file you were reading.
    const commands = new CommandRegistry();
    const opened: (string | null)[] = [];
    registerSettingsCommand(commands, (section) => opened.push(section));

    expect(commands.dispatch(OPEN_SETTINGS_COMMAND_ID)).toBe(true);

    // The command names a SECTION, never null — null is the closed state,
    // so passing it would open Settings and immediately close it (which is
    // exactly what the first version of this did). `DEFAULT_SETTINGS_SECTION`
    // is where a door with no preference of its own lands.
    expect(opened).toEqual([DEFAULT_SETTINGS_SECTION]);
  });

  it("leaves the editor area alone", () => {
    // The point of the change: whatever you were reading is still there
    // behind the modal.
    const commands = new CommandRegistry();
    const layout = createShellLayoutStore();
    registerSettingsCommand(commands, () => {});
    commands.dispatch(OPEN_SETTINGS_COMMAND_ID);
    expect(layout.getState().takeover).toBeNull();
  });
});

// ── Override JSON validation ────────────────────────────────────────

describe("parseKeymapOverridesText", () => {
  it("accepts string, array, and null values", () => {
    const result = parseKeymapOverridesText(
      JSON.stringify({
        "a.one": "Mod-K",
        "a.two": ["Mod-J", "F2"],
        "a.three": null,
      }),
    );
    expect(result).toEqual({
      ok: true,
      overrides: { "a.one": "Mod-K", "a.two": ["Mod-J", "F2"], "a.three": null },
    });
  });

  it("rejects malformed JSON with the parse error", () => {
    const result = parseKeymapOverridesText("{not json");
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toContain("Not valid JSON");
  });

  it("rejects non-object top levels", () => {
    for (const text of ['["array"]', '"string"', "3", "null"]) {
      expect(parseKeymapOverridesText(text).ok).toBe(false);
    }
  });

  it("rejects non-string values, naming the command id", () => {
    const result = parseKeymapOverridesText('{"a.one": 7}');
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toContain('"a.one"');
  });

  it("rejects unparsable keybindings, naming the binding", () => {
    const result = parseKeymapOverridesText('{"a.one": "Mod-"}');
    expect(result.ok).toBe(false);
    if (!result.ok) expect(result.error).toContain('"Mod-"');
  });
});

// ── KeymapOverridesService ──────────────────────────────────────────

describe("KeymapOverridesService", () => {
  it("loads the persisted overrides at construction (lenient)", () => {
    const storage = memoryStorage({
      [KEYMAP_STORAGE_KEY]: JSON.stringify({ "a.one": "Mod-K", bad: 7 }),
    });
    const service = new KeymapOverridesService(storage);
    expect(service.current).toEqual({ "a.one": "Mod-K" });
  });

  it("set() replaces, persists under the versioned key, and notifies", () => {
    const storage = memoryStorage();
    const service = new KeymapOverridesService(storage);
    const listener = vi.fn();
    service.onDidChange(listener);

    service.set({ "a.one": "Mod-J" });

    expect(service.current).toEqual({ "a.one": "Mod-J" });
    expect(storage.dump(KEYMAP_STORAGE_KEY)).toBe(
      JSON.stringify({ "a.one": "Mod-J" }),
    );
    expect(listener).toHaveBeenCalledTimes(1);
  });

  it("degrades to in-session overrides when storage throws", () => {
    const denied = {
      getItem: () => {
        throw new Error("denied");
      },
      setItem: () => {
        throw new Error("denied");
      },
    };
    const service = new KeymapOverridesService(denied);
    expect(service.current).toEqual({});
    service.set({ "a.one": "Mod-J" });
    expect(service.current).toEqual({ "a.one": "Mod-J" });
  });

  it("unsubscribe stops notifications", () => {
    const service = new KeymapOverridesService(memoryStorage());
    const listener = vi.fn();
    const unsubscribe = service.onDidChange(listener);
    unsubscribe();
    service.set({});
    expect(listener).not.toHaveBeenCalled();
  });
});

// ── Rendered document ───────────────────────────────────────────────

let container: HTMLDivElement | null = null;
let root: Root | null = null;

afterEach(() => {
  if (root) act(() => root!.unmount());
  container?.remove();
  container = null;
  root = null;
  window.localStorage.removeItem(DIAGNOSTICS_STORAGE_KEY);
});

interface Harness {
  commands: CommandRegistry;
  themes: ThemeService;
  overrides: KeymapOverridesService;
  store: StudioStore;
  /** The provider's current keymap, captured by a probe child. */
  keymap: () => Keymap;
}

function KeymapProbe({ capture }: { capture: (keymap: Keymap) => void }) {
  capture(useShell().keymap);
  return null;
}

function renderSettings(): Harness {
  const commands = new CommandRegistry();
  const themes = new ThemeService(BUILTIN_THEMES, memoryStorage());
  const overrides = new KeymapOverridesService(memoryStorage());
  const store = createStudioStore();
  let latestKeymap: Keymap | null = null;

  container = document.createElement("div");
  document.body.appendChild(container);
  root = createRoot(container);

  const doc = createElement(SettingsDocument, {
    doc: settingsRef(),
    groupId: "g1",
    active: true,
  });
  const probe = createElement(KeymapProbe, {
    capture: (keymap: Keymap) => {
      latestKeymap = keymap;
    },
  });
  act(() => {
    root!.render(
      createElement(
        ShellProvider,
        { commands, themes, keymapOverrides: overrides, isMac: true } as never,
        createElement(StoreProvider, { store } as never, doc as ReactNode, probe),
      ),
    );
  });

  return { commands, themes, overrides, store, keymap: () => latestKeymap! };
}

function setTextareaValue(textarea: HTMLTextAreaElement, value: string): void {
  const setter = Object.getOwnPropertyDescriptor(
    HTMLTextAreaElement.prototype,
    "value",
  )!.set!;
  act(() => {
    setter.call(textarea, value);
    textarea.dispatchEvent(new Event("input", { bubbles: true }));
  });
}

describe("SettingsDocument — theme section", () => {
  it("reflects the current theme and drives ThemeService.select", () => {
    const h = renderSettings();
    const radios = [
      ...container!.querySelectorAll<HTMLInputElement>(
        "[aria-label='Theme'] .settings-radio input",
      ),
    ];
    expect(radios.map((r) => r.value)).toEqual(["mocha", "latte", "manuscript", "inky", "inky-dark"]);
    expect(radios[0].checked).toBe(true);

    act(() => radios[1].click());
    expect(h.themes.current).toBe("latte");
    expect(radios[1].checked).toBe(true);
    expect(radios[0].checked).toBe(false);
  });

  it("reflects external changes (e.g. the palette theme command)", () => {
    const h = renderSettings();
    act(() => void h.themes.select("latte"));
    const radios = [
      ...container!.querySelectorAll<HTMLInputElement>(
        "[aria-label='Theme'] .settings-radio input",
      ),
    ];
    expect(radios[1].checked).toBe(true);
  });
});

describe("SettingsDocument — keymap section", () => {
  it("rejects invalid JSON: inline error, nothing saved", () => {
    const h = renderSettings();
    const textarea = container!.querySelector<HTMLTextAreaElement>(".settings-json")!;
    setTextareaValue(textarea, "{not json");
    act(() => container!.querySelector<HTMLButtonElement>(".settings-apply")!.click());

    expect(container!.querySelector(".settings-error")?.textContent).toContain(
      "Not valid JSON",
    );
    expect(h.overrides.current).toEqual({});
  });

  it("rejects bad bindings: inline error, nothing saved", () => {
    const h = renderSettings();
    const textarea = container!.querySelector<HTMLTextAreaElement>(".settings-json")!;
    setTextareaValue(textarea, '{"a.one": "Mod-"}');
    act(() => container!.querySelector<HTMLButtonElement>(".settings-apply")!.click());

    expect(container!.querySelector(".settings-error")).not.toBeNull();
    expect(h.overrides.current).toEqual({});
  });

  it("applies valid JSON: service updated, error cleared, live keymap rebuilt", () => {
    const h = renderSettings();
    act(() => {
      h.commands.register({ id: "a.cmd", title: "A", keybinding: "Mod-K", run: () => {} });
    });
    expect(h.keymap().bindingFor("a.cmd")?.key).toBe("k");

    const textarea = container!.querySelector<HTMLTextAreaElement>(".settings-json")!;
    setTextareaValue(textarea, '{"a.cmd": "Mod-J"}');
    act(() => container!.querySelector<HTMLButtonElement>(".settings-apply")!.click());

    expect(container!.querySelector(".settings-error")).toBeNull();
    expect(h.overrides.current).toEqual({ "a.cmd": "Mod-J" });
    // The ShellProvider subscription rebuilt the resolution table live.
    expect(h.keymap().bindingFor("a.cmd")?.key).toBe("j");
    expect(
      h.keymap().resolveChord({ key: "k", mod: true, shift: false, alt: false }),
    ).toBeUndefined();
  });
});

describe("SettingsDocument — external functions section", () => {
  it("dispatches the store action and persists under the versioned key", () => {
    const h = renderSettings();
    // Scope to the External functions section — the Editor section also
    // renders a `.settings-select` (the argument-form glyph) and renders
    // first, so a bare `.settings-select` query would grab that one instead.
    //
    // It was titled "Diagnostics" until #3148 added the [lints] section,
    // which is what "Diagnostics" now means. This one is about
    // external-function checking specifically, and unlike the lints it is a
    // studio preference rather than a brink.toml setting.
    const diagSection = [...container!.querySelectorAll(".settings-section")].find(
      (s) => s.querySelector(".settings-section-title")?.textContent === "External functions",
    )!;
    const select = diagSection.querySelector<HTMLSelectElement>(".settings-select")!;
    expect(select.value).toBe("error");

    act(() => {
      select.value = "off";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    });

    expect(h.store.getState().externalCheck).toBe("off");
    expect(loadDiagnosticsSettings(window.localStorage)).toEqual({
      externalCheck: "off",
    });
  });
});

// ── Diagnostics persistence + bootstrap apply ───────────────────────

describe("diagnostics settings persistence", () => {
  it("round-trips through the versioned key and defaults on garbage", () => {
    const storage = memoryStorage();
    saveDiagnosticsSettings(storage, { externalCheck: "off" });
    expect(loadDiagnosticsSettings(storage)).toEqual({ externalCheck: "off" });

    expect(loadDiagnosticsSettings(memoryStorage())).toEqual({
      externalCheck: "error",
    });
    expect(
      loadDiagnosticsSettings(
        memoryStorage({ [DIAGNOSTICS_STORAGE_KEY]: "not json" }),
      ),
    ).toEqual({ externalCheck: "error" });
    expect(
      loadDiagnosticsSettings(
        memoryStorage({ [DIAGNOSTICS_STORAGE_KEY]: '{"externalCheck":"loud"}' }),
      ),
    ).toEqual({ externalCheck: "error" });
  });

  it("initialize applies a pre-seeded level to the session before compiling", () => {
    const store = createStudioStore();
    const setExternalCheck = vi.fn();
    const triggerCompile = vi.fn(() => {
      // The wasm session must already carry the restored level when the
      // first compile runs.
      expect(setExternalCheck).toHaveBeenCalledWith("off");
    });
    const project = { getSession: () => ({ setExternalCheck }) } as never;
    const documents = { triggerCompile } as never;

    // Bootstrap restore happens before initialize: only seeds the state.
    store.getState().setExternalCheck("off");
    expect(setExternalCheck).not.toHaveBeenCalled();

    store.getState().initialize(project, documents);
    expect(setExternalCheck).toHaveBeenCalledExactlyOnceWith("off");
    expect(triggerCompile).toHaveBeenCalledTimes(1);
  });

  it("post-bind changes apply to the session and recompile; same level no-ops", () => {
    const store = createStudioStore();
    const setExternalCheck = vi.fn();
    const triggerCompile = vi.fn();
    const project = { getSession: () => ({ setExternalCheck }) } as never;
    const documents = { triggerCompile } as never;
    store.getState().initialize(project, documents);
    triggerCompile.mockClear();

    store.getState().setExternalCheck("off");
    expect(setExternalCheck).toHaveBeenLastCalledWith("off");
    expect(triggerCompile).toHaveBeenCalledTimes(1);

    // Same level again — no extra wasm call, no recompile.
    store.getState().setExternalCheck("off");
    expect(setExternalCheck).toHaveBeenCalledTimes(1);
    expect(triggerCompile).toHaveBeenCalledTimes(1);
  });
});
