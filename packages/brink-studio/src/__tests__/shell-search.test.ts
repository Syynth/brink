/**
 * Search tool window tests (issue #94, spec §4).
 *
 * Covers: the search.focus command (registration, open-without-toggle
 * semantics, focus sequence), the generated Mod-N numbering with Search
 * registered last (Binder stays Mod-1, Search gets Mod-6),
 * ensureToolWindowOpen across tiers, and the store slice — live search
 * over a fake project session, regex validation errors, per-match replace
 * and replace-all through the binder-style updateFile/invalidateFile/
 * triggerCompile path, and the stale-result guards.
 */

import { describe, expect, it, vi } from "vitest";
import {
  CommandRegistry,
  ToolWindowRegistry,
  createShellLayoutStore,
  ensureToolWindowOpen,
  isToolWindowOpen,
  registerViewToggleCommands,
  viewToggleCommandId,
  type ToolWindowDescriptor,
} from "@brink/studio-shell";
import { createStudioStore, type StoreNotification } from "@brink/studio-store";
import {
  SEARCH_FOCUS_COMMAND_ID,
  SEARCH_TOOL_WINDOW_ID,
  registerSearchFocusCommand,
} from "@brink/studio-ui";

// ── Harness ─────────────────────────────────────────────────────────

function descriptor(id: string, over: Partial<ToolWindowDescriptor> = {}): ToolWindowDescriptor {
  return {
    id,
    title: id,
    icon: null,
    defaultPlacement: { dock: "left", section: "start" },
    defaultOpen: false,
    component: () => null,
    ...over,
  };
}

/** The main.tsx registration order: binder … output, then search (#94). */
function registerMainToolWindows(registry: ToolWindowRegistry): void {
  registry.register(descriptor("binder", { defaultOpen: true }));
  registry.register(descriptor("state", { defaultPlacement: { dock: "right", section: "start" } }));
  registry.register(descriptor("program", { defaultPlacement: { dock: "bottom", section: "start" } }));
  registry.register(descriptor("problems", { defaultPlacement: { dock: "bottom", section: "start" } }));
  registry.register(descriptor("output", { defaultPlacement: { dock: "bottom", section: "end" } }));
  registry.register(descriptor(SEARCH_TOOL_WINDOW_ID));
}

interface FakeProject {
  project: { getSession(): unknown; applyEdit(path: string, source: string): boolean };
  sources: Map<string, string>;
  updates: Array<{ path: string; source: string }>;
}

function fakeProject(files: Record<string, string>): FakeProject {
  const sources = new Map(Object.entries(files));
  const updates: Array<{ path: string; source: string }> = [];
  const session = {
    listFiles: () => [...sources.keys()].map((path) => ({ path })),
    getFileSource: (path: string) => sources.get(path) ?? null,
    updateFile: (path: string, source: string) => {
      sources.set(path, source);
      updates.push({ path, source });
    },
  };
  return {
    project: {
      getSession: () => session,
      // The shared apply-edits seam (#137) the replace paths route through.
      // Real `applyEdit` (issue #2306) reports whether the write applied —
      // this fake has no read-only concept, so it always applies.
      applyEdit: (path: string, source: string) => {
        session.updateFile(path, source);
        return true;
      },
    },
    sources,
    updates,
  };
}

function searchStore(files: Record<string, string>) {
  const store = createStudioStore();
  const fake = fakeProject(files);
  const documents = { invalidateFile: vi.fn(), triggerCompile: vi.fn() };
  const notifications: StoreNotification[] = [];
  store.setState({
    _project: fake.project as never,
    _documents: documents as never,
    _notify: (n) => notifications.push(n),
  });
  return { store, fake, documents, notifications };
}

// ── search.focus command ────────────────────────────────────────────

