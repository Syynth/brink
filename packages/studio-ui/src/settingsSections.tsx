/**
 * The Settings sections, as data (#3174).
 *
 * One entry per rail row. The modal knows how to draw a rail and a pane and
 * nothing else, so adding a settings surface means adding an entry here —
 * never editing the shell. That is what keeps the page from drifting behind
 * what is actually configurable, which is the failure mode a hand-laid-out
 * settings screen has.
 *
 * `keywords` is searched alongside the title, so a section is findable by
 * what it DOES as well as by its name — "lint", "todo" and "warning" should
 * all reach Diagnostics, and none of them is in the word "Diagnostics".
 */

import type { SettingsSection } from "./SettingsModal.js";
import { SETTINGS_ICONS } from "./SettingsModal.js";
import {
  DiagnosticsSection,
  EditorSection,
  EditorViewSection,
  KeymapSection,
  ProjectSection,
  ThemeSection,
} from "./SettingsDocument.js";
import { LintSettings } from "./LintSettings.js";
import { SETTINGS_SECTION_IDS } from "./settingsSectionIds.js";

/**
 * `groupId` reaches `ProjectSection`, which mounts the real `brink.toml`
 * document — a CM6 view keyed by (document, group). The modal is not an
 * editor group, so it passes a stable id of its own rather than borrowing
 * one: two views of the same document in the same group would collide.
 */
export function settingsSections(groupId: string): SettingsSection[] {
  return [
    {
      id: SETTINGS_SECTION_IDS.project,
      title: "Project",
      keywords: "brink.toml entry conventions dialect types indent drafts config",
      icon: SETTINGS_ICONS.project,
      body: <ProjectSection groupId={groupId} />,
    },
    {
      id: SETTINGS_SECTION_IDS.diagnostics,
      title: "Diagnostics",
      keywords: "lints warnings errors todo suppress allow deny external functions",
      icon: SETTINGS_ICONS.diagnostics,
      body: (
        <>
          <LintSettings />
          <DiagnosticsSection />
        </>
      ),
    },
    {
      id: SETTINGS_SECTION_IDS.editor,
      title: "Editor",
      keywords: "font size view mode code single file continuous tabs",
      icon: SETTINGS_ICONS.editor,
      body: (
        <>
          <EditorViewSection />
          <EditorSection />
        </>
      ),
    },
    {
      id: SETTINGS_SECTION_IDS.appearance,
      title: "Appearance",
      keywords: "theme colour color dark light manuscript inky",
      icon: SETTINGS_ICONS.appearance,
      body: <ThemeSection />,
    },
    {
      id: SETTINGS_SECTION_IDS.keymap,
      title: "Keymap",
      keywords: "keybinding shortcut chord override",
      icon: SETTINGS_ICONS.keymap,
      body: <KeymapSection />,
    },
  ];
}
