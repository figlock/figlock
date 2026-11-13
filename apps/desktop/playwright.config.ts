import { defineConfig } from "@playwright/test";

// The real UI in Chromium, with the Tauri commands answered by e2e/fake-core.js (FIG-91).
export default defineConfig({
  testDir: "e2e",
  fullyParallel: true,
  forbidOnly: !!process.env.CI,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? [["github"], ["list"]] : "list",
  use: {
    baseURL: "http://127.0.0.1:1420",
    viewport: { width: 1100, height: 760 },
    trace: "retain-on-failure",
  },
  webServer: {
    command: "pnpm exec vite --port 1420 --strictPort --host 127.0.0.1",
    url: "http://127.0.0.1:1420",
    reuseExistingServer: !process.env.CI,
  },
});
