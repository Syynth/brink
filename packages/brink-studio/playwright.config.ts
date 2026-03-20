import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "e2e",
  timeout: 15000,
  use: {
    baseURL: "http://localhost:5180",
  },
  webServer: {
    command: "pnpm dev",
    port: 5180,
    reuseExistingServer: true,
    timeout: 10000,
  },
});
