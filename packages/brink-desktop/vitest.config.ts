import { defineConfig } from "vitest/config";

// Node environment — no DOM needed. The Tauri IPC surfaces themselves
// (menu/window events, `invoke`) are not run headlessly here; only the
// awaitable-save seam (`quit.ts`) is unit-tested. See docs/decision-log.md
// "Desktop close: no dirty prompt; quit awaits the final save" (#2370) for
// why the actual quit path gets a manual-verification note instead.
export default defineConfig({
  test: {
    environment: "node",
  },
});
