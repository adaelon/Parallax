import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  test: {
    css: true,
  },
  server: {
    strictPort: true,
    host: "127.0.0.1",
    port: 1420,
  },
});
