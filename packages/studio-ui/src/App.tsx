/**
 * App — thin composition over the studio shell (spec §3, issue #80).
 *
 * The old hardcoded three-pane layout is gone: ShellFrame renders strips,
 * docks, and the editor area (groups + per-group tab bars, §7.8) purely from
 * the shell registries and stores — tool windows and document types are
 * registered at bootstrap in brink-studio/main.tsx. This component only
 * provides the root element (tier observer + data-tier for CSS) and the
 * app-level transient surfaces (palette, quick-open, the new-file prompt,
 * the notification stack §7.5).
 */

import { useRef } from "react";
import {
  CommandPalette,
  NotificationStack,
  ShellFrame,
  useShellLayout,
  useThemeId,
  useTier,
} from "@brink/studio-shell";
import { QuickOpen } from "./QuickOpen.js";
import { NewFilePrompt } from "./NewFilePrompt.js";

function App() {
  const tier = useShellLayout((s) => s.tier);
  // Theme (spec §7.4): the persisted selection is read by the ThemeService
  // before the first render, so data-theme is right on the initial paint.
  const themeId = useThemeId();

  const rootRef = useRef<HTMLDivElement>(null);
  useTier(rootRef);

  return (
    <div className="brink-studio" data-tier={tier} data-theme={themeId} ref={rootRef}>
      <ShellFrame />
      <CommandPalette />
      <QuickOpen />
      <NewFilePrompt />
      <NotificationStack />
    </div>
  );
}

export { App };
