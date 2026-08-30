/**
 * Shared session bootstrap for e2e specs (W7/#3300 — RULED "no
 * auto-start": the startup compile leaves the Player idle, Run/Start
 * begins the session). Specs that need a RUNNING story call this after
 * load; the poll retry-clicks through the compile race (the
 * placeholder's Start does nothing until story bytes land).
 */
import { expect, type Page } from "@playwright/test";

export async function ensureStoryStarted(page: Page): Promise<void> {
  await expect
    .poll(
      async () => {
        const txt =
          (await page
            .locator(".story-text")
            .textContent()
            .catch(() => "")) ?? "";
        if (txt.trim().length > 0) return true;
        await page
          .locator(".session-placeholder-start")
          .click({ timeout: 500 })
          .catch(() => {});
        return false;
      },
      { timeout: 20_000 },
    )
    .toBe(true);
}

/**
 * Failure-state dump (#3346): read the live studio store's session/debug
 * state so a CI-only failure identifies its stuck branch instead of
 * costing a blind rerun. The story-graph overlay flake motivated this —
 * `data-current` absent for the whole wait window can mean a stuck
 * degraded checksum window, an unresolved `current_location`, or a dead
 * overlay subscription, and only state-at-failure tells them apart.
 * Everything is read defensively: the page may be mid-teardown after a
 * test timeout, and a dump must never throw over the real failure.
 */
export async function dumpStudioState(page: Page): Promise<string> {
  try {
    const state = await page.evaluate(() => {
      type AnyStore = { getState(): Record<string, unknown> };
      const stores = (window as unknown as { __brinkStores?: AnyStore[] }).__brinkStores;
      const st = stores?.[0]?.getState();
      if (!st) return { error: "no __brinkStores[0]" };
      const debugState = st.debugState as {
        status?: string;
        current_location?: string | null;
        turn_index?: number;
        position?: unknown;
      } | null;
      const provider = st._provider as { pacedRunning?: () => boolean } | null;
      let pacedRunning: boolean | string = "n/a";
      try {
        pacedRunning = provider?.pacedRunning?.() ?? "no provider";
      } catch (e) {
        pacedRunning = `threw: ${String(e)}`;
      }
      const lines = (st.sessionLines as { text: string }[] | undefined) ?? [];
      return {
        sessionStatus: st.sessionStatus,
        sessionPaused: st.sessionPaused,
        sessionAuto: st.sessionAuto,
        sessionPacedMs: st.sessionPacedMs,
        programChecksum: st.programChecksum,
        compiledChecksum: st.compiledChecksum,
        degraded:
          st.programChecksum !== null &&
          st.compiledChecksum !== null &&
          st.programChecksum !== st.compiledChecksum,
        debugState: debugState
          ? {
              status: debugState.status,
              current_location: debugState.current_location,
              turn_index: debugState.turn_index,
              position: debugState.position,
            }
          : null,
        lastOutcomeReason: (st.debugLastOutcome as { reason?: unknown } | null)?.reason ?? null,
        choices: (st.sessionChoices as unknown[] | undefined)?.length ?? 0,
        transcript: {
          length: lines.length,
          tail: lines.slice(-2).map((l) => l.text.slice(0, 60)),
        },
        pacedRunning,
        entryFile: st.entryFile,
        storyBytes: st.storyBytes ? "present" : "null",
      };
    });
    return JSON.stringify(state, null, 2);
  } catch (e) {
    return `dumpStudioState failed: ${String(e)}`;
  }
}
