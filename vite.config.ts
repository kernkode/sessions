import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri expects a fixed port and fails if it is not available.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: {
    port: 5273,
    strictPort: true,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  build: {
    // Modern WebView2 / WKWebView: no needless transpilation.
    // A single local bundle loads faster than several chunks.
    target: "esnext",
    sourcemap: false,
    chunkSizeWarningLimit: 2048,
  },
});
