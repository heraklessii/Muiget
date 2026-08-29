import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri, dev sunucusunu sabit bir portta bekler; port dolusa fail etmeli ki
// Tauri penceresi yanlis bir adrese baglanmasin.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react()],

  // Tauri CLI'in urettigi hatalari gizlememek icin
  clearScreen: false,

  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 1421,
        }
      : undefined,
    watch: {
      // Rust tarafi degisince Vite'in bosuna yeniden derlemesini engelle
      ignored: ["**/src-tauri/**"],
    },
  },
});
