/**
 * Desktop shell entry (docs/desktop-shell-spec.md, D1).
 *
 * The shell is deliberately thin: pick a folder, build a
 * `TauriFileProvider`, hand everything to `mountStudio` — the same
 * embedder API the playground and any external host use. Everything below
 * this file is the editor packages' business.
 *
 * Project lifecycle (Open/Close) is driven from the native menu via shell
 * events (`menu:open-project` / `menu:close-project`, forwarded by
 * src-tauri) and from the landing screen's button — both funnel into the
 * same `chooseAndOpen` / `closeProject` pair, which own the single
 * `StudioHandle`.
 *
 * App quit (#2370) is separate from Close Project: the window's
 * `onCloseRequested` hook and the app-menu Quit item (`menu:quit`, routed
 * the same way as open/close-project rather than through
 * `PredefinedMenuItem::quit`) both funnel into `handleQuitRequested`, which
 * clears the autosave ticker then awaits the final `saveAll` (capped)
 * before the window is destroyed — a teardown path alongside
 * `closeProject`'s explicit-close and reopen-via-`openProject` clears
 * (#2517; see `autosaveTimer`'s doc comment).
 *
 * macOS Dock Quit / Dock-icon Cmd-Q is a THIRD OS-level quit surface
 * neither of those two reaches, and — despite an earlier version of this
 * file claiming otherwise — is NOT funneled through `handleQuitRequested`
 * today (#2400 remains open; 2026-08-21 review, PR #2927). It reaches
 * `src-tauri` as `RunEvent::Exit` (via `applicationShouldTerminate:`), not
 * `RunEvent::ExitRequested`, and nothing in the dependency tree implements
 * the delegate method that would let Rust intercept it. See
 * `docs/desktop-shell-spec.md`'s "Menus" section for the full reachability
 * writeup and what a real fix would need.
 *
 * File associations (D3, #2393) are a third way a project opens, bundled
 * `.app` only: double-clicking a `.ink`/`.brink` file reaches `handleFileOpen`
 * via `shell:file-open` events / the `take_pending_opens` boot pull, and
 * routes through this same `openProject`/`closeProject` pair — see
 * `file-open.ts` for the (unit-tested) focus-vs-reopen decision.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { save } from "@tauri-apps/plugin-dialog";
import {
  mountStudio,
  type Command,
  type StudioApi,
  type StudioHandle,
} from "@brink-lang/studio";
import type { FileChange } from "@brink-lang/editor";
import {
  TauriFileProvider,
  createProject,
  discoverProjectConfig,
  pickProjectFile,
  pickProjectFolder,
  previousExitClean,
  projectAnchorExists,
  pruneRecent,
  readAppSettings,
  writeAppSettings,
  pushRecent,
  readRecents,
  saveBytesDialog,
} from "./tauri-provider.js";
import {
  anchorForPath,
  buildConflictModel,
  recentDisplayFor,
  resolveBootAction,
} from "./project-open.js";
import { clearConflictBanner, renderConflictBanner } from "./conflict-banner.js";
import { showNewProjectDialog } from "./new-project-dialog.js";
import { awaitSaveAllBeforeQuit } from "./quit.js";
import { runCli } from "./cli.js";
import { exportStoryToInkb } from "./export.js";
import { exportXliff, type ExportXliffApi } from "./export-xliff.js";
import { resolveFileOpenAction } from "./file-open.js";
import { checkForUpdates, shouldAutoCheck, type UpdateApi } from "./updater.js";
import {
  UPDATE_CHECK_COMMAND,
  UPDATE_INSTALL_COMMAND,
  UPDATE_LATER_COMMAND,
} from "./update-commands.js";

/** Loadable project extensions; keep in sync with `list_files` in src-tauri. */
const ENTRY_FALLBACKS = ["story.brink", "main.ink", "main.brink", "story.ink"];

/**
 * Autosave cadence (2026-08-07 ruling, `docs/desktop-shell-spec.md`'s
 * autosave row): autosave IS `saveAll` (celeris §10.1.1) on a 2-minute
 * ticker whenever dirty files exist; clean ticks are no-ops inside the
 * command. Exported (module-level, not a function-local) so
 * `__tests__/autosave-reopen.test.ts` can import — rather than
 * restate — the exact value it pins (#2517); a Settings surface can make
 * this configurable later, but until then this is the single source of
 * truth `openProject`'s `setInterval` call reads.
 */
export const AUTOSAVE_MS = 120_000;

/**
 * Arm an autosave ticker for `api`: every `AUTOSAVE_MS`, dispatch
 * `file.saveAll` if (and only if) something is dirty — a clean tick is a
 * no-op inside the command itself (celeris §10.1.1, see `AUTOSAVE_MS`'s
 * doc comment). Shared by `openProject` (arming a fresh ticker for the
 * newly mounted project) and `handleQuitRequested`'s `destroy()`-rejection
 * recovery (re-arming the ticker it disarmed before the failed quit
 * attempt) — extracted (2026-08-21 review, PR #2927) so the two copies of
 * this closure body cannot drift.
 */
function armAutosave(api: StudioApi): ReturnType<typeof setInterval> {
  return setInterval(() => {
    if (api.getDirtyFiles().length > 0) api.dispatch("file.saveAll");
  }, AUTOSAVE_MS);
}

/** The one open project. D1 is single-window, single-project by ruling. */
let current: StudioHandle | null = null;
/** The open project's root path (absolute), or null when none is open —
 * the default Export filename derives from its final component, the D3
 * file-association handler's "is this file inside it?" test, and (with
 * `currentEntryFile`) the project-relative key `exportXliff` hands the
 * sidecar via the shell's own `resolve()` guard rather than an absolute
 * path built by hand here. Cleared on close, alongside `current`. */
let currentRoot: string | null = null;
/** The autosave ticker for the open project; cleared on close, on reopen
 * (via `closeProject`, called at the top of `openProject`), and on quit
 * (`handleQuitRequested`, #2517) — never left armed past the `StudioHandle`
 * it closes over. */
