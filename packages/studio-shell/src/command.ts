/**
 * @brink/studio-shell — command registry.
 *
 * The single dispatch point for every shell action. Keybindings, palette
 * entries, menu items, strip clicks, and buttons all dispatch commands by id;
 * nothing binds a key directly to a function (docs/studio-shell-spec.md §6).
 */

export interface Command {
  /** Stable, namespaced id, e.g. "player.toggleVisible". */
  id: string;
  /** Palette display title, e.g. "View: Toggle Player". */
  title: string;
  /**
   * Default keybinding(s), e.g. "Mod-J" or ["Mod-Shift-P", "F1"] (Mod = Cmd
   * on macOS, Ctrl elsewhere). Several defaults exist because browsers
   * reserve different chords (#107); the first is the primary shown in
   * hints. Resolution goes through the keymap layer, which merges user
   * overrides — never read this field to handle keys directly.
   */
  keybinding?: string | readonly string[];
  /** Enablement predicate, consulted at dispatch and when listing. */
  when?: () => boolean;
  run(args?: unknown): void | Promise<void>;
}

/** Id prefix reserved for embedder-host registrations (spec §8.1). */
export const HOST_ID_PREFIX = "host.";

/**
 * Validate an embedder-host id: `host.<vendor>.<name>` with non-empty vendor
 * and name segments (spec §8.1). `kind` names the registry for the error
 * message ("command", "tool window", "status bar item").
 */
export function assertHostId(kind: string, id: string): void {
  const rest = id.startsWith(HOST_ID_PREFIX) ? id.slice(HOST_ID_PREFIX.length) : null;
  const dot = rest === null ? -1 : rest.indexOf(".");
  if (rest === null || dot <= 0 || dot === rest.length - 1) {
    throw new Error(
      `host ${kind} id "${id}" must be namespaced "host.<vendor>.<name>" (spec §8.1)`,
    );
  }
}

export class CommandRegistry {
  private readonly commands = new Map<string, Command>();
  private readonly changeListeners = new Set<() => void>();

  /**
   * Register a command. Throws on duplicate ids and on built-ins claiming the
   * host-reserved prefix. Returns an unregister function.
   */
  register(command: Command): () => void {
    if (command.id.startsWith(HOST_ID_PREFIX)) {
      throw new Error(
        `command id "${command.id}" uses the prefix reserved for embedder hosts`,
      );
    }
    return this.insert(command);
  }

  /**
   * Register an embedder-host command (spec §8.1) — the inverse of
   * `register`'s built-in rule: the id MUST carry the `host.<vendor>.`
   * prefix. Throws on malformed ids and collisions with a clean error.
   * Returns an unregister function.
   */
  registerHost(command: Command): () => void {
    assertHostId("command", command.id);
    return this.insert(command);
  }

  private insert(command: Command): () => void {
    if (this.commands.has(command.id)) {
      throw new Error(`duplicate command id "${command.id}"`);
    }
    this.commands.set(command.id, command);
    this.notifyChange();
    return () => {
      if (this.commands.delete(command.id)) this.notifyChange();
    };
  }

  /**
   * Subscribe to registrations/unregistrations. Components register commands
   * at mount, so keymaps and menus rebuild from this. Returns an unsubscribe
   * function.
   */
  onDidChange(listener: () => void): () => void {
    this.changeListeners.add(listener);
    return () => {
      this.changeListeners.delete(listener);
    };
  }

  private notifyChange(): void {
    for (const listener of this.changeListeners) listener();
  }

  get(id: string): Command | undefined {
    return this.commands.get(id);
  }

  /** All registered commands, in registration order. */
  list(): Command[] {
    return [...this.commands.values()];
  }

  isEnabled(id: string): boolean {
    const command = this.commands.get(id);
    return command !== undefined && (command.when === undefined || command.when());
  }

  /**
   * Dispatch by id. Returns true if the command ran; false if it is unknown
   * or disabled by its `when` predicate.
   */
  dispatch(id: string, args?: unknown): boolean {
    const command = this.commands.get(id);
    if (command === undefined || (command.when !== undefined && !command.when())) {
      return false;
    }
    void command.run(args);
    return true;
  }
}
