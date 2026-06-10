import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  timeout: 15000,
  use: {
    // Dedicated e2e port — never reuse the developer's live dev server
    // (:5180), which may be serving a different checkout's code.
    baseURL: "http://localhost:5190",
    // Pin to the "wide" responsive tier so the 3-pane split layout is active.
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: "pnpm dev --port 5190 --strictPort",
    port: 5190,
    reuseExistingServer: false,
    timeout: 30000,
  },
});