let autosaveTimer: ReturnType<typeof setInterval> | null = null;
/**
 * The open project's EFFECTIVE entry file — `StudioHandle.entryFile`,
 * i.e. `ProjectSession.getEntryFile()`'s result with `[project] entry`
 * precedence already applied (issue #2331), never the raw host fallback
 * `resolveEntryFile` computed before `mountStudio` ran (2026-08 review
 * finding: using the host fallback here would export a different story
 * than the editor compiles for any project whose `brink.toml` names an
 * entry outside `ENTRY_FALLBACKS`). Set once `mountStudio` resolves; see
 * `openProject`.
 */
let currentEntryFile: string | null = null;
/**
 * The shell-owned banner strip above the mounted studio (#3021's conflict
 * banner renders here). Created per `openProject`, cleared with the rest
 * of `#app` on close/landing.
 */
let bannerHost: HTMLElement | null = null;
/**
 * Governing configs the user chose "Keep standalone" for, per session —
 * keyed by config path so reopening the same standalone story in one
 * session doesn't re-nag. Deliberately NOT persisted: the ruling wants
 * the precedence to never be silent across sessions.
 */
const dismissedConflicts = new Set<string>();

/**
 * Resolve the tab to open first, for a configless project (no `brink.toml`,
 * or one that doesn't set `[project] entry`). `ProjectSession` now owns
 * `[project] entry` precedence itself (issue #2331, ruled 2026-08-07
 * "`[project] entry` beats `mountStudio`'s `entryFile`") — it discovers
 * `brink.toml` and supersedes whatever `entryFile` `mountStudio` was given
 * the moment a valid `entry` is found, so this shell no longer needs to
 * peek at the TOML itself to guess which tab wins: it just picks a
 * reasonable fallback and lets `ProjectSession` override it. The regex peek
 * this function used to do that job is DELETED, not merely unused — the
 * schema slot `brink-project-config` now carries makes it redundant, and a
 * dumb regex duplicating real TOML parsing was always the second-best way
 * to answer this question.
 */
function resolveEntryFile(files: Record<string, string>): string {
  for (const candidate of ENTRY_FALLBACKS) {
    if (files[candidate] !== undefined) return candidate;
  }
  const sources = Object.keys(files)
    .filter((p) => p.endsWith(".brink") || p.endsWith(".ink"))
    .sort();
  return sources[0] ?? Object.keys(files).sort()[0] ?? "main.ink";
}

function appRoot(): HTMLElement {
  const el = document.getElementById("app");
  if (!el) throw new Error("Missing #app container");
  return el;
}

/**
 * The app-icon lockup (compare `docs/design/project-open-flow/Main.dc.html`
 * — the night squircle users see in the Dock, brand blue `#7E96FF` kept
 * exact rather than harmonized to the UI accent). A static markup
 * constant, never interpolated with data.
 */
const LANDING_LOCKUP_SVG = `<svg width="84" height="84" viewBox="0 0 120 120" xmlns="http://www.w3.org/2000/svg" aria-label="Brink Studio"><path d="M60,0 C12,0 0,12 0,60 C0,108 12,120 60,120 C108,120 120,108 120,60 C120,12 108,0 60,0 Z" fill="#101420"></path><g transform="translate(23.6,28) scale(0.727)"><path d="M50 0 C54 10 65.6 23.4 74.94 37.34 A30 30 0 1 1 25.06 37.34 C34.4 23.4 46 10 50 0 Z" fill="#7E96FF"></path><path d="M36 54 L56 54 M50 43 L62 54 L50 65" fill="none" stroke="#101420" stroke-width="7.5" stroke-linecap="round" stroke-linejoin="round"></path></g></svg>`;

/**
 * Render the landing screen (#3021 — compare
 * `docs/design/project-open-flow/Main.dc.html`): the lockup, the two
 * doors (New Project… / Open…), and the recents list with door-kind
 * badges. Paths are inserted via `textContent`, never HTML interpolation —
 * filesystem paths are not attacker input in this shell, but a folder
 * name is still untrusted-enough text that it shouldn't be concatenated
 * into `innerHTML`.
 *
 * `error` (e.g. "picked a .toml that isn't brink.toml") renders as a
 * dismissable line under the doors.
 */
