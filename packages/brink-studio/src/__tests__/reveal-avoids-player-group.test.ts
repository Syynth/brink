/**
 * Opening an editor document must not bury the Player (maintainer,
 * 2026-09-11).
 *
 * `openDocument` defaults to the focused group, and a click inside the Player
 * focuses the Player's group first — the group `<section>`'s `onFocus` maps to
 * `focusin`, which bubbles up from the clicked control. So the Player's own
 * "open in the editor" button opened the file directly over the Player, in the
 * one case where you want both on screen: the transcript and the line it came
 * from. Same for a click in the Binder, Problems or Search while the Player
 * happens to hold focus.
 *
 * `groupForEditorOpen` (`mount.tsx`) is the decision, pure over the group
 * list, and `openEditorDocument` is the composition that applies it — the
 * exact functions `setDocumentOpener`'s production branches call, so this
 * tests the shipped path rather than a re-implementation of it. Booting the
 * studio is not needed (and not possible here: `mountStudio` wants wasm).
 */

import { describe, it, expect } from "vitest";
import {
  createEditorGroupsStore,
  documentKey,
  findTab,
  type EditorGroup,
} from "@brink/studio-shell";
import { inkFileRef, playerRef } from "@brink/studio-ui";
import { groupForEditorOpen, openEditorDocument } from "../mount.js";

const PLAYER_KEY = documentKey(playerRef());
const fileRef = (path: string) => inkFileRef({ kind: "file", path });
const fileKey = (path: string) => documentKey(fileRef(path));

const playerTab = { ref: playerRef(), pinned: true };
const fileTab = (path: string, pinned = true) => ({ ref: fileRef(path), pinned });

/** The default layout (spec §4's Inky two-up): entry file left, Player right,
 *  with the Player focused because the author just clicked inside it. */
function twoUp(): EditorGroup[] {
  return [
    { id: "group-1", tabs: [fileTab("story.ink")], activeKey: fileKey("story.ink") },
    { id: "group-2", tabs: [playerTab], activeKey: PLAYER_KEY },
  ];
}

describe("groupForEditorOpen", () => {
  it("sends a new file to the other split when the Player holds the focused one", () => {
    expect(groupForEditorOpen(twoUp(), "group-2", fileKey("scene2.ink"))).toBe("group-1");
  });

  it("leaves the policy alone when the editor holds focus", () => {
    // Nothing is being covered: the ordinary focused-group open is correct.
    expect(groupForEditorOpen(twoUp(), "group-1", fileKey("scene2.ink"))).toBeUndefined();
  });

  it("leaves an already-open document to the reveal policy", () => {
    // §7.8: an explicit group target SKIPS the any-group reveal and is how a
    // tab gets deliberately duplicated. Redirecting here would mint a second
    // tab for a file that is already open — worse than the bug being fixed.
    const groups = twoUp();
    expect(groupForEditorOpen(groups, "group-2", fileKey("story.ink"))).toBeUndefined();
  });

  it("does not redirect a re-open of the Player itself", () => {
    expect(groupForEditorOpen(twoUp(), "group-2", PLAYER_KEY)).toBeUndefined();
  });

  it("displaces the Player when it is the only group", () => {
    // There is nowhere else to put it, and the author asked to see the file.
    // This deliberately does NOT split the editor area on their behalf.
    const groups: EditorGroup[] = [
      { id: "group-1", tabs: [playerTab], activeKey: PLAYER_KEY },
    ];
    expect(groupForEditorOpen(groups, "group-1", fileKey("scene2.ink"))).toBeUndefined();
  });

  it("ignores a Player that is open in the focused group but not showing", () => {
    // Another tab is already covering it, so this open covers nothing new.
    const groups: EditorGroup[] = [
      { id: "group-1", tabs: [fileTab("story.ink")], activeKey: fileKey("story.ink") },
      {
        id: "group-2",
        tabs: [playerTab, fileTab("notes.ink")],
        activeKey: fileKey("notes.ink"),
      },
    ];
    expect(groupForEditorOpen(groups, "group-2", fileKey("scene2.ink"))).toBeUndefined();
  });

  it("picks a group that is not the Player's when there are three", () => {
    const groups: EditorGroup[] = [
      { id: "group-1", tabs: [playerTab], activeKey: PLAYER_KEY },
      { id: "group-2", tabs: [fileTab("a.ink")], activeKey: fileKey("a.ink") },
      { id: "group-3", tabs: [fileTab("b.ink")], activeKey: fileKey("b.ink") },
    ];
    const picked = groupForEditorOpen(groups, "group-1", fileKey("scene2.ink"));
    expect(picked).not.toBe("group-1");
    expect(picked).toBe("group-2");
  });

  it("can use an empty group rather than cover the Player", () => {
    const groups: EditorGroup[] = [
      { id: "group-1", tabs: [], activeKey: null },
      { id: "group-2", tabs: [playerTab], activeKey: PLAYER_KEY },
    ];
    expect(groupForEditorOpen(groups, "group-2", fileKey("scene2.ink"))).toBe("group-1");
  });

  it("is inert when the focused group id names no group", () => {
    expect(groupForEditorOpen(twoUp(), "group-9", fileKey("scene2.ink"))).toBeUndefined();
  });
});

