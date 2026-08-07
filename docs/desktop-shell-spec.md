# Desktop shell spec — `brink-desktop` (Tauri)

**Status:** v1 ruled 2026-08-06 (`docs/decision-log.md`, "Desktop app v1: Tauri
shell, local build first; mobile deferred"). Revives ruling-ledger #28
(2026-03-17, "brink-studio standalone app uses Tauri"), which the ledger
flagged as built-by-nobody with no owning issue. This spec is the owner;
`docs/brink-studio-spec.md` §"Desktop app (Tauri)" and
`docs/studio-shell-spec.md`'s desktop mentions defer here.

## Scope ruling

**v1 is a local build.** No code signing, no notarization, no updater, no
multi-OS release matrix — a macOS dev build the maintainer runs locally.
Promotion to a distributable is a later, separate stage (D4) with its own
workflow file (never `release.yml` — cargo-dist owns that file and edits
break the release `plan` check).

**Mobile is recorded as an interest and explicitly deferred.** It shapes one
choice today: Tauri 2's iOS/Android targets keep a future mobile client on
this same stack and this same webview frontend, which is part of why the
shell choice stands. Nothing else in this spec may take a dependency that
forecloses mobile, and nothing may be added *for* mobile before it is
scheduled.

## Architecture (reaffirmed from the March ruling)

The webview runs the **existing studio frontend** — the same
`@brink-lang/studio` `mountStudio` build the playground uses — with the
**wasm backend** (`@brink-lang/web`). One integration path: browser,
embedded, and desktop all exercise the same `EditorSession`. A native
Rust core spoken to over Tauri IPC remains the *perf escape hatch only*,
explicitly out of v1 (and out of v2; it gets specced if a real project
measurably outgrows the wasm session, not before).

The shell is deliberately thin. Its entire job:

| Concern | Mechanism |
|---|---|
| Window, chrome, shortcuts | Tauri window + native menu bar |
| Open a project | native folder dialog → `TauriFileProvider` → `mountStudio` |
| File I/O | `TauriFileProvider` implementing `packages/ink-editor/src/provider.ts` |
| External edits | fs watcher → `onExternalChange` → the existing #320 conflict/merge UI |
| Project config | nothing — `ProjectSession` already discovers `brink.toml` and re-runs on every change (#2324) |
| Recent projects | Tauri store (or a JSON file in app-data) |
| Export | `compile_project` bytes → native save dialog |

Everything below the shell — analysis, diagnostics, claiming, completions,
the Player — is the editor packages' business and is protected by the
editor acceptance gate (`crates/brink-web/src/editor/acceptance_gate.rs`).

## The one new component: `TauriFileProvider`

Implements the full `FileProvider` contract over Tauri's fs API. The
interface was designed for this (its doc comment names "Tauri/FS" as an
anticipated implementation); every method maps directly:

| Contract method | Tauri mapping | Notes |
|---|---|---|
| `listFiles()` | recursive dir walk | include `*.brink`, `*.ink`, `brink.toml`; skip dotfiles, `target/`, `node_modules/` |
| `readFile` / `requestFile` | `fs.readTextFile` | `requestFile` returns null for paths outside the project root — never escape the opened folder |
| `onFileChanged` | **buffer, don't write** | v1 keeps the studio's explicit-save model: dirty state lives in the editor; disk writes happen on `file.save`/`saveAll` via `requestSave`. Autosave is a later option, not a default |
| `createFile` / `deleteFile` / `renameFile` | `fs.writeTextFile` / `remove` / `rename` | implement `renameFile` natively (atomic; the fallback create+delete loses nothing today but real fs deserves real rename) |
| `onExternalChange` | fs watcher on the project root | debounce; deliver `null` on delete; unsubscribe on teardown per the contract. This lights up the #320 conflict → kept-buffer → merge surface with a *real* watcher for the first time |
| `requestSave` | write all dirty buffers | the egress batch (`onFilesChanged`, #154) is the source of what to write |

Path discipline: provider keys are project-relative with `/` separators
(the studio's convention); the provider owns the mapping to absolute OS
paths and never leaks them into the session.

## Workspace placement

`packages/brink-desktop/` — a small TS host package (the mount wrapper,
menu wiring, provider) plus `src-tauri/` (the Rust shell crate).

**The `src-tauri` crate is excluded from the root cargo workspace.** The
workspace already carries Bevy's dependency graph as its dominant build
cost; Tauri's graph is comparably heavy and would land in every
`cargo test --workspace`, every wave agent's shared target, and CI's
required lanes for zero coverage benefit (the shell has almost no logic).
Cost of exclusion: its dep versions are managed in its own `Cargo.toml`
rather than the workspace table — acceptable for a leaf artifact.

CI in v1: none required. A non-required smoke job (`cargo check` the shell
crate + `pnpm build` the package) may be added if drift appears. The
required lanes must not grow a Tauri build.

## Menus

Generated from the studio's **command registry**, per
`docs/studio-shell-spec.md`'s own forward-pointer ("the same registry could
feed a native menu bar in a future desktop shell"). The shell maps
registry commands into the native menu and calls `dispatch(commandId)` —
no hand-maintained parallel menu logic. v1 menu surface: App (about/quit),
File (Open Folder…, Open Recent, Save, Save All, Export `.inkb`…, Close
Window), Edit (native webview roles), Story (Play/Restart, from the
existing player commands), View (panel toggles), Help (docs link).

Quit is a plain `MenuItem` (not Tauri's `PredefinedMenuItem::quit`),
forwarded to the webview as a `menu:quit` shell event — the same pattern
already used for Open/Close Project. Ruled 2026-08-07 (#2370): the
predefined Quit item's native teardown is not guaranteed to reach the
webview (`on_menu_event`/`CloseRequested`) on every platform, which would
silently bypass the guarded quit path (`awaitSaveAllBeforeQuit`) for ⌘Q,
the most common real quit action on macOS.

## Entry flow

1. Open Folder… → folder dialog → instantiate `TauriFileProvider` at that
   root → `listFiles` → `mountStudio` with the provider.
2. Entry file: let `ProjectSession`'s `brink.toml` discovery decide (#2324
   recorded the precedence); when no `brink.toml` names an entry, fall back
   to `main.ink` / single-file heuristics — whatever the studio already
   does for the playground, unchanged.
3. Reopening: recent-projects list → same flow.

## Stages

- **D1 — the spike.** Scaffold (`pnpm tauri dev` against the Vite build),
  `TauriFileProvider` (open/read/save; no watcher), Open Folder flow.
  Acceptance: open a real on-disk copy of the acceptance-gate project,
  see zero diagnostics, edit, save, verify on disk.
- **D2 — a real host.** fs watcher → `onExternalChange` (acceptance: edit
  a file in another editor, see the #320 conflict surface), native
  rename, recent projects, registry-driven menus, window title = project
  name, quit awaits the final `saveAll` before the window closes (#2370;
  ruled 2026-08-07 — no dirty-state close-confirmation prompt, that's dead
  UI given autosave + save-on-close).
- **D3 — output.** Export `.inkb` via `compile_project` bytes + save
  dialog; `brink-cli` as a Tauri **sidecar** for batch ops (xliff
  export/locale compile) so both cores ship from one workspace version.
  File associations (`.ink`, `.brink`).
- **D4 — distribution (deferred).** Signing, notarization, updater, own
  release workflow, and the promote-to-distributable decision. Not
  planned until explicitly scheduled.

## Out of scope, recorded so nobody relitigates

- Native-core-over-IPC (perf escape hatch only, needs evidence first).
- The studio shell redesign (`docs/studio-shell-spec.md`) — lands
  independently; the desktop app consumes whichever shell exists.
- Scrivenings mode and other editor features — editor-package work, not
  shell work.
- Mobile (deferred as above).
- Multi-window / multi-project (one window, one project in v1).
