/**
 * What the window and the landing's recents call a project (decision log
 * 2026-10-09): its `brink.toml`'s `[project] name`, or its folder's name.
 *
 * `brink.toml` is parsed shell-side (`config_project_name`), by the config
 * crate every other surface uses, so this module only decides WHEN to ask
 * and what to do with the answer. It takes the asking as a function, so the
 * decisions get unit tests without a shell (`__tests__/project-title.test.ts`).
 */

import type { FileChange } from "@brink-lang/editor";
import { baseName } from "./project-open.js";
import { parentDir } from "./file-open.js";

/** The app's own name, which every window title ends with. */
export const APP_TITLE = "Brink Studio";

/** The project config's root-relative path — the one `brink.toml` the
 *  studio applies for a project opened at its root. */
export const CONFIG_FILE = "brink.toml";

/** `name — Brink Studio`, with the folder's name when there is no name. */
export function windowTitle(name: string | null, root: string): string {
  const folder = root.split("/").at(-1) || root;
  return `${name ?? folder} — ${APP_TITLE}`;
}

/** The config's text after an egress batch: its content when the batch
 *  wrote `brink.toml`, null when it deleted it, and undefined when the
 *  batch did not touch it (the title stays as it is). */
export function configTextIn(changes: readonly FileChange[]): string | null | undefined {
  let text: string | null | undefined;
  for (const change of changes) {
    if (change.path !== CONFIG_FILE) continue;
    text = change.type === "deleted" ? null : (change.content ?? text);
  }
  return text;
}

/**
 * The window title for one open project. The title is the folder's name at
 * once, and the config's name as soon as the shell has parsed it; every
 * edit to `brink.toml` asks again. The latest ask wins, so a slow answer
 * about older text never overwrites a newer one, and nothing lands after
 * {@link ProjectTitle.dispose} (the project closed).
 */
export class ProjectTitle {
  private asked = 0;
  private disposed = false;

  constructor(
    private readonly root: string,
    private readonly nameOf: (configText: string) => Promise<string | null>,
    private readonly apply: (title: string) => void,
  ) {}

  /** Re-title from the config's current text; null means no config. */
  async update(configText: string | null): Promise<void> {
    const ask = ++this.asked;
    const name = configText === null ? null : await this.nameOf(configText);
    if (this.disposed || ask !== this.asked) return;
    this.apply(windowTitle(name, this.root));
  }

  dispose(): void {
    this.disposed = true;
  }
}

/**
 * The `[project] name` of the `brink.toml` at `configPath` (absolute), read
 * when the landing draws so a recent is never stale. Null when the file is
 * gone, unreadable or unnamed — the row falls back.
 */
export async function readProjectName(
  configPath: string,
  readFile: (root: string, rel: string) => Promise<string>,
  nameOf: (configText: string) => Promise<string | null>,
): Promise<string | null> {
  let text: string;
  try {
    text = await readFile(parentDir(configPath), baseName(configPath));
  } catch {
    return null;
  }
  return nameOf(text);
}
