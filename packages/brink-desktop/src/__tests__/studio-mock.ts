/**
 * The shared `vi.mock` body for `@brink-lang/studio`.
 *
 * Same reason as `tauri-provider-mock.ts`: `main.tsx` reaches into this
 * module at module scope, and a missing key is a namespace access that
 * throws while the summary still reports every test as passing.
 *
 * Only `mountStudio` is replaced — each suite spies on it differently, and
 * that spy IS what those suites assert against. Everything else comes from
 * `vi.importActual`, deliberately: `main.tsx` builds a real settings section
 * out of the studio's row primitives, and hand-stubbing them would make this
 * file a second copy of them, free to drift from the real ones without
 * anything noticing.
 *
 * It must be `importActual` rather than a static import — a static one is
 * hoisted above the `vi.mock` call that uses it, and initialises to the
 * mocked module it is meant to be building.
 *
 * A plain module, NOT a `.test.ts` (house rule, #2510/#2516).
 */

import { vi } from "vitest";

export async function studioMock(mountStudio: unknown): Promise<Record<string, unknown>> {
  const actual = await vi.importActual<Record<string, unknown>>("@brink-lang/studio");
  return { ...actual, mountStudio };
}