async function renderLanding(error?: string): Promise<void> {
  const root = appRoot();
  bannerHost = null;
  // Undo openProject's flex-column shell layout — the landing owns #app now.
  root.classList.remove("project-shell");
  root.innerHTML = `
    <div id="landing">
      <div class="landing-lockup">
        ${LANDING_LOCKUP_SVG}
        <div class="landing-name">Brink Studio</div>
        <div class="landing-sub">Open a story file or a project config to begin.</div>
      </div>
      <div class="landing-doors">
        <button class="landing-door" id="new-project">
          <span class="door-head"><span class="door-dot door-dot-new"></span><span class="door-title">New Project…</span></span>
          <span class="door-body">Pick a folder. Creates main.ink and brink.toml, ready to play.</span>
        </button>
        <button class="landing-door" id="open-project">
          <span class="door-head"><span class="door-dot door-dot-open"></span><span class="door-title">Open…</span></span>
          <span class="door-body">Open a .ink file — it becomes the entry point — or a brink.toml.</span>
        </button>
      </div>
      <div class="landing-error" hidden></div>
      <div class="landing-recents">
        <div class="landing-cap">Recent</div>
        <ul id="recent-projects" class="recent-projects"></ul>
      </div>
      <label class="landing-reopen">
        <input type="checkbox" id="reopen-last" />
        <span>Reopen last project on launch</span>
      </label>
    </div>`;
  document
    .getElementById("new-project")
    ?.addEventListener("click", () => openNewProjectDialog());
  document
    .getElementById("open-project")
    ?.addEventListener("click", () => void chooseAndOpen());

  const errorEl = root.querySelector<HTMLElement>(".landing-error");
  if (errorEl !== null && error !== undefined) {
    errorEl.hidden = false;
    errorEl.textContent = error;
  }

  // "Reopen last project on launch" (#3016) — persisted in settings.json;
  // honored by bootLanding on the next launch (after a clean exit only).
  const reopenBox = root.querySelector<HTMLInputElement>("#reopen-last");
  if (reopenBox !== null) {
    void readAppSettings().then((settings) => {
      reopenBox.checked = settings.reopenLastProject;
    });
    reopenBox.addEventListener("change", () => {
      void writeAppSettings({ reopenLastProject: reopenBox.checked }).catch((e: unknown) => {
        console.error("[brink-desktop] write_app_settings failed", e);
      });
    });
  }

  const list = document.getElementById("recent-projects");
  if (list === null) return;
  const recents = await readRecents().catch((e: unknown) => {
    console.error("[brink-desktop] read_recents failed", e);
    return [];
  });
  // Empty state (maintainer feedback, 2026-08-23): a bordered list with
  // zero rows collapses to a bare line, and `hidden` on the section was
  // dead — the class's `display: flex` beats the hidden attribute's UA
  // `display: none`. Keep the section, say what belongs here instead.
  if (recents.length === 0) {
    const empty = document.createElement("li");
    empty.className = "recent-empty";
    empty.textContent = "No recent projects yet — anything you open shows up here.";
    list.appendChild(empty);
  }
  for (const path of recents) {
    const display = recentDisplayFor(path, null);
    const item = document.createElement("li");
    const entry = document.createElement("button");
    entry.className = "recent-project";
    entry.title = path;
    const badge = document.createElement("span");
    badge.className = `recent-badge recent-badge-${display.kind}`;
    badge.textContent = display.kind.toUpperCase();
    const name = document.createElement("span");
    name.className = "recent-name";
    name.textContent = display.name;
    const detail = document.createElement("span");
    detail.className = "recent-detail";
    detail.textContent = display.detail;
    entry.append(badge, name, detail);
    entry.addEventListener("click", () => void openRecent(path));
    item.appendChild(entry);
    list.appendChild(item);
  }
}

/** Wire the New Project dialog (#3012) to the real shell commands. On
 *  success the created brink.toml opens on the toml door. */
function openNewProjectDialog(): void {
  showNewProjectDialog({
    chooseFolder: () => pickProjectFolder(),
    create: (dir, entry) => createProject(dir, entry),
    open: (tomlPath) => openAnchorPath(tomlPath),
  });
}

/** How one `openProject` call differs from the pre-#3021 folder open.
 *  Every field optional: a bare `openProject(root)` is exactly the legacy
 *  folder door (which `__tests__/autosave-*.test.ts` still drive). */
export interface OpenProjectOptions {
  /** Project-relative entry file (the story door's opened file). When
   *  absent, the configless fallback (`resolveEntryFile`) applies. */
  entryFile?: string;
  /** Whether that entry is a human's explicit choice — forwarded to
   *  `mountStudio` so a `brink.toml`'s `[project] entry` never supersedes
   *  it (the #2331 revision, ruled 2026-08-23). */
  entryIsExplicit?: boolean;
  /** The anchor recorded in recents (the opened FILE for the two file
   *  doors); defaults to `root` (the legacy folder door). */
  recentPath?: string;
  /** Absolute file to run governing-config discovery from after mount —
   *  the story door's conflict banner probe. */
  conflictProbe?: string;
}

/**
 * Exported (only) so `__tests__/autosave-reopen.test.ts` can drive the real
 * open/close pair directly (#2486) — every in-app caller still reaches this
 * through the same `chooseAndOpen` / menu-event / `handleFileOpen` wiring as
 * before; the export adds no new call site.
 */
