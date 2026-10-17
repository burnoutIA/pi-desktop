import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  // App de bureau chargée depuis le disque : un paquet de ~1 Mo (xterm.js, highlight.js) ne pose pas de problème.
  build: { target: "es2022", chunkSizeWarningLimit: 1500 },
});
