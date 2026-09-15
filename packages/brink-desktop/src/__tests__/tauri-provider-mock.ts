/**
 * The shared `vi.mock` body for `../tauri-provider.js`.
 *
 * Five suites drive `main.tsx`, which binds the whole provider surface at
 * module scope and again inside `unifiedUpdateApi()`. A key missing from the
 * mock is a module-namespace access that THROWS — and vitest reports that as
 * an unhandled rejection while every test still says "passed", so the failure
 * is loud in the log and invisible in the summary.
 *
 * Five hand-maintained copies of the same object drifted twice in one
 * session (F1's `bundleUpdateApply`/`bundleActivate`, F3's
 * `bundleAvailable`). This is the drift's single source instead. A suite
 * that needs a different answer overrides the key it cares about:
 *
 * ```ts
 * vi.mock("../tauri-provider.js", () => ({
 *   ...tauriProviderMock({ TauriFileProvider: FakeProvider }),
 *   readRecents: vi.fn(() => Promise.resolve(["/proj"])),
 * }));
 * ```
 *
 * A plain module, NOT a `.test.ts` — importing a test file re-registers its
 * `describe`/`it` blocks in the importer (house rule, #2510/#2516).
 */

import { vi } from "vitest";

/**
 * Every export `main.tsx` reaches for, with the boring answer.
 *
 * `TauriFileProvider` has no default: each suite supplies its own fake, and
 * a shared one would couple their file behaviour together.
 */
export function tauriProviderMock({
  TauriFileProvider,
}: {
  TauriFileProvider: unknown;
}): Record<string, unknown> {
  return {
    TauriFileProvider,
    pickProjectFolder: vi.fn(() => Promise.resolve(null)),
    projectAnchorExists: vi.fn(() => Promise.resolve(true)),
    pickProjectFile: vi.fn(() => Promise.resolve(null)),
    discoverProjectConfig: vi.fn(() => Promise.resolve(null)),
    createProject: vi.fn(() => Promise.resolve("")),
    readAppSettings: vi.fn(() =>
      Promise.resolve({
        reopenLastProject: false,
        updatePolicy: { mode: "auto", channel: "stable" },
      }),
    ),
    writeAppSettings: vi.fn(() => Promise.resolve()),
    previousExitClean: vi.fn(() => Promise.resolve(true)),
    pruneRecent: vi.fn(() => Promise.resolve([])),
    pushRecent: vi.fn(() => Promise.resolve([])),
    readRecents: vi.fn(() => Promise.resolve([])),
    saveBytesDialog: vi.fn(() => Promise.resolve(null)),
    // The OTA boot confirmation runs at main.tsx module scope and is
    // deliberately unconditional (docs/desktop-ota-spec.md Stage 2).
    bundleReady: vi.fn(() => Promise.resolve({ version: null, rolledBackFrom: null })),
    bundleUpdateCheck: vi.fn(() => Promise.resolve({ kind: "upToDate" })),
    bundleUpdateApply: vi.fn(() => Promise.resolve({ kind: "upToDate" })),
    bundleActivate: vi.fn(() => Promise.resolve()),
    bundleList: vi.fn(() =>
      Promise.resolve({
        active: null,
        history: [],
        policy: { mode: "auto", channel: "stable" },
      }),
    ),
    bundleAvailable: vi.fn(() => Promise.resolve([])),
  };
}