export async function openProject(root: string, opts: OpenProjectOptions = {}): Promise<void> {
  const provider = new TauriFileProvider(root);
  const paths = await provider.listFiles();
  const files: Record<string, string> = {};
  for (const path of paths) {
    files[path] = await provider.readFile(path);
  }

  // Tear down any previous project only after the new one's files loaded,
  // so a cancelled or failed open never leaves a blank window. Awaited
  // (#2444) — closeProject's guarded save must land before the new
  // project's files are mounted over it.
  await closeProject();
  currentRoot = root;

  const el = appRoot();
  el.replaceChildren();
  // The shell owns a banner strip above the studio (#3021's conflict
  // banner); the studio mounts into its own child so the strip never
  // overlaps editor chrome.
  el.classList.add("project-shell");
  const banner = document.createElement("div");
  banner.id = "project-banner-host";
  const studioHost = document.createElement("div");
  studioHost.id = "studio-host";
  el.append(banner, studioHost);
  bannerHost = banner;

  const folderName = root.split("/").at(-1) ?? root;
  document.title = `${folderName} — Brink Studio`;

  // A configless-project fallback ONLY — `ProjectSession.initialize()` may
  // supersede this with a `brink.toml`-named entry the instant mountStudio
  // resolves (issue #2331) UNLESS the entry is a human's explicit choice
  // (`opts.entryIsExplicit`, the file-anchored open model). `currentEntryFile`
  // is set from the resolved `StudioHandle.entryFile` below, never from
  // this local, so callers like `exportXliff` always see the effective
  // entry.
  const entryFile = opts.entryFile ?? resolveEntryFile(files);
  currentRoot = root;

  current = await mountStudio(studioHost, {
    files,
    provider,
    entryFile,
    entryIsExplicit: opts.entryIsExplicit,
    // Scope for the per-project editor state that survives a reload — open
    // tabs, order, pins, splits, cursor and scroll. The project root is the
    // identity: stable across sessions, and distinct per project, so two
    // projects each keep their own tabs instead of overwriting one another.
    sessionScope: root,
    // Host commands backing the update toast's buttons — toast actions
    // dispatch command ids, so the buttons need real commands.
    extensions: { commands: UPDATE_COMMANDS },
    // The overlay contract (D2, 2026-08-07 ruling): egress delivery is NOT
    // persistence — dirty means "diverges from the last canonical save".
    // Canonical writes happen through provider.requestSave, awaited by the
    // save commands, which only re-baseline on success.
    egressPersists: false,
    // The #154 egress feeds the BACKUP RING — crash protection at ~500 ms
    // granularity, bounded (25 entries / 10 MB) in the shell, orthogonal
    // to dirty. Backups never clear dirty; ⌘S does.
    onFilesChanged: (changes: FileChange[]) => {
      void provider.ringBackups(changes).catch((e: unknown) => {
        // Ring failures must never block editing — but after two silent
        // -failure hunts (the unregistered command; the unwired hook),
        // they must be VISIBLE: route through the studio's notification
        // surface once the handle exists. The ref is assigned right after
        // mountStudio resolves; a failure in the first ~500 ms window
        // falls back to the console.
        const api = current?.api;
        if (api) {
          api.notify({
            severity: "error",
            source: "backup",
            message: `Backup ring append failed: ${e instanceof Error ? e.message : String(e)}`,
          });
        } else {
          console.error("[brink-desktop] backup ring append failed", e);
        }
      });
    },
  });
  // The EFFECTIVE entry (issue #2331 precedence already applied), not the
  // host fallback computed above — see `currentEntryFile`'s doc comment.
  currentEntryFile = current.entryFile;

  // Autosave IS saveAll (celeris §10.1.1): one save path, one artifact
  // class. Clean ticks are no-ops inside the command. See `AUTOSAVE_MS`'s
  // doc comment for the ruling and why it's a module-level export, and
  // `armAutosave`'s doc comment for why this ticker body lives there
  // instead of inline here.
  autosaveTimer = armAutosave(current.api);

  // Record the successful open (#2394). Fire-and-forget: a recents-store
  // hiccup must never block the project the user just opened. The shell
  // also keeps the native Open Recent submenu in sync as part of this
  // command (`rebuild_menu` in src-tauri).
  void pushRecent(opts.recentPath ?? root).catch((e: unknown) => {
    console.error("[brink-desktop] push_recent failed", e);
  });

  // The story door's governing-config probe (#3010): fire-and-forget —
  // the project is already open and working; the banner is advisory.
  if (opts.conflictProbe !== undefined) {
    void probeGoverningConfig(opts.conflictProbe).catch((e: unknown) => {
      console.error("[brink-desktop] governing-config discovery failed", e);
    });
  }
}

/**
 * Walk up from an explicitly opened story file for a governing
 * `brink.toml` (the compiler's own discovery, via the shell command) and
 * render the conflict banner when one governs (#3010/#3021 — compare
 * `docs/design/project-open-flow/Conflict.dc.html`). The one-click switch
 * is only offered when the opened file IS the config's declared entry,
 * per the ruling.
 */
async function probeGoverningConfig(openedFile: string): Promise<void> {
  const discovered = await discoverProjectConfig(openedFile);
  const model = buildConflictModel(openedFile, discovered);
  if (model === null || dismissedConflicts.has(model.configPath)) return;
  const host = bannerHost;
  if (host === null) return;
  renderConflictBanner(host, model, {
    switchToProject: () => {
      void (async () => {
        // Upgrade rewrites the recents entry IN PLACE (one entry, never
        // two, per the ruling): open the toml anchor — which pushes it to
        // recents — then drop the story-file anchor entry.
        await openAnchorPath(model.configPath);
        await pruneRecent(openedFile).catch(() => {});
      })();
    },
    keepStandalone: () => {
      dismissedConflicts.add(model.configPath);
      clearConflictBanner(host);
    },
  });
}

/**
 * Open one anchor path through its door (#3021): `brink.toml` → the toml
 * door, `.ink` → the story door (explicit entry + conflict probe),
 * anything else → the legacy folder door. The single open seam every
 * caller (landing doors, recents, Open Recent menu, New Project) funnels
 * through.
 */
export async function openAnchorPath(path: string): Promise<void> {
  const anchor = anchorForPath(path);
  if ("error" in anchor) {
    if (current === null) {
      void renderLanding(anchor.error);
    } else {
      // A project is open (⌘O over a mounted studio): surface through its
      // notification surface instead of a console line nobody sees.
      current.api.notify({ severity: "error", source: "open", message: anchor.error });
    }
    return;
  }
  await openProject(anchor.root, {
    entryFile: anchor.entryFile ?? undefined,
    entryIsExplicit: anchor.entryIsExplicit,
    recentPath: anchor.recentPath,
    conflictProbe: anchor.conflictProbe ?? undefined,
  });
}

/** The Open… door (#3021): a native FILE picker — a `.ink` story or a
 *  `brink.toml` — routed through {@link openAnchorPath}. Opening a folder
 *  is no longer a door; the legacy folder kind survives only in old
 *  recents entries. */
async function chooseAndOpen(): Promise<void> {
  const path = await pickProjectFile();
  if (path === null) return; // user cancelled
  try {
    await openAnchorPath(path);
  } catch (e: unknown) {
    console.error("[brink-desktop] failed to open project", e);
    if (current === null) {
      currentRoot = null;
      void renderLanding(e instanceof Error ? e.message : String(e));
    }
  }
}

/**
 * Open a project chosen from the recents list (native Open Recent submenu
 * or the landing-screen list). Unlike {@link chooseAndOpen}, a failure here
 * lazily prunes the stale entry (#2394's "do not crash the open flow;
 * prune lazily") — the path came from a persisted list, not a fresh folder
 * pick, so a missing/moved folder is an expected failure mode, not a rare
 * edge case.
 *
 * Pruning is gated on {@link projectAnchorExists} (2026-08 review finding),
 * not fired on every rejection: `openProject` can also reject from a
 * permission error, a file deleted mid-listing, or a `mountStudio` failure
 * — none of which mean the project itself is gone. Only an actually-missing
 * root gets removed from `recents.json` and the native Open Recent submenu;
 * every other failure just surfaces to the console and re-shows the
 * landing screen with the entry intact.
 */
