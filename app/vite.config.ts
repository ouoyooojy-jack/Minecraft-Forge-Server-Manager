import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

export default defineConfig({
  plugins: [svelte()],

  // Tauri owns the terminal output; don't let Vite wipe it.
  clearScreen: false,

  server: {
    // Fixed port: tauri.conf.json's devUrl points here.
    port: 1420,
    strictPort: true,
    watch: {
      // Rust changes are cargo's business; watching them just double-reloads.
      ignored: ["**/src-tauri/**"],
    },
  },

  build: {
    // WebView2 tracks Edge, so there is no old-browser tail to support.
    target: "esnext",
    sourcemap: true,
  },
});
