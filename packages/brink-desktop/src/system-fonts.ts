/**
 * The machine's font families for Settings › Player › Font (#3439), from
 * the shell's `list_system_fonts` command. A failed call is not an error
 * the author can act on — the studio shows its curated list instead, so
 * this resolves to an empty list rather than throwing, and the mount
 * passes an empty list through as "no host list".
 */
import { invoke } from "@tauri-apps/api/core";

export async function systemFonts(): Promise<readonly string[]> {
  try {
    const fonts = await invoke<unknown>("list_system_fonts");
    return Array.isArray(fonts) ? fonts.filter((f): f is string => typeof f === "string") : [];
  } catch {
    return [];
  }
}