describe("openEditorDocument — the shipped composition, over a real store", () => {
  /** The default two-up with the Player focused, as a click inside it leaves
   *  things: entry file left, Player right. */
  function studio() {
    const groups = createEditorGroupsStore();
    groups.getState().openDocument(fileRef("story.ink"), { pinned: true });
    groups.getState().openDocument(playerRef(), { group: "split-right", pinned: true });
    // A click inside the Player focuses its group (the section's focusin).
    const playerGroup = findTab(groups.getState().groups, PLAYER_KEY)!.group.id;
    groups.getState().focusGroup(playerGroup);
    return { groups, playerGroup };
  }

  const visible = (groups: ReturnType<typeof createEditorGroupsStore>): string[] =>
    groups.getState().groups.map((g) => g.activeKey ?? "");

  it("the Player stays on screen when its own reveal button opens a file", () => {
    const { groups, playerGroup } = studio();
    expect(visible(groups)).toEqual([fileKey("story.ink"), PLAYER_KEY]); // non-vacuity

    openEditorDocument(groups, fileRef("scene2.ink"), false);

    // The file took the editor split; the Player is still the visible tab in
    // its own, which is the whole point.
    expect(visible(groups)).toEqual([fileKey("scene2.ink"), PLAYER_KEY]);
    expect(findTab(groups.getState().groups, PLAYER_KEY)?.group.id).toBe(playerGroup);
    // Exactly one tab for the file — no duplicate minted by the redirect.
    const all = groups.getState().groups.flatMap((g) => g.tabs);
    expect(all.filter((t) => documentKey(t.ref) === fileKey("scene2.ink"))).toHaveLength(1);
  });

  it("revealing a file that is already open focuses it in place", () => {
    const { groups } = studio();
    openEditorDocument(groups, fileRef("story.ink"), false);
    expect(visible(groups)).toEqual([fileKey("story.ink"), PLAYER_KEY]);
    const all = groups.getState().groups.flatMap((g) => g.tabs);
    expect(all.filter((t) => documentKey(t.ref) === fileKey("story.ink"))).toHaveLength(1);
  });

  it("opens into the focused group as before when the editor has focus", () => {
    const { groups } = studio();
    const editorGroupId = groups.getState().groups[0].id;
    groups.getState().focusGroup(editorGroupId);

    openEditorDocument(groups, fileRef("scene2.ink"), false);

    expect(visible(groups)).toEqual([fileKey("scene2.ink"), PLAYER_KEY]);
  });
});
