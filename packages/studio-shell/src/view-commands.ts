/**
 * @brink/studio-shell — generated view-toggle commands (spec §5.2, §6).
 *
 * One `view.toggle.<id>` command per registered tool window, with JetBrains
 * style Mod-1…Mod-9 mnemonics assigned by registration order (Binder Mod-1,
 * Player Mod-2, …). Strip clicks, the palette, and keybindings all dispatch
 * these — nothing toggles the layout store directly.
 */

import type { CommandRegistry } from "./command.js";
import type { ShellLayoutStore } from "./layout-store.js";
import type { ToolWindowDescriptor } from "./toolwindow.js";

/** Command id for a tool window's toggle, e.g. "view.toggle.binder". */
export function viewToggleCommandId(toolWindowId: string): string {
  return `view.toggle.${toolWindowId}`;
}

/**
 * Register a `view.toggle.<id>` command per descriptor (in the given order;
 * the first nine get Mod-1…Mod-9). Returns a disposer that unregisters them
 * all — callers re-invoke after registry changes.
 */
export function registerViewToggleCommands(
  commands: CommandRegistry,
  descriptors: readonly ToolWindowDescriptor[],
  layout: ShellLayoutStore,
): () => void {
  const disposers = descriptors.map((descriptor, index) =>
    commands.register({
      id: viewToggleCommandId(descriptor.id),
      title: `View: Toggle ${descriptor.title}`,
      keybinding: index < 9 ? `Mod-${index + 1}` : undefined,
      run: () => layout.getState().toggleToolWindow(descriptor.id),
    }),
  );
  return () => {
    for (const dispose of disposers) dispose();
  };
}