async function openRecent(path: string): Promise<void> {
  try {
    await openAnchorPath(path);
  } catch (e: unknown) {
    console.error("[brink-desktop] failed to open recent project", e);
    // Default to "exists" on a failed check itself, so a transient
    // check-command error can never masquerade as evidence of deletion.
    const anchorGone = !(await projectAnchorExists(path).catch(() => true));
    if (anchorGone) {
      await pruneRecent(path).catch(() => {});
    }
    if (current === null) void renderLanding();
  }
}

/**
 * Unmount the current project and restore the landing screen. Before the
 * unmount, awaits a canonical save of anything dirty (the unmount's own
 * egress flush only feeds the RING under the overlay contract — the ring
 * protects the work, but close should leave canonical files current too).
 * Ruled 2026-08-07 (docs/decision-log.md, "Desktop close: no dirty prompt;
 * quit awaits the final save"): no dirty-state close confirmation prompt —
 * dead UI, not implemented.
 *
 * The save itself reuses `awaitSaveAllBeforeQuit` (quit.ts) — the exact
 * same guarded, unconditional-dispatch, redispatch-until-drained-or-capped
 * seam `handleQuitRequested` awaits below — rather than a second copy of
 * that logic (#2444). Before this fix, Close Project's flush was a single
 * `dispatch("file.saveAll")`, fire-and-forget (never awaited) and gated on
 * `getDirtyFiles().length > 0`: structurally the same gap #2434/#2437 fixed
 * for quit, on this sibling teardown path. The gate was wrong for the same
 * reason it was wrong there — `getDirtyFiles()` only reflects the 500ms
 * debounce-recorded dirty set, so a keystroke made just before Close
 * Project could see an empty dirty set and skip the save entirely,
 * dropping the edit — and never awaiting meant `current.unmount()` could
 * run (and the studio tear down) before that single dispatch's write ever
 * landed.
 *
 * Clearing `autosaveTimer` here — and `openProject` always awaiting this
 * before arming a fresh one — is the entire reason a reopened project never
 * ends up with two autosave tickers running at once. That "no-duplicate-
 * interval" property is pinned by `__tests__/autosave-reopen.test.ts`
 * (#2486): dropping or reordering the clear below is a data-loss-adjacent
 * regression, not a cosmetic one (docs/desktop-shell-spec.md, autosave row).
 * Exported (only) so that test can call it directly — see `openProject`'s
 * export comment.
 *
 * `current` (and `currentRoot`/`currentEntryFile`) are captured into a
 * local and cleared SYNCHRONOUSLY, before the `await` below (2026-08-21
 * review, PR #2927): this function used to null them only after
 * `awaitSaveAllBeforeQuit` resolved, so a second, overlapping call — e.g. a
 * second Close Project click during the (up to ~3s) save wait, when the
 * `menu:close-project` listener is fire-and-forget `() => void
 * closeProject()` with no visible feedback disabling it, or Close Project
 * immediately followed by Open Recent — would still read `current !==
 * null`, pass the guard above, and later call `handle.unmount()` a second
 * time against an already-unmounted (or concurrently unmounting) handle.
 * Nulling the module state up front makes that guard reject the second
 * call immediately and synchronously, before it can await anything.
 */
export async function closeProject(): Promise<void> {
  if (current === null) return;
  const handle = current;
  current = null;
  currentRoot = null;
  currentEntryFile = null;
  if (autosaveTimer !== null) {
    clearInterval(autosaveTimer);
    autosaveTimer = null;
  }
  await awaitSaveAllBeforeQuit(handle.api);
  handle.unmount();
  document.title = "Brink Studio";
  void renderLanding();
}

/**
 * File > Export XLIFF… (D3, #2392) — proves the `brink-cli` sidecar path
 * end to end. The export logic itself lives in `export-xliff.ts` (2026-08
 * review finding: logic living directly in `main.tsx` cannot be unit-tested
 * — `quit.ts` + `QuitSaveApi` exist for exactly this reason); this wrapper's
 * only job is gathering the currently-open project (root + EFFECTIVE entry,
 * `currentEntryFile`, never the host fallback) and the studio's notify sink,
 * then handing them to the extracted, unit-tested function.
 */
async function handleExportXliff(): Promise<void> {
  const api = current?.api;
  if (currentRoot === null || currentEntryFile === null || api === undefined) {
    console.warn("[brink-desktop] Export XLIFF: no project open");
    return;
  }
  const exportApi: ExportXliffApi = {
    runCli: (invocation) => runCli(invocation),
    save,
    notify: (entry) => api.notify(entry),
  };
  await exportXliff({ root: currentRoot, entryFile: currentEntryFile }, exportApi);
}

/**
 * Export Story (.inkb) (D3 slice 1, #2391). The actual compile-then-save
 * logic lives in `export.ts` (extracted for testability, like `quit.ts`);
 * this wrapper just supplies the live `StudioApi`, the open project's root,
 * and the real save dialog.
 */
async function handleExportInkb(): Promise<void> {
  if (current === null || currentRoot === null) return;
  await exportStoryToInkb(current.api, currentRoot, saveBytesDialog);
}

