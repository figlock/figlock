import tailwindcss from "@tailwindcss/vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";
import { svelteTesting } from "@testing-library/svelte/vite";
import { defineConfig } from "vitest/config";

// Tauri loads the built files from dist; in dev it points the window at this server.
export default defineConfig({
  plugins: [tailwindcss(), svelte(), svelteTesting()],
  clearScreen: false,
  server: { port: 1420, strictPort: true, host: "127.0.0.1" },
  build: { target: "safari16", outDir: "dist", emptyOutDir: true },
  test: { environment: "jsdom", include: ["src/**/*.test.ts"] },
});
