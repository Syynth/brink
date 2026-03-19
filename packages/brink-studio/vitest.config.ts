import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  test: {
    environment: "jsdom",
  },
  resolve: {
    alias: {
      "brink-web": resolve(__dirname, "src/__mocks__/brink-web.ts"),
    },
  },
});