/**
 * App quit (#2370, docs/decision-log.md 2026-08-07 "Desktop close: no dirty
 * prompt; quit awaits the final save"): NO confirmation prompt — explicitly
 * ruled out. The one safety piece is that the window must not actually
 * close until the final `saveAll` has had its chance to land, so quitting
 * can never race the in-flight canonical write. `awaitSaveAllBeforeQuit` is
 * capped (~3s) — a hung write cannot make the app unquittable, since the
 * backup ring (#154) already bounds the loss.
 *
 * `autosaveTimer` is cleared here too, BEFORE the `await` below (#2517):
 * this is `closeProject`'s structurally parallel sibling for teardown, and
 * leaving the ticker armed through the (up to ~3s) save-wait means an
 * autosave tick landing in that window would fire `dispatch("file.saveAll")`
 * against a project mid-quit — redundant at best alongside
 * `awaitSaveAllBeforeQuit`'s own dispatch/redispatch, and a needless extra
 * write path racing the teardown at worst. Disarming it up front removes
 * that window entirely rather than relying on the 120s cadence being long
 * enough in practice. Exported (only) so
 * `__tests__/autosave-quit.test.ts` can drive it directly, same as
 * `openProject`/`closeProject` for `__tests__/autosave-reopen.test.ts`.
 *
 * `getCurrentWindow().destroy()` is wrapped in a `try`/`catch` (#2401): this
 * runs AFTER the native side has already committed to not closing on its
 * own — `onCloseRequested`'s `event.preventDefault()` for red-button close,
 * or simply having reached this function via the `menu:quit` event for
 * ⌘Q — so an unhandled rejection here — an IPC failure, a permission
 * denial — would leave the window closable only via Force Quit: nothing
 * else in this file ever retries. A caught failure re-arms `autosaveTimer`
 * (cleared above, and otherwise never re-armed — the narrower case #2401's
 * own tracking comment records) so autosave does not stay silently dead for
 * the rest of the session, and surfaces the failure through the studio's
 * notification surface so the author sees the app didn't quit rather than
 * assuming it's about to. This does not make `destroy()` itself succeed —
 * only a Force Quit (or a later, successful quit attempt) can end the
 * process once it's rejected — but it keeps the app in a working,
 * save-capable state instead of a silently degraded one, and a subsequent
 * ⌘Q / red-button close calls this same function again rather than the app
 * being permanently wedged. (Dock Quit does not reach this function at all
 * — #2400 remains open; see this file's header comment.)
 */
export async function handleQuitRequested(): Promise<void> {
  if (current !== null) {
    if (autosaveTimer !== null) {
      clearInterval(autosaveTimer);
      autosaveTimer = null;
    }
    await awaitSaveAllBeforeQuit(current.api);
  }
  try {
    await getCurrentWindow().destroy();
  } catch (e: unknown) {
    console.error("[brink-desktop] quit failed: window destroy() rejected", e);
    if (current !== null) {
      if (autosaveTimer === null) {
        autosaveTimer = armAutosave(current.api);
      }
      current.api.notify({
        severity: "error",
        source: "quit",
        message: `Quit failed to close the window (${e instanceof Error ? e.message : String(e)}). Your work has been saved — try quitting again.`,
      });
    }
  }
}

/**
 * Focus an already-open project file through the studio's own navigation
 * protocol (`editor.reveal`, docs/studio-shell-spec.md §6.1) — the same
 * command Problems/Search/Story Graph dispatch, reused rather than forked
 * (the span is a no-op placeholder; a source Location just needs a valid
 * span shape to resolve, and reveal-at-file-open has no meaningful offset).
 */
function focusFile(rel: string): void {
  current?.api.dispatch("editor.reveal", {
    kind: "source",
    file: rel,
    span: { start: 0, end: 0 },
  });
}

/**
 * Handle one OS file-open request (D3, #2393): `resolveFileOpenAction`
 * (file-open.ts) is the pure decision — focus in place, or open a new
 * project root. Opening reuses `openProject`, which already close-saves
 * any previously open project before mounting the new one (the ruled
 * close flow) — this never forks a second open path.
 *
 * Mirrors {@link chooseAndOpen}'s failure handling (2026-08 review finding):
 * `openProject` can realistically reject (an unreadable subdirectory, a
 * non-UTF-8 file, a `mountStudio` throw). Without a catch here, a failed
 * open left `current === null` with the landing screen never re-rendered
 * (blank window) and `currentRoot` already pointing at the failed root, so
 * every later double-click under that root would resolve to a dead
 * `focus` action against a null `current`. On failure, fall back to the
 * landing screen exactly like `chooseAndOpen` and skip the focus dispatch
 * — there is nothing mounted to focus into.
 */
async function handleFileOpen(path: string): Promise<void> {
  const action = resolveFileOpenAction(path, currentRoot);
  if (action.kind === "open") {
    try {
      await openProject(action.root, {
        // The story door (#3021): a double-clicked `.ink` is a human's
        // explicit entry choice, recorded in recents as the file anchor
        // and probed for a governing config. A `.brink` keeps the legacy
        // folder door (native file-anchoring deferred).
        entryFile: action.rel,
        entryIsExplicit: action.entryIsExplicit,
        recentPath: action.entryIsExplicit ? path : action.root,
        conflictProbe: action.entryIsExplicit ? path : undefined,
      });
    } catch (e: unknown) {
      console.error("[brink-desktop] failed to open project from file-open", e);
      if (current === null) {
        currentRoot = null;
        void renderLanding();
      }
      return;
    }
  }
  focusFile(action.rel);
}

/**
 * File associations (docs/desktop-shell-spec.md D3; #2393) ONLY bite in the
 * bundled `.app` — `bundle.fileAssociations` registers `.ink`/`.brink` with
 * the OS, and a dev run (`pnpm tauri dev`) never receives an OS file-open
 * launch, so nothing below ever fires there. On a bundled build,
 * double-clicking (or Dock-dropping) an associated file reaches
 * `RunEvent::Opened` in src-tauri/src/lib.rs, which forwards paths here as
 * live `shell:file-open` events. `take_pending_opens` is a one-time pull at
 * boot for paths that arrived before this listener was attached (a cold
 * launch — the Rust side buffers until this exact call, then switches to
 * live emits; see `PendingOpens`'s doc comment there).
 *
 * The pull is sequenced strictly after the listener is registered
 * (2026-08 review finding): `listen()` is itself an async IPC round-trip
 * (`invoke('plugin:event|listen', …)` under the hood), so firing
 * `take_pending_opens` at module top level without awaiting `listen()`
 * first raced the two — `take_pending_opens` flips the Rust side's
 * `PendingOpens` state to "ready" (live-emit) the instant it runs, which
 * could land before the listener actually existed, silently dropping an
 * `Opened` delivered in that exact window (Tauri's JS event bus has no
 * replay). Chaining `.then()` off `listen()`'s own resolution closes that
 * gap.
 */
