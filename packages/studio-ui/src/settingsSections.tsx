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
import { FormattingSettings } from "./FormattingSettings.js";
import { ProseSettings } from "./ProseSettings.js";
import { DraftSettings } from "./DraftSettings.js";
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
      id: SETTINGS_SECTION_IDS.general,
      scope: "project",
      title: "General",
      keywords:
        "brink.toml entry conventions dialect types indent drafts draft scratch cut wip config project",
      icon: SETTINGS_ICONS.project,
      body: (
        <>
          <DraftSettings />
          <ProjectSection groupId={groupId} />
        </>
      ),
    },
    {
      id: SETTINGS_SECTION_IDS.formatting,
      scope: "project",
      title: "Formatting",
      keywords: "indent spaces tabs width fmt format whitespace",
      icon: SETTINGS_ICONS.formatting,
      body: <FormattingSettings />,
    },
    {
      id: SETTINGS_SECTION_IDS.diagnostics,
      scope: "project",
      title: "Diagnostics",
      keywords: "lints warnings errors todo suppress allow deny",
      icon: SETTINGS_ICONS.diagnostics,
      body: <LintSettings />,
    },
    {
      id: SETTINGS_SECTION_IDS.prose,
      scope: "project",
      title: "Prose",
      keywords: "spelling spellcheck grammar dictionary dialect british american typo",
      icon: SETTINGS_ICONS.prose,
      body: <ProseSettings />,
    },
    {
      id: SETTINGS_SECTION_IDS.editor,
      scope: "app",
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
      scope: "app",
      title: "Appearance",
      keywords: "theme colour color dark light manuscript inky",
      icon: SETTINGS_ICONS.appearance,
      body: <ThemeSection />,
    },
    {
      id: SETTINGS_SECTION_IDS.keymap,
      scope: "app",
      title: "Keymap",
      keywords: "keybinding shortcut chord override",
      icon: SETTINGS_ICONS.keymap,
      body: <KeymapSection />,
    },
    {
      // Split out of Diagnostics by the scope switch (#3174): the `[lints]`
      // table is written to brink.toml and shared, this flag is a studio
      // preference that stays on your machine. They were one section with a
      // hint explaining the difference, which the switch now states.
      id: SETTINGS_SECTION_IDS.external,
      scope: "app",
      title: "External functions",
      keywords: "host manifest binding check severity diagnostics",
      icon: SETTINGS_ICONS.diagnostics,
      body: <DiagnosticsSection />,
    },
  ];
}
