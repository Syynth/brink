/**
 * The updater's host command ids, checked against the REAL validator.
 *
 * Desktop 0.4.0 shipped these unnamespaced (`update.install`, …). Host
 * commands must be `host.<vendor>.<name>` (studio-shell spec §8.1), so
 * `CommandRegistry.registerHost` threw inside `mountStudio` and opening ANY
 * project failed — the app fell back to the landing screen showing the
 * validator's error. It reached a release because nothing here ever ran the
 * validator: the tests restated the same wrong literals they were meant to
 * check, so they agreed with the bug. Importing the real values WAS possible
 * — `autosave-reopen.test.ts` pulls `AUTOSAVE_MS` out of `main.tsx` with a
 * dynamic import after its mocks — merely awkward enough that nobody did.
 *
 * The point of this file is that it imports the SHIPPING ids and hands them
 * to the SHIPPING registry. A literal in here would recreate the hole.
 */

import { describe, expect, it } from "vitest";
import { CommandRegistry } from "@brink/studio-shell";

import {
  UPDATE_CHECK_COMMAND,
  UPDATE_COMMAND_IDS,
  UPDATE_INSTALL_COMMAND,
  UPDATE_LATER_COMMAND,
} from "../update-commands.js";

describe("updater host command ids", () => {
  it("every id registers as a host command", () => {
    const commands = new CommandRegistry();
    for (const id of UPDATE_COMMAND_IDS) {
      expect(() =>
        commands.registerHost({ id, title: `test ${id}`, run: () => {} }),
      ).not.toThrow();
    }
  });

  it("the registry rejects the unnamespaced ids 0.4.0 shipped", () => {
    // Pins the failure mode itself, so the guard above cannot be weakened
    // into something that would pass for the broken ids too.
    const commands = new CommandRegistry();
    expect(() =>
      commands.registerHost({ id: "update.install", title: "x", run: () => {} }),
    ).toThrow(/must be namespaced/);
  });

  it("the ids are distinct", () => {
    expect(new Set(UPDATE_COMMAND_IDS).size).toBe(UPDATE_COMMAND_IDS.length);
    expect(UPDATE_COMMAND_IDS).toEqual([
      UPDATE_INSTALL_COMMAND,
      UPDATE_LATER_COMMAND,
      UPDATE_CHECK_COMMAND,
    ]);
  });
});
