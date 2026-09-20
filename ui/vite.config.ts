import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
  plugins: [react()],
  // Tauri serves the built frontend from an embedded asset protocol at a
  // non-root base, so absolute paths like /assets/... 404. Relative base
  // makes the emitted asset URLs resolve against the page URL.
  base: "./",
  build: {
    outDir: "dist",
    rollupOptions: {
      input: {
        main: "index.html",
        pill: "pill.html",
      },
    },
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
  },
});
