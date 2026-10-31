import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

// Set by `tauri dev` when targeting a device on the network; unset on desktop.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  // The React app lives in ui/; this file sits at the repo root so
  // `npm run dev` and `npm run tauri dev` work from there.
  root: "ui",
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    ...(host ? { hmr: { protocol: "ws", host, port: 1421 } } : {}),
    watch: { ignored: ["**/src-tauri/**", "**/target/**", "**/crates/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // The shell is WKWebView, so Safari is the only browser that matters.
    target: "safari16",
    minify: !process.env.TAURI_ENV_DEBUG,
    sourcemap: Boolean(process.env.TAURI_ENV_DEBUG),
  },
});
