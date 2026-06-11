/**
 * @brink/studio-shell — embedder extension API (docs/studio-shell-spec.md §8).
 *
 * Hosts embedding the studio (the embedded playground; RPG Maker MZ planned)
 * may provide their own tool windows, commands, and status-bar items at
 * mount time. The extension point is registration into the same registries
 * the built-ins use — no dynamic loading, no marketplace, no sandboxing, no
 * separate code path. Host items are equal citizens: tool windows dock,
 * toggle, drag, persist, and appear in strips/palette/hamburger exactly like
 * built-ins, and layout persistence drops unknown ids on load (§7.1), which
 * handles a host removing a panel between sessions.
 *
 * The one rule is namespacing: every host id must carry the
 * `host.<vendor>.` prefix (built-ins are forbidden from it); the registries'
 * `registerHost` paths validate this and reject collisions with clean errors.
 */

import { CommandRegistry, type Command } from "./command.js";
import { ToolWindowRegistry, type ToolWindowDescriptor } from "./toolwindow.js";
import { StatusBarRegistry, type StatusBarItemDescriptor } from "./statusbar.js";

/**
 * Host-provided surfaces, passed once at mount alongside the existing
 * initialization wiring (spec §8.1). All ids must be `host.<vendor>.<name>`.
 */
export interface StudioExtensions {
  /** §7.1 shape; ids must be "host.<vendor>.<name>". */
  toolWindows?: ToolWindowDescriptor[];
  /** §6 shape; same id namespacing. */
  commands?: Command[];
  /** §7.3 shape; same id namespacing. */
  statusBarItems?: StatusBarItemDescriptor[];
}

/** The registries an extension installs into (the same ones built-ins use). */
export interface StudioExtensionRegistries {
  commands: CommandRegistry;
  toolWindows: ToolWindowRegistry;
  statusBarItems: StatusBarRegistry;
}

/**
 * Install a host extension config into the registries. Validates the
 * mandatory `host.<vendor>.` namespacing and rejects collisions by throwing;
 * on failure, everything this call already registered is rolled back, so a
 * rejected install leaves the registries untouched.
 *
 * Returns an uninstall function that unregisters every item (idempotent).
 */
export function installStudioExtensions(
  extensions: StudioExtensions,
  registries: StudioExtensionRegistries,
): () => void {
  const disposers: (() => void)[] = [];
  const disposeAll = (): void => {
    // Unregister in reverse registration order.
    for (let i = disposers.length - 1; i >= 0; i--) disposers[i]();
    disposers.length = 0;
  };

  try {
    for (const toolWindow of extensions.toolWindows ?? []) {
      disposers.push(registries.toolWindows.registerHost(toolWindow));
    }
    for (const command of extensions.commands ?? []) {
      disposers.push(registries.commands.registerHost(command));
    }
    for (const item of extensions.statusBarItems ?? []) {
      disposers.push(registries.statusBarItems.registerHost(item));
    }
  } catch (error) {
    disposeAll();
    throw error;
  }

  return disposeAll;
}
