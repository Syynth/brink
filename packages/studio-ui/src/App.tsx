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
import { SearchCommands } from "./SearchView.js";
import { ConflictMergeView } from "./ConflictMergeView.js";
import { useStudioStore } from "./StoreContext.js";
import type { CSSProperties, ReactNode } from "react";

/** `children` render inside the `.brink-studio` root — popup hosts and other
 *  fixed-position surfaces MUST live here, or the scoped styles and theme
 *  tokens never reach them (the "menu renders unstyled at the document
 *  tail" bug class, #3054 review). */
function App({ children }: { children?: ReactNode }) {
  const tier = useShellLayout((s) => s.tier);
  // Theme (spec §7.4): the persisted selection is read by the ThemeService
  // before the first render, so data-theme is right on the initial paint.
  const themeId = useThemeId();

  const rootRef = useRef<HTMLDivElement>(null);
  useTier(rootRef);

  // Gutter visibility (Settings / editor context menu): mirrored as a root
  // class the stylesheet acts on — no CM reconfiguration, and hiding them
  // also removes the WebKit per-gutter-element layout cost (#3119).
  const showGutters = useStudioStore((s) => s.showGutters);

  // Editor text size: mirrored onto the root as a CSS custom property the
  // CM6 theme reads (`--bs-editor-font-size`). A style property rather than
  // a class because the value is continuous, and going through CSS means
  // every mounted editor reflows at once with no CM reconfiguration.
  const editorFontSize = useStudioStore((s) => s.editorFontSize);
  const appFontSize = useStudioStore((s) => s.appFontSize);
  // Player prose size (W13/#3306): 0 = follow the app scale — the var
  // stays unset so player.css's fallback (`--bs-font-prose`) applies.
  const playerFontSize = useStudioStore((s) => s.playerFontSize);

  return (
    <div
      className={"brink-studio" + (showGutters ? "" : " brink-gutters-hidden")}
      data-tier={tier}
      data-theme={themeId}
      style={
        {
          "--bs-editor-font-size": `${editorFontSize}px`,
          "--bs-font-base": `${appFontSize}px`,
          ...(playerFontSize > 0
            ? { "--bs-player-font-size": `${playerFontSize}px` }
            : {}),
        } as CSSProperties
      }
      ref={rootRef}
    >
      <ShellFrame />
      <CommandPalette />
      <QuickOpen />
      <NewFilePrompt />
      <SearchCommands />
      <ConflictMergeView />
      <NotificationStack />
      {children}
      {/* Reparent mount for CM6 tooltips (#3349, #3357 review): CM6's own
          tooltip container is `position: relative`, not `fixed`/`absolute`
          (`tooltips({ parent })`, see `tooltip-portal.ts`), so it cannot
          mount as a direct child of this flex root without becoming an
          in-flow item that disrupts the shell's layout — that broke the
          binder/single-file-view/drag-redock E2E suites. This layer is kept
          out of flow (`position: absolute; width: 0; height: 0`,
          `editor.css`) so `tooltip-portal.ts` has somewhere to mount that
          stays inside the theme scope without touching flex flow. Must stay
          a sibling of `children` (not inside it), and last in DOM order has
          no significance — it never participates in layout or paint order
          for anything but the fixed-positioned tooltips CM6 places inside
          it. */}
      <div className="brink-tooltip-layer" aria-hidden="true" />
    </div>
  );
}

export { App };
