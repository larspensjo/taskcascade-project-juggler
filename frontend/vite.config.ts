import react from "@vitejs/plugin-react";
import { defineConfig, loadEnv } from "vite";

export default defineConfig(({ mode }) => {
  const backendPort =
    loadEnv(mode, ".", "TASKCASCADE_").TASKCASCADE_PORT || "8080";
  return {
    plugins: [react()],
    server: { proxy: { "/api": `http://127.0.0.1:${backendPort}` } },
  };
});