describe("search.focus", () => {
  function harness() {
    const commands = new CommandRegistry();
    const layout = createShellLayoutStore();
    const registry = new ToolWindowRegistry();
    registerMainToolWindows(registry);
    layout.getState().syncFromRegistry(registry.list());
    const store = createStudioStore();
    registerSearchFocusCommand(commands, layout, store);
    return { commands, layout, store };
  }

  it("registers palette-discoverable with the Mod-Shift-F binding", () => {
    const { commands } = harness();
    const command = commands.get(SEARCH_FOCUS_COMMAND_ID);
    expect(command?.title).toBe("Search: Find in Files");
    expect(command?.keybinding).toBe("Mod-Shift-F");
  });

  it("opens the closed tool window and bumps the focus sequence", () => {
    const { commands, layout, store } = harness();
    expect(isToolWindowOpen(layout.getState(), SEARCH_TOOL_WINDOW_ID)).toBe(false);

    expect(commands.dispatch(SEARCH_FOCUS_COMMAND_ID)).toBe(true);

    expect(isToolWindowOpen(layout.getState(), SEARCH_TOOL_WINDOW_ID)).toBe(true);
    expect(store.getState().searchFocusSeq).toBe(1);
    // Search displaces the binder in the shared left/start section.
    expect(isToolWindowOpen(layout.getState(), "binder")).toBe(false);
  });

  it("never closes an already-open window (not a toggle)", () => {
    const { commands, layout, store } = harness();
    commands.dispatch(SEARCH_FOCUS_COMMAND_ID);
    commands.dispatch(SEARCH_FOCUS_COMMAND_ID);
    expect(isToolWindowOpen(layout.getState(), SEARCH_TOOL_WINDOW_ID)).toBe(true);
    expect(store.getState().searchFocusSeq).toBe(2);
  });
});

// ── Generated Mod-N numbering ───────────────────────────────────────

describe("tool-window numbering with search registered last", () => {
  it("keeps Binder at Mod-1 and assigns Search Mod-6", () => {
    const commands = new CommandRegistry();
    const layout = createShellLayoutStore();
    const registry = new ToolWindowRegistry();
    registerMainToolWindows(registry);
    layout.getState().syncFromRegistry(registry.list());
    registerViewToggleCommands(commands, registry.list(), layout);

    const bindings = registry
      .list()
      .map((d) => commands.get(viewToggleCommandId(d.id))?.keybinding);
    expect(bindings).toEqual(["Mod-1", "Mod-2", "Mod-3", "Mod-4", "Mod-5", "Mod-6"]);
    expect(commands.get(viewToggleCommandId(SEARCH_TOOL_WINDOW_ID))?.keybinding).toBe(
      "Mod-6",
    );
  });
});

// ── ensureToolWindowOpen ────────────────────────────────────────────

describe("ensureToolWindowOpen", () => {
  function layoutWith(tier: "wide" | "medium" | "narrow") {
    const layout = createShellLayoutStore();
    const registry = new ToolWindowRegistry();
    registerMainToolWindows(registry);
    layout.getState().syncFromRegistry(registry.list());
    layout.getState().setTier(tier);
    return layout;
  }

  it("is a no-op for unknown ids", () => {
    const layout = layoutWith("wide");
    const before = layout.getState();
    ensureToolWindowOpen(layout, "nope");
    expect(layout.getState()).toBe(before);
  });

  it("opens a closed window; leaves an open one open", () => {
    const layout = layoutWith("wide");
    ensureToolWindowOpen(layout, SEARCH_TOOL_WINDOW_ID);
    expect(isToolWindowOpen(layout.getState(), SEARCH_TOOL_WINDOW_ID)).toBe(true);
    ensureToolWindowOpen(layout, SEARCH_TOOL_WINDOW_ID);
    expect(isToolWindowOpen(layout.getState(), SEARCH_TOOL_WINDOW_ID)).toBe(true);
  });

  it("medium tier: surfaces the hidden drawer for an open side window", () => {
    const layout = layoutWith("medium");
    ensureToolWindowOpen(layout, SEARCH_TOOL_WINDOW_ID);
    expect(layout.getState().drawers.left).toBe(true);
    // Hide the drawer (scrim click) — ensure resurfaces it without closing.
    layout.getState().closeDrawers();
    ensureToolWindowOpen(layout, SEARCH_TOOL_WINDOW_ID);
    expect(isToolWindowOpen(layout.getState(), SEARCH_TOOL_WINDOW_ID)).toBe(true);
    expect(layout.getState().drawers.left).toBe(true);
  });

  it("narrow tier: surfaces the overlay for an open non-left window", () => {
    const layout = layoutWith("narrow");
    ensureToolWindowOpen(layout, "state");
    expect(layout.getState().narrowView).toBe("state");
    layout.getState().setNarrowView(null);
    ensureToolWindowOpen(layout, "state");
    expect(layout.getState().narrowView).toBe("state");
  });
});

