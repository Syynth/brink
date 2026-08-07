/**
 * File-association decision logic (D3, docs/desktop-shell-spec.md; #2393).
 *
 * `bundle.fileAssociations` (tauri.conf.json) registers `.ink`/`.brink`
 * with the OS — this only bites in the BUNDLED `.app`; a dev run
 * (`pnpm tauri dev`) never receives a file-open launch, so nothing here is
 * reachable there. Double-clicking (or Dock-dropping) an associated file
 * fires the Rust-side `RunEvent::Opened` handler (src-tauri/src/lib.rs),
 * which forwards absolute paths to the frontend as `shell:file-open`
 * events (live) or via one `take_pending_opens` pull at boot (cold-start
 * opens that raced the webview's listener attaching).
 *
 * This module is pure decision logic, kept separate from `main.tsx` for
 * the same reason `quit.ts` is: `main.tsx` owns the actual IO (the single
 * `openProject`/`closeProject`/`StudioHandle` seam) and stays a thin,
 * hard-to-unit-test IPC-wiring layer, while the "what should happen"
 * question — focus in place vs. open a new project — is ordinary pure
 * logic that deserves real tests.
 */

/** One resolved response to an OS file-open request. */
export type FileOpenAction =
  | { kind: "focus"; rel: string }
  | { kind: "open"; root: string; rel: string };

/**
 * Parent directory of an absolute POSIX path (v1 is a macOS-only local
 * build per docs/desktop-shell-spec.md's scope ruling — no Windows
 * backslash handling here). A path with no `/` (or only a leading one)
 * has no meaningful parent short of the filesystem root.
 */
export function parentDir(path: string): string {
  const idx = path.lastIndexOf("/");
  return idx > 0 ? path.slice(0, idx) : "/";
}

/**
 * Project-relative key for `path` under `root`, or `null` when `path`
 * does not live inside `root`. Mirrors the provider-key convention
 * (`/`-separated, root-relative) `TauriFileProvider`/`list_files` already
 * use — this never invents a second path space.
 */
export function relativeToRoot(root: string, path: string): string | null {
  const prefix = root.endsWith("/") ? root : `${root}/`;
  return path.startsWith(prefix) ? path.slice(prefix.length) : null;
}

/**
 * Decide what one OS file-open request should do, given the currently open
 * project root (`null` if no project is open):
 *
 * - No project open, or `path` falls outside it → open the file's
 *   containing folder as the new project root (the caller runs the
 *   existing `openProject` seam, which already close-saves any previously
 *   open project before mounting the new one — the ruled close flow) and
 *   focus `path` there.
 * - `path` is inside the open project → just focus it, no reopen.
 */
export function resolveFileOpenAction(path: string, currentRoot: string | null): FileOpenAction {
  if (currentRoot !== null) {
    const rel = relativeToRoot(currentRoot, path);
    if (rel !== null) return { kind: "focus", rel };
  }
  const root = parentDir(path);
  // `root` is `path`'s own parent, so `path` is always inside it — the
  // fallback to the raw path only guards a malformed input (e.g. one with
  // no `/` at all, where `parentDir` returns "/" and the join is moot).
  const rel = relativeToRoot(root, path) ?? path;
  return { kind: "open", root, rel };
}
