import vue from "@vitejs/plugin-vue";
import { existsSync, readFileSync } from "node:fs";
import { join, normalize, sep } from "node:path";
import { fileURLToPath, URL } from "node:url";
import { defineConfig } from "vite";

const host = process.env.TAURI_DEV_HOST;
const materialIconsDir = fileURLToPath(new URL("./node_modules/vscode-material-icons/generated/icons", import.meta.url));
const materialIconsPublicPath = "/assets/material-icons/";
const materialIconAssetNames = [
  "file",
  "document",
  "folder",
  "folder-open",
  "folder-root",
  "folder-root-open",
  "image",
  "video",
  "audio",
  "zip",
  "pdf",
  "word",
  "powerpoint",
  "table",
  "lock",
  "key",
  "certificate",
  "font",
  "exe",
  "dll",
  "console",
  "powershell",
  "log",
  "settings",
  "json",
  "yaml",
  "xml",
  "toml",
  "markdown",
  "html",
  "css",
  "sass",
  "less",
  "javascript",
  "typescript",
  "typescript-def",
  "javascript-map",
  "css-map",
  "react",
  "react_ts",
  "vue",
  "nodejs",
  "npm",
  "yarn",
  "pnpm",
  "python",
  "python-misc",
  "go",
  "go-mod",
  "rust",
  "java",
  "javaclass",
  "kotlin",
  "swift",
  "c",
  "cpp",
  "csharp",
  "php",
  "ruby",
  "database",
  "docker",
  "git",
  "nginx",
  "properties",
  "editorconfig",
  "env",
  "ini",
  "shell",
  "makefile",
  "cmake",
  "gradle",
  "protobuf",
  "graphql",
  "svg",
  "csv",
  "text",
  "lib",
];

function materialIconsPlugin() {
  function emitMaterialIcon(iconFileName, prefix = "assets/material-icons") {
    const fullPath = join(materialIconsDir, iconFileName);
    if (!existsSync(fullPath)) return;

    this.emitFile({
      type: "asset",
      fileName: `${prefix}/${iconFileName}`,
      source: readFileSync(fullPath),
    });
  }

  return {
    name: "material-icons-assets",
    configureServer(server) {
      const iconsRoot = normalize(`${materialIconsDir}${sep}`);

      server.middlewares.use(materialIconsPublicPath, (req, res, next) => {
        const rawUrl = decodeURIComponent((req.url || "").split("?")[0] || "");
        const fileName = rawUrl.replace(/^\/+/, "");
        if (!fileName.endsWith(".svg")) return next();

        const target = normalize(join(materialIconsDir, fileName));
        if (!target.startsWith(iconsRoot) || !existsSync(target)) return next();

        res.setHeader("Content-Type", "image/svg+xml; charset=utf-8");
        res.end(readFileSync(target));
      });
    },
    generateBundle() {
      materialIconAssetNames
        .map((iconName) => `${iconName}.svg`)
        .forEach((iconFileName) => emitMaterialIcon.call(this, iconFileName));
    },
  };
}

function isIgnoredRollupWarning(warning) {
  const id = warning?.id || "";
  return warning?.code === "INVALID_ANNOTATION" && id.includes("@vueuse/core");
}

export default defineConfig(async () => ({
  plugins: [
    vue(),
    materialIconsPlugin(),
  ],

  resolve: {
    alias: {
      "@": fileURLToPath(new URL("./src", import.meta.url)),
    },
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
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
      // 3. tell Vite to ignore watching `src-tauri`
      ignored: ["**/src-tauri/**"],
    },
  },
  // Build optimizations for faster startup and better iGPU performance
  build: {
    target: 'esnext',
    cssCodeSplit: false,
    // File manager and Ace language mode chunks are lazy-loaded desktop features.
    chunkSizeWarningLimit: 700,
    reportCompressedSize: false,
    rollupOptions: {
      onwarn(warning, handler) {
        if (isIgnoredRollupWarning(warning)) return;
        handler(warning);
      },
      output: {
        manualChunks: (id) => {
          // Core framework
          if (id.includes('node_modules/vue') || id.includes('node_modules/@vue')) return 'vendor-vue';
          if (id.includes('node_modules/reka-ui')) return 'vendor-reka';
          if (id.includes('node_modules/@vueuse')) return 'vendor-vueuse';
          if (id.includes('node_modules/pinia')) return 'vendor-pinia';
          if (id.includes('node_modules/vee-validate') || id.includes('node_modules/@vee-validate') || id.includes('node_modules/zod')) return 'vendor-form';
          // Xterm is large — isolate
          if (id.includes('node_modules/@xterm')) return 'vendor-xterm';
          // Tauri
          if (id.includes('node_modules/@tauri-apps')) return 'vendor-tauri';
          // Icons
          if (id.includes('node_modules/@lucide') || id.includes('node_modules/vscode-material-icons')) return 'vendor-icons';
          // Tree component
          if (id.includes('node_modules/@he-tree')) return 'vendor-tree';
        },
      },
    },
  },
}));
