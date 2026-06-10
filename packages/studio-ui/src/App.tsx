/**
 * App — thin composition over the studio shell (spec §3, issue #80).
 *
 * The old hardcoded three-pane layout is gone: ShellFrame renders strips,
 * docks, and the editor area purely from the tool-window registry plus the
 * shell layout store (both owned by ShellProvider; tool windows are
 * registered at bootstrap in brink-studio/main.tsx). This component only
 * provides the root element (tier observer + data-tier for CSS), the editor
 * slot, and the app-level transient surfaces (palette, toast).
 */

import { useRef, type ReactNode } from "react";
import { CommandPalette, ShellFrame, useShellLayout, useTier } from "@brink/studio-shell";
import { EditorPane } from "./EditorPane.js";
import { Toast } from "./Toast.js";
import { QuickOpen } from "./QuickOpen.js";

function App({ editorSlot }: { editorSlot: ReactNode }) {
  const tier = useShellLayout((s) => s.tier);

  const rootRef = useRef<HTMLDivElement>(null);
  useTier(rootRef);

  return (
    <div className="brink-studio" data-tier={tier} ref={rootRef}>
      <ShellFrame editorSlot={<EditorPane>{editorSlot}</EditorPane>} />
      <CommandPalette />
      <QuickOpen />
      <Toast />
    </div>
  );
}

export { App };