// ── Search slice ────────────────────────────────────────────────────

describe("search slice", () => {
  const FILES = {
    "main.ink": "INCLUDE story.ink\n\n-> intro\n",
    "story.ink": "=== intro ===\nThe intro begins.\n-> END\n",
  };

  it("runSearch groups matches by file in sorted-path order", () => {
    const { store } = searchStore(FILES);
    store.getState().setSearchQuery("intro");
    store.getState().runSearch();

    const results = store.getState().searchResults;
    expect(results).not.toBeNull();
    expect(results!.files.map((f) => f.path)).toEqual(["main.ink", "story.ink"]);
    expect(results!.totalMatches).toBe(3);
    expect(store.getState().searchError).toBeNull();
  });

  it("empty query clears results; invalid regex sets the inline error", () => {
    const { store } = searchStore(FILES);
    store.getState().setSearchQuery("intro");
    store.getState().runSearch();
    expect(store.getState().searchResults).not.toBeNull();

    store.getState().setSearchQuery("");
    store.getState().runSearch();
    expect(store.getState().searchResults).toBeNull();

    store.getState().setSearchQuery("(");
    store.getState().toggleSearchOption("regex");
    store.getState().runSearch();
    expect(store.getState().searchResults).toBeNull();
    expect(store.getState().searchError).toContain("Invalid regex");
  });

  it("options change results: case-sensitive narrows matches", () => {
    const { store } = searchStore(FILES);
    store.getState().setSearchQuery("the");
    store.getState().runSearch();
    expect(store.getState().searchResults!.totalMatches).toBe(1); // "The"

    store.getState().toggleSearchOption("caseSensitive");
    store.getState().runSearch();
    expect(store.getState().searchResults!.totalMatches).toBe(0);
  });

  it("acceptSearchMatch applies one replacement and keeps the snapshot (frozen)", () => {
    const { store, fake, documents } = searchStore(FILES);
    store.getState().setSearchQuery("begins");
    store.getState().setSearchReplace("starts");
    store.getState().runSearch();

    const id = store.getState().searchResults!.files[0]!.matches[0]!.id;
    store.getState().acceptSearchMatch(id);

    expect(fake.sources.get("story.ink")).toContain("The intro starts.");
    expect(documents.invalidateFile).toHaveBeenCalledWith("story.ink");
    expect(documents.triggerCompile).toHaveBeenCalledTimes(1);
    // Frozen snapshot: the row stays, receipted — never re-searched away.
    expect(store.getState().searchResults!.totalMatches).toBe(1);
    expect(store.getState().searchReplaced).toEqual({ [id]: true });
  });

  it("acceptSearchMatch on a live-stale span applies nothing", () => {
    const { store, fake, documents } = searchStore(FILES);
    store.getState().setSearchQuery("begins");
    store.getState().setSearchReplace("starts");
    store.getState().runSearch();
    const id = store.getState().searchResults!.files[0]!.matches[0]!.id;

    // The file changes underneath the snapshot span.
    fake.sources.set("story.ink", "=== intro ===\nRewritten.\n-> END\n");
    store.getState().acceptSearchMatch(id);

    expect(fake.updates).toHaveLength(0);
    expect(documents.invalidateFile).not.toHaveBeenCalled();
    expect(store.getState().searchReplaced).toEqual({});
    // The row itself survives (frozen snapshot; the compile-seam remap is
    // what flags it, in production).
    expect(store.getState().searchResults!.totalMatches).toBe(1);
  });

  it("acceptAllSearchMatches applies every pending match and notifies", () => {
    const { store, fake, documents, notifications } = searchStore(FILES);
    store.getState().setSearchQuery("intro");
    store.getState().setSearchReplace("prologue");
    store.getState().runSearch();

    store.getState().acceptAllSearchMatches();

    expect(fake.sources.get("main.ink")).toContain("-> prologue");
    expect(fake.sources.get("story.ink")).toContain("=== prologue ===");
    expect(fake.sources.get("story.ink")).toContain("The prologue begins.");
    expect(documents.invalidateFile).toHaveBeenCalledWith("main.ink");
    expect(documents.invalidateFile).toHaveBeenCalledWith("story.ink");
    expect(documents.triggerCompile).toHaveBeenCalledTimes(1);
    expect(notifications).toEqual([
      {
        severity: "info",
        source: "search",
        message: "Replaced 3 matches in 2 files",
      },
    ]);
    // Every row keeps its receipt; the set is never re-searched away.
    expect(Object.keys(store.getState().searchReplaced)).toHaveLength(3);
    expect(store.getState().searchResults!.totalMatches).toBe(3);
  });

  it("acceptAllSearchMatches excludes skipped and live-stale matches per-match", () => {
    const { store, fake, notifications } = searchStore(FILES);
    store.getState().setSearchQuery("intro");
    store.getState().setSearchReplace("prologue");
    store.getState().runSearch();
    const snapshot = store.getState().searchResults!;
    const mainId = snapshot.files[0]!.matches[0]!.id;

    // Skip main.ink's match; stale story.ink underneath (both its matches
    // fail the live guard — excluded per-match, never a global abort).
    store.getState().toggleSearchSkip(mainId);
    fake.sources.set("story.ink", "=== other ===\nRewritten.\n-> END\n");
    store.getState().acceptAllSearchMatches();

    expect(fake.sources.get("main.ink")).toContain("-> intro");
    expect(store.getState().searchReplaced).toEqual({});
    expect(notifications).toEqual([
      expect.objectContaining({
        severity: "info",
        message: "Replaced 0 matches in 0 files",
      }),
    ]);

    // Undo the skip: main.ink's match is pending again and applies.
    store.getState().toggleSearchSkip(mainId);
    store.getState().acceptAllSearchMatches();
    expect(fake.sources.get("main.ink")).toContain("-> prologue");
    expect(store.getState().searchReplaced).toEqual({ [mainId]: true });
  });

  it("accepting uses the snapshot's frozen query even after the input changed", () => {
    const { store, fake } = searchStore(FILES);
    store.getState().setSearchQuery("begins");
    store.getState().setSearchReplace("starts");
    store.getState().runSearch();
    const id = store.getState().searchResults!.files[0]!.matches[0]!.id;

    // Typing a different query without re-running must not change what
    // Accept applies (the snapshot owns its origin).
    store.getState().setSearchQuery("bogus");
    store.getState().acceptSearchMatch(id);
    expect(fake.sources.get("story.ink")).toContain("The intro starts.");
  });

  it("regex accept-all expands capture groups", () => {
    const { store, fake } = searchStore({ "a.ink": "VAR gold = 12\n" });
    store.getState().toggleSearchOption("regex");
    store.getState().setSearchQuery("VAR (\\w+)");
    store.getState().setSearchReplace("CONST $1");
    store.getState().runSearch();
    store.getState().acceptAllSearchMatches();
    expect(fake.sources.get("a.ink")).toBe("CONST gold = 12\n");
  });
});
