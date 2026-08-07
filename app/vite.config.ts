import { fileURLToPath, URL } from "node:url";
import tailwindcss from "@tailwindcss/vite";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) },
  },
  build: {
    // The page is embedded in the binary and served from localhost, so a
    // single file beats many small requests; the size budget is the binary's,
    // not a network's.
    chunkSizeWarningLimit: 1200,
  },
});
