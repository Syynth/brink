import { defineConfig } from "@playwright/test";

/**
 * Perf scenario runner (measure-first ruling, docs/decision-log.md
 * 2026-08-24) — DELIBERATELY separate from playwright.config.ts so the
 * normal e2e gate never runs these: perf scenarios are measurement, not
 * pass/fail coverage, and they take minutes. Run via `pnpm test:perf`.
 *
 * Every run writes an artifact directory under `<repo>/perf-runs/`
 * (gitignored): the probe's JSON report, the wasm-internal counters, a CDP
 * trace loadable in DevTools/Perfetto, and a meta.json (commit, scenario,
 * fixture). `scripts/perf-compare.mjs` diffs runs.
 *
 * Single worker + no parallelism: concurrent scenarios would contend for
 * CPU and corrupt each other's timings.
 */
export default defineConfig({
  testDir: "perf",
  timeout: 180_000,
  workers: 1,
  fullyParallel: false,
  // Measurement, not coverage: a flaky wait is a broken scenario — fix it,
  // don't retry it into a misleading artifact.
  retries: 0,
  use: {
    // Own port, distinct from dev (5180) and e2e (5190).
    baseURL: "http://localhost:5195",
    viewport: { width: 1280, height: 800 },
  },
  webServer: {
    command: "pnpm dev --port 5195 --strictPort",
    port: 5195,
    reuseExistingServer: false,
    timeout: 60_000,
  },
});
