import { defineConfig } from "vite";
import vue from "@vitejs/plugin-vue";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const packageVersion = JSON.parse(
  readFileSync(path.join(__dirname, "package.json"), "utf8"),
).version as string;

function gitRevision() {
  try {
    const revision = execFileSync("git", ["rev-parse", "--short=12", "HEAD"], {
      cwd: __dirname,
      encoding: "utf8",
      stdio: ["ignore", "pipe", "ignore"],
    }).trim();
    const isDirty =
      execFileSync("git", ["status", "--porcelain", "--untracked-files=no"], {
        cwd: __dirname,
        encoding: "utf8",
        stdio: ["ignore", "pipe", "ignore"],
      }).trim().length > 0;

    return isDirty ? `${revision}-dirty` : revision;
  } catch {
    return "unknown";
  }
}

// https://vitejs.dev/config/
export default defineConfig({
  plugins: [vue()],
  define: {
    __FONO_FRONTEND_BUILD__: JSON.stringify({
      version: packageVersion,
      revision: gitRevision(),
    }),
  },

  // Tauri production build uses file:// URLs — relative paths required
  base: "./",

  // Tauri expects a fixed port, fail if that port is not available
  clearScreen: false,

  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },

  server: {
    port: 1420,
    strictPort: true,
    watch: {
      // Tell vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },

  // Produce sourcemaps for debug builds
  build: {
    rollupOptions: {
      input: {
        main: path.resolve(__dirname, "index.html"),
        v3: path.resolve(__dirname, "v3.html"),
      },
    },
    target: "es2022",
    sourcemap: !!process.env.TAURI_DEBUG,
    minify: !process.env.TAURI_DEBUG ? "esbuild" : false,
    // Tauri uses Chromium on Windows — modern target is fine
    chunkSizeWarningLimit: 1500,
  },
});
