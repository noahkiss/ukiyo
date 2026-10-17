import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The Tauri dev window loads this server (`devUrl` in tauri.conf.json). Pinned so another
// project's server never answers in its place.
const PORT = 4823;

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  // fs.allow: the bundled recipe catalogue lives in the repo's catalog/, outside app/.
  server: { port: PORT, strictPort: true, host: "localhost", fs: { allow: [".", "../catalog/recipes"] } },
  preview: { port: PORT, strictPort: true, host: "localhost" },
  build: { outDir: "dist", target: "safari16", emptyOutDir: true },
});
