import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// In development, API calls go to a locally running clipdeck-server.
export default defineConfig({
  plugins: [react()],
  server: {
    proxy: { "/api": "http://localhost:8080", "/healthz": "http://localhost:8080" },
  },
  build: { target: "es2022", chunkSizeWarningLimit: 900 },
  worker: { format: "es" },
});
