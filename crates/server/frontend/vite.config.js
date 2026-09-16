import { readFileSync, rmSync, writeFileSync } from "node:fs";
import { join } from "node:path";
import { defineConfig } from "vite";

function resolveDistFile(distDir, href) {
  return join(distDir, href.replace(/^\.\//, "").replace(/^\//, ""));
}

function inlineBuiltAssets() {
  return {
    name: "inline-built-assets",
    closeBundle() {
      const distDir = join(process.cwd(), "dist");
      const htmlPath = join(distDir, "index.html");
      let html = readFileSync(htmlPath, "utf8");
      html = html.replace(/<link rel="modulepreload"[^>]*>/g, "");
      html = html.replace(
        /<script type="module"[^>]*src="([^"]+)"[^>]*><\/script>/g,
        (_match, src) => {
          const code = readFileSync(resolveDistFile(distDir, src), "utf8");
          return `<script type="module">${code}</script>`;
        },
      );
      html = html.replace(
        /<link rel="stylesheet"[^>]*href="([^"]+)"[^>]*>/g,
        (_match, href) => {
          const css = readFileSync(resolveDistFile(distDir, href), "utf8");
          return `<style>${css}</style>`;
        },
      );
      writeFileSync(htmlPath, html);
      rmSync(join(distDir, "assets"), { recursive: true, force: true });
    },
  };
}

export default defineConfig({
  root: ".",
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    cssCodeSplit: false,
    assetsInlineLimit: 0,
    rollupOptions: {
      input: "index.html",
      output: {
        inlineDynamicImports: true,
      },
    },
  },
  plugins: [inlineBuiltAssets()],
});
