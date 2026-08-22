import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// In development the Vite server proxies to the Rust API, so the frontend
// keeps HMR and the backend stays a plain local process. In production the
// built bundle is embedded into the diffuse binary.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: {
      "/api": {
        target: "http://127.0.0.1:5177",
        changeOrigin: false,
      },
    },
  },
  build: { outDir: "dist", emptyOutDir: true },
});