const coldStartOpens: Promise<boolean> = listen<string[]>("shell:file-open", (event) => {
  void (async () => {
    for (const path of event.payload) {
      await handleFileOpen(path);
    }
  })();
})
  .then(() => invoke<string[]>("take_pending_opens"))
  .then(async (paths) => {
    for (const path of paths) {
      await handleFileOpen(path);
    }
    // Reported to bootLanding (#3016): a double-clicked file always wins
    // over auto-reopen, so a cold-start OS open suppresses it.
    return paths.length > 0;
  })
  .catch((e: unknown) => {
    console.error("[brink-desktop] file-open wiring failed", e);
    return false;
  });

void listen("menu:new-project", () => openNewProjectDialog());
void listen("menu:open-project", () => void chooseAndOpen());
void listen("menu:close-project", () => void closeProject());
void listen("menu:export-inkb", () => void handleExportInkb());
void listen("menu:export-xliff", () => void handleExportXliff());
// File → Open Recent (#2394): src-tauri emits the chosen path as the event
// payload (see `on_menu_event` in src-tauri/src/lib.rs).
void listen<string>("menu:open-recent", (event) => void openRecent(event.payload));
// The app-menu Quit item (⌘Q) is routed here as a plain shell event — like
// open/close-project — instead of `PredefinedMenuItem::quit`, specifically
// so it funnels through the same guarded path as the window close below
// rather than the OS quit item's own (unguarded) native teardown.
void listen("menu:quit", () => void handleQuitRequested());
// View → view mode (decision log 2026-08-26). The menu is a second door onto
// the same commands the palette and the Settings picker use, so all three
// agree by construction rather than by being kept in step.
void listen("menu:view-mode-code", () => {
  current?.api.dispatch("view.editor.code");
});
void listen("menu:view-mode-single", () => {
  current?.api.dispatch("view.editor.single");
});
void listen("menu:view-mode-continuous", () => {
  current?.api.dispatch("view.editor.continuous");
});
// View → editor font size. The native items carry no accelerators (see
// `build_menu` in src-tauri/src/lib.rs); they dispatch the same commands the
// studio's own ⌘+/⌘−/⌘0 bindings do, so the menu is a second door onto one
// implementation rather than a second implementation.
void listen("menu:view-font-increase", () => {
  current?.api.dispatch("editor.fontSize.increase");
});
void listen("menu:view-font-decrease", () => {
  current?.api.dispatch("editor.fontSize.decrease");
});
void listen("menu:view-font-reset", () => {
  current?.api.dispatch("editor.fontSize.reset");
});
// View → panels. The payload is a tool-window id; the shell generates one
// `view.toggle.<id>` command per registered tool window. A menu entry for a
// tool window this build does not register dispatches nothing (the registry
// returns false) rather than throwing — that is what keeps the native list
// in `VIEW_PANELS` safe to drift.
void listen<string>("menu:view-toggle", (event) => {
  current?.api.dispatch(`view.toggle.${event.payload}`);
});
// Routed through the command so the menu item and the toast's Try Again
// share one path (and one throttle clock).
void listen("menu:check-updates", () => {
  lastUpdateCheckAt = Date.now();
  void checkForUpdates(updateApi());
});

/** One id for every update toast, so each stage REPLACES the last rather
 *  than stacking (the notification service treats a repeated id as a
 *  replacement). */
const UPDATE_NOTIFICATION_ID = "update";
// The ids live in `update-commands.ts` (no side effects) so tests can
// import and validate them; see that file for why 0.4.0 shipped them wrong.
export {
  UPDATE_INSTALL_COMMAND,
  UPDATE_LATER_COMMAND,
  UPDATE_CHECK_COMMAND,
} from "./update-commands.js";

/** Resolver for the offer currently on screen, if any. */
let pendingUpdateOffer: ((accepted: boolean) => void) | null = null;

/** Settle the outstanding offer exactly once. Safe to call when none is up. */
function settleUpdateOffer(accepted: boolean): void {
  const resolve = pendingUpdateOffer;
  pendingUpdateOffer = null;
  resolve?.(accepted);
}

/**
 * Host commands backing the update toast's buttons. Toast actions dispatch
 * command ids (NotificationAction carries no callbacks), so the buttons
 * need real commands — contributed through the host extension seam like
 * any other host command.
 *
 * The two offer commands are gated by `when`: with no offer pending they
 * are unavailable, so neither can be invoked from the palette to "install"
 * an update that was never staged.
 */
export const UPDATE_COMMANDS: Command[] = [
  {
    id: UPDATE_INSTALL_COMMAND,
    title: "Update: Install and Restart",
    when: () => pendingUpdateOffer !== null,
    run: () => settleUpdateOffer(true),
  },
  {
    id: UPDATE_LATER_COMMAND,
    title: "Update: Later",
    when: () => pendingUpdateOffer !== null,
    run: () => settleUpdateOffer(false),
  },
  {
    id: UPDATE_CHECK_COMMAND,
    title: "Update: Check for Updates",
    run: () => {
      // Manual checks bypass the throttle (the author asked) but still
      // restart its clock, so alt-tabbing right afterwards doesn't
      // immediately fire a second round trip.
      lastUpdateCheckAt = Date.now();
      void checkForUpdates(updateApi());
    },
  },
];

/**
 * Bind the injected {@link UpdateApi} to the real plugins (D4). The decision
 * tree itself lives in `updater.ts`, dependency-free and unit-tested; this is
 * only the wiring.
 */
