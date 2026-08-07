/**
 * `brink-cli` sidecar bridge (docs/desktop-shell-spec.md, D3 / #2392).
 *
 * The frontend never talks to the sidecar's process API directly — it goes
 * through the shell's `run_cli` command, which enforces the fixed
 * subcommand allowlist (export-xliff, compile-locale, regenerate-xliff,
 * compile) before spawning anything. This module is just the thin,
 * typed wrapper around that one `invoke` call plus its output stream.
 */

import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

/** One line of sidecar output, matching the shell's `CliOutputLine`. */
export interface CliOutputLine {
  stream: "stdout" | "stderr";
  line: string;
}

/**
 * Run an allowlisted `brink-cli` subcommand as the Tauri sidecar. Resolves
 * to the process exit code once it terminates; rejects if the shell
 * refuses the args (missing/disallowed subcommand) or the sidecar itself
 * fails to spawn. `onOutput`, if given, is called for each streamed
 * stdout/stderr line as the process runs.
 */
export async function runCli(
  args: string[],
  onOutput?: (line: CliOutputLine) => void,
): Promise<number> {
  const unlisten = onOutput
    ? await listen<CliOutputLine>("cli:output", (event) => onOutput(event.payload))
    : null;
  try {
    return await invoke<number>("run_cli", { args });
  } finally {
    unlisten?.();
  }
}
