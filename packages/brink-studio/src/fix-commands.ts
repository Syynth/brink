/**
 * Auto-fix commands (`docs/autofix-spec.md` §7's command-palette surface).
 *
 * Two entries, both callers of `fix_all` with a `Select{tiers: ["safe"]}`:
 * the whole project, and the file the focused editor is showing. The editor
 * context menu's "Fix all safe in this file" dispatches the second rather
 * than duplicating the call, so both roads run the identical selection.
 *
 * No keybinding: batch-rewriting the manuscript is not a gesture that
 * belongs on a chord an author can hit by accident. Both are findable in the
 * palette, and rebindable from Settings ▸ Keymap like any other command.
 */

import type { CommandRegistry } from "@brink/studio-shell";
import {
  FIX_ALL_SAFE_FILE_COMMAND_ID,
  FIX_ALL_SAFE_PROJECT_COMMAND_ID,
  runFixAll,
  safeSelect,
  type FixStoreState,
} from "@brink/studio-ui";

export { FIX_ALL_SAFE_FILE_COMMAND_ID, FIX_ALL_SAFE_PROJECT_COMMAND_ID };

/** What this module needs of the studio store. */
export interface FixCommandDeps {
  /** The store's current state — read fresh per invocation, never captured. */
  getState: () => FixStoreState;
  /**
   * The file the focused editor is showing, or `null` with no editor focused.
   * Read at dispatch time: the palette can be opened from anywhere, and the
   * focused tab may have changed since registration.
   */
  activePath: () => string | null;
  notify: (n: { severity: "info" | "warning" | "error"; source: string; message: string }) => void;
}

/** Register both commands. Returns a disposer. */
export function registerFixCommands(
  commands: CommandRegistry,
  deps: FixCommandDeps,
): () => void {
  const disposers = [
    commands.register({
      id: FIX_ALL_SAFE_PROJECT_COMMAND_ID,
      title: "Fix: Fix all safe in project",
      run: () => {
        void runFixAll(deps.getState(), safeSelect(), "Fix all safe in project");
      },
    }),
    commands.register({
      id: FIX_ALL_SAFE_FILE_COMMAND_ID,
      title: "Fix: Fix all safe in this file",
      run: () => {
        const path = deps.activePath();
        if (path === null) {
          // Honest refusal rather than silently widening to the project:
          // "in this file" with no file is not "in every file".
          deps.notify({
            severity: "info",
            source: "fix",
            message: "No editor focused — nothing to fix",
          });
          return;
        }
        void runFixAll(deps.getState(), safeSelect(path), `Fix all safe in ${path}`);
      },
    }),
  ];
  return () => {
    for (const dispose of disposers) dispose();
  };
}
