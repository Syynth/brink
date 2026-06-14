/**
 * Capability-gated drive commands (docs/live-inspector-spec.md §4, #180).
 *
 * Each `story.*` command ANDs the bound provider's advertised capabilities
 * into its `when` predicate. An observe-only provider (empty capability set)
 * therefore fails every drive command's `when`, so they vanish from the
 * palette / strips / view headers with no per-view branching — the views
 * render fully populated but read-only. The local provider advertises the
 * full set, so studio behavior is unchanged (verified in story-session.test).
 */

import { describe, it, expect } from "vitest";
import {
  createStudioStore,
  type SessionCapability,
  type SessionProvider,
  type SessionSnapshot,
  type SessionStatus,
} from "@brink/studio-store";
import { CommandRegistry } from "@brink/studio-shell";
import { registerStoryCommands } from "../story-commands.js";

/** A minimal non-local provider advertising an arbitrary capability set. */
function fakeProvider(caps: SessionCapability[], status: SessionStatus): SessionProvider {
  const snapshot: SessionSnapshot = {
    status,
    transcript: [],
    choices: [],
    debugState: null,
    programChecksum: null,
    programModel: null,
    programInkt: null,
  };
  return {
    kind: "remote",
    capabilities: new Set(caps),
    getSnapshot: () => snapshot,
    subscribe: () => () => {},
    dispose: () => {},
  };
}

function setup() {
  const store = createStudioStore();
  const commands = new CommandRegistry();
  registerStoryCommands(commands, store);
  // A program exists, so only capabilities (not a missing program) can gate.
  store.setState({ storyBytes: new Uint8Array([1]) });
  return { store, commands };
}

describe("capability-gated commands (#180)", () => {
  it("defaults to the full local set before any provider binds", () => {
    const { store, commands } = setup();
    // No provider bound; capabilities default to the local full set.
    expect(commands.isEnabled("story.start")).toBe(true); // status none + bytes

    store.setState({ sessionStatus: "awaiting-choice" });
    expect(commands.isEnabled("story.choose")).toBe(true);
    expect(commands.isEnabled("story.stop")).toBe(true);
  });

  it("an observe-only provider disables every drive command", () => {
    const { store, commands } = setup();
    store.getState()._bindProvider(fakeProvider([], "awaiting-choice"));

    // Status alone would enable choose/stop/restart here, but no capability
    // is advertised — so every drive verb fails its `when`.
    for (const id of ["story.restart", "story.stop", "story.choose", "story.continue"]) {
      expect(commands.isEnabled(id), id).toBe(false);
    }

    // A `-> DONE` turn boundary would normally allow continue — still gated off.
    store.setState({ sessionStatus: "done" });
    expect(commands.isEnabled("story.continue")).toBe(false);
  });

  it("an interactive remote provider enables only what it advertises", () => {
    const { store, commands } = setup();
    // The game lets the studio choose, but drives continue/stop itself.
    store.getState()._bindProvider(fakeProvider(["choose"], "awaiting-choice"));

    expect(commands.isEnabled("story.choose")).toBe(true);
    expect(commands.isEnabled("story.stop")).toBe(false);
    expect(commands.isEnabled("story.restart")).toBe(false);

    store.setState({ sessionStatus: "running" });
    expect(commands.isEnabled("story.continue")).toBe(false); // no "continue" cap
  });

  it("restores the full set when the provider is disposed", () => {
    const { store, commands } = setup();
    store.getState()._bindProvider(fakeProvider([], "awaiting-choice"));
    expect(commands.isEnabled("story.choose")).toBe(false);

    store.getState().disposeSession();
    // Back to the default local capabilities; status returns to none.
    store.setState({ sessionStatus: "awaiting-choice" });
    expect(commands.isEnabled("story.choose")).toBe(true);
  });
});
