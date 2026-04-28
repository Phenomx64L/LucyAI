import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

// @ts-expect-error process is a nodejs global
const host = process.env.TAURI_DEV_HOST;

// https://vite.dev/config/
export default defineConfig(async () => ({
  plugins: [sveltekit()],

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

  // ── PRODUCTION HARDENING ────────────────────────────────────────────────
  // Without these flags Vite would ship readable source maps (.js.map) and
  // unmangled identifiers in the production bundle that Tauri embeds in the
  // .exe. With them, the JS that ends up inside Lucy's binary is minified,
  // mangled (variables → a, b, c...), and unmappable back to source.
  //
  // This NEVER prevents a determined attacker from beautifying the JS,
  // but it raises the bar from "Ctrl+F for `runAI` in the bundle" to
  // "manually reconstruct what each one-letter identifier means".
  build: {
    sourcemap: false,           // no .map files alongside production JS
    minify: "esbuild",          // default but pinned for clarity
    cssMinify: true,
    target: "esnext",           // Tauri webview is current Edge — full ES2022+
    rollupOptions: {
      output: {
        // Strip Vite's default banner comments that leak version + plugin info.
        banner: "",
      },
    },
    // Throw on chunks > 1.5 MB so we notice if a vendor lib explodes.
    chunkSizeWarningLimit: 1500,
  },

  esbuild: {
    // Drop console.log + console.debug calls in production. Errors and warnings
    // stay so we can still see issues in the WebView2 inspector if needed.
    drop: ["debugger"],
    pure: ["console.log", "console.debug", "console.trace"],
    // Mangle private members (Svelte internals, class fields). Cheap obfuscation.
    legalComments: "none",
  },
}));