function updateApi(): UpdateApi {
  return {
    check: async () => {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (update === null) return null;
      return {
        version: update.version,
        downloadAndInstall: async () => {
          // Amend the offer toast in place (same id) so the accepted
          // update reports itself instead of going quiet until the app
          // restarts under the author.
          current?.api.notify({
            id: UPDATE_NOTIFICATION_ID,
            severity: "info",
            source: "update",
            message: `Downloading ${update.version}\u2026 the app will restart when it finishes.`,
            timeoutMs: 0,
          });
          await update.downloadAndInstall();
        },
      };
    },
    confirm: async (version) => {
      // An update offer is a notification, not an interruption: a modal
      // steals focus mid-sentence for something that can wait. With a
      // project open it becomes a sticky toast carrying its own actions;
      // the promise this returns is settled by whichever the author picks
      // (see UPDATE_COMMANDS), so updater.ts's decision tree is unchanged.
      const api = current?.api;
      if (!api) {
        // Landing screen: no studio surface exists yet, so there is nowhere
        // to put a toast. The native dialog stays the fallback.
        const { ask } = await import("@tauri-apps/plugin-dialog");
        return ask(`Brink Studio ${version} is available. Install and restart?`, {
          title: "Update available",
          kind: "info",
          okLabel: "Install and Restart",
          cancelLabel: "Later",
        });
      }
      // A second check while an offer is still up replaces it; the older
      // promise settles as declined so no caller is left hanging.
      settleUpdateOffer(false);
      return new Promise<boolean>((resolve) => {
        pendingUpdateOffer = resolve;
        api.notify({
          id: UPDATE_NOTIFICATION_ID,
          severity: "info",
          source: "update",
          message: `Brink Studio ${version} is available.`,
          // Sticky: an offer that evaporates while you read it is worse
          // than no offer at all.
          timeoutMs: 0,
          actions: [
            { label: "Install and Restart", commandId: UPDATE_INSTALL_COMMAND },
            { label: "Later", commandId: UPDATE_LATER_COMMAND },
          ],
        });
      });
    },
    notify: (severity, message) => {
      // With a project open the studio's own surface is the right place; on
      // the landing screen there is no StudioApi yet, so fall back to a
      // native dialog rather than dropping the message on the floor.
      const api = current?.api;
      if (api) {
        api.notify({
          // Same id as the offer, so an outcome REPLACES the offer in place
          // rather than stacking a second update toast beside it.
          id: UPDATE_NOTIFICATION_ID,
          severity,
          source: "update",
          message,
          // A failed check or install is worth retrying without hunting
          // through the menu bar. Errors are sticky by severity default.
          actions:
            severity === "error"
              ? [{ label: "Try Again", commandId: UPDATE_CHECK_COMMAND }]
              : undefined,
        });
        return;
      }
      void import("@tauri-apps/plugin-dialog").then(({ message: dialog }) =>
        dialog(message, { title: "Brink Studio", kind: severity === "error" ? "error" : "info" }),
      );
    },
    // Reuses the quit guard rather than a third save discipline — see
    // updater.ts's module doc. A no-op when no project is open.
    awaitSave: async () => {
      if (current !== null) await awaitSaveAllBeforeQuit(current.api);
    },
    relaunch: async () => {
      const { relaunch } = await import("@tauri-apps/plugin-process");
      await relaunch();
    },
  };
}

/** When the last check of any kind ran (epoch ms); 0 = never. */
let lastUpdateCheckAt = 0;

/**
 * An automatic check — silent, and gated by `shouldAutoCheck` (the policy
 * itself lives in updater.ts, dependency-free and unit-tested; this is only
 * the wiring, like the rest of updateApi).
 */
async function autoCheckForUpdates(now: number = Date.now()): Promise<void> {
  if (
    !shouldAutoCheck({ lastCheckAt: lastUpdateCheckAt, offerPending: pendingUpdateOffer !== null, now })
  ) {
    return;
  }
  lastUpdateCheckAt = now;
  await checkForUpdates(updateApi(), { silent: true });
}

// Launch check (ruled 2026-08-22): silent, and deliberately delayed — the
// first seconds after startup belong to mounting the editor, not to a
// network round trip. Silent means a no-update result and an offline failure
// both say nothing; only an actual update prompts.
setTimeout(() => void autoCheckForUpdates(), 5_000);

// Focus check (ruled 2026-08-25): coming back to the window is the natural
// moment to notice a release that shipped while the author was elsewhere —
// the launch check only ever fires for people who restart. Same silent,
// throttled path; `onFocusChanged` fires for blur too, so the flag matters.
void getCurrentWindow().onFocusChanged(({ payload: focused }) => {
  if (focused) void autoCheckForUpdates();
});

void getCurrentWindow().onCloseRequested(async (event) => {
  // Always prevent the native close first. (`@tauri-apps/api`'s own default
  // — skip `preventDefault()` and it auto-destroys once this handler
  // resolves — would work identically here, since nothing below ever
  // rejects; calling it explicitly just keeps "await save, then destroy"
  // visible in this file instead of resting on that SDK default.)
  event.preventDefault();
  await handleQuitRequested();
});

/**
 * Launch (#3016): wait for any cold-start OS file-open to land first (a
 * double-clicked file always wins), then either auto-reopen the last
 * project — preference ON and the previous session exited cleanly — or
 * show the landing (with a note when the crash guard suppressed a
 * reopen). Every input failure degrades to the plain landing screen.
 */
async function bootLanding(): Promise<void> {
  const osOpenHandled = await coldStartOpens;
  if (current !== null) return; // an OS open already mounted a project
  const [settings, prevClean, recents] = await Promise.all([
    readAppSettings(),
    previousExitClean(),
    readRecents().catch(() => [] as string[]),
  ]);
  const action = resolveBootAction({
    reopenLastProject: settings.reopenLastProject,
    previousExitClean: prevClean,
    osOpenHandled,
    recents,
  });
  if (action.kind === "none") return;
  if (action.kind === "reopen") {
    try {
      await openAnchorPath(action.path);
      if (current !== null) return;
    } catch (e: unknown) {
      console.error("[brink-desktop] reopen-last-project failed", e);
    }
    if (current === null) void renderLanding();
    return;
  }
  void renderLanding(action.note);
}

void bootLanding().catch((e: unknown) => {
  console.error("[brink-desktop] boot failed", e);
  void renderLanding();
});
