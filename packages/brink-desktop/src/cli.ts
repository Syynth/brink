/**
 * `brink-cli` sidecar bridge (docs/desktop-shell-spec.md, D3 / #2392).
 *
 * The frontend never talks to the sidecar's process API directly — it goes
 * through the shell's `run_cli` command, which enforces the fixed
 * subcommand allowlist (export-xliff, compile-locale, regenerate-xliff,
 * compile) before spawning anything. This module is just the thin,
 * typed wrapper around that one `invoke` call plus its output stream.
 *
 * `root` + `rel` (not a raw input path) is deliberate (2026-08 review
 * finding): the shell resolves `rel` against `root` through its own
 * stay-inside-the-project-root guard before the sidecar ever sees a path,
 * exactly like `read_file`/`write_file` do. Only `rest` may still carry an
 * absolute path (e.g. `export-xliff`'s dialog-chosen `--output <path>`).
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** One line of sidecar output, matching the shell's `CliOutputLine`. */
export interface CliOutputLine {
  stream: "stdout" | "stderr";
  line: string;
}

/**
 * One `run_cli` invocation. `rel` is a project-relative key (resolved
 * against `root` by the shell); `rest` is forwarded to the sidecar verbatim
 * after the resolved input path.
 */
export interface CliInvocation {
  root: string;
  rel: string;
  subcommand: string;
  rest?: string[];
}

/**
 * Run an allowlisted `brink-cli` subcommand as the Tauri sidecar. Resolves
 * to the process exit code once it terminates; rejects if the shell
 * refuses the invocation (missing/disallowed subcommand, `rel` escaping
 * `root`) or the sidecar itself fails to spawn. `onOutput`, if given, is
 * called for each streamed stdout/stderr line as the process runs.
 */
export async function runCli(
  invocation: CliInvocation,
  onOutput?: (line: CliOutputLine) => void,
): Promise<number> {
  const unlisten = onOutput
    ? await listen<CliOutputLine>("cli:output", (event) => onOutput(event.payload))
    : null;
  try {
    return await invoke<number>("run_cli", {
      root: invocation.root,
      rel: invocation.rel,
      subcommand: invocation.subcommand,
      rest: invocation.rest ?? [],
    });
  } finally {
    unlisten?.();
  }
}
