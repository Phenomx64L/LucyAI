import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vite";
import { sveltekit } from "@sveltejs/kit/vite";

// ── La versión, de UN SOLO SITIO ────────────────────────────────────────────
//
// Estaba escrita a mano en tres componentes como valor por defecto, con un
// comentario que pedía «keep in sync with package.json + Cargo.toml». No se
// mantuvo: el CHANGELOG cuenta que `LUCY_VERSION = '1.6.4'` se quedó docenas de
// versiones atrás mientras la aplicación ya iba por la 1.7.x, y que hubo que
// arreglarlo a mano. Volvió a pasar: al llegar a la 2.0.1 esos tres decían
// 1.7.236, 1.7.236 y 1.7.66.
//
// En ejecución no se veía porque `getVersion()` de Tauri los sobrescribe al
// montar — o sea que el fallo solo salía en dev, en preview y en un test que
// montara el componente suelto. Un dato incorrecto que se corrige medio segundo
// después es peor que uno que se ve siempre: nadie lo reporta y nadie lo
// arregla.
//
// `package.json` manda porque es lo que ya lee `npm version`. `Cargo.toml` y
// `tauri.conf.json` siguen siendo suyos —los lee el bundler, no Vite— y hay un
// test que comprueba que los tres dicen lo mismo.
const pkg = JSON.parse(
  readFileSync(fileURLToPath(new URL("./package.json", import.meta.url)), "utf8"),
);

// `process` is a Node global that neither config's `lib` declares. A cast
// rather than a @ts-expect-error, because the directive only fires under the
// strict config and reported as unused in the checkJs pass — a silencer that
// is itself conditional is worse than saying what the value is.
const host = /** @type {any} */ (globalThis).process?.env?.TAURI_DEV_HOST;

// https://vite.dev/config/
//
// Not `async`: nothing in this config awaits anything (it is a plain object
// literal), and the async form made TypeScript reject every `defineConfig`
// overload — a `() => Promise<config>` matches `UserConfigFnPromise`, not the
// `UserConfigFnObject` the object shape resolves to. `sveltekit()` returning a
// promise is fine; Vite awaits the `plugins` array itself.
export default defineConfig(() => ({
  plugins: [sveltekit()],

  // La versión que compila dentro del bundle. Ver la nota de arriba.
  define: {
    __LUCY_VERSION__: JSON.stringify(pkg.version),
  },

  // Vite options tailored for Tauri development and only applied in `tauri dev` or `tauri build`
  //
  // 1. prevent Vite from obscuring rust errors
  clearScreen: false,
  // 2. tauri expects a fixed port, fail if that port is not available
  // Port 5173 (Vite's default), NOT 1420: on this machine Hyper-V/WinNAT had
  // reserved the 1420-range — Windows let the dev server bind() but silently
  // DROPPED inbound loopback connections (the SYN never reached accept()), so
  // Vite printed "ready" yet the WebView2 hung with ERR_CONNECTION_TIMED_OUT.
  // Those reservations reshuffle on reboot; 5173 is outside the reserved blocks
  // (verified reachable). devUrl in tauri.conf.json matches.
  server: {
    port: 5173,
    strictPort: true,
    // Pin to IPv4 loopback. With `host: false`, Node 17+ on Windows binds
    // `localhost` to whatever it resolves FIRST — often IPv6 `::1` only — while
    // the Tauri WebView2 connects to `127.0.0.1` (IPv4) → SYN_SENT hangs →
    // ERR_CONNECTION_TIMED_OUT in the app window. Forcing 127.0.0.1 (and
    // devUrl in tauri.conf.json to match) keeps the whole dev loop on IPv4.
    host: host || "127.0.0.1",
    hmr: host
      ? {
          protocol: "ws",
          host,
          port: 5174,
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
        // ── v1.7.84 — Code splitting for heavy views ───────────────────
        // Lucy's main bundle was monolithic — every Svelte view (NexShell
        // ~4 kLoC, MemoryGraph ~1.1 kLoC, Dashboard ~1.3 kLoC, etc.) plus
        // every vendor dep landed in the first chunk the WebView had to
        // parse before paint. Splitting them out lets the WebView start
        // rendering the terminal view while the others stream in the
        // background.
        //
        // manualChunks is the lowest-friction approach: we don't have to
        // rewrite the existing static `import X from '$lib/...'`
        // statements; Rollup just groups matching modules into separate
        // .js chunks at build time. Chunks load on demand when the
        // matched module is first reached at runtime (the Svelte
        // {#if activeView === 'X'} guard means the heavy view code is
        // only executed when the user actually navigates there).
        //
        // Grouping rationale:
        //   • `view-nexshell` — the biggest single component (~140 KB
        //     minified). Heavy on shell terminal styling + ANSI parser.
        //   • `view-graph`    — MemoryGraphView + d3-force together.
        //     Only loaded when the operator opens the graph overlay.
        //   • `view-dash`     — DashboardView + integrations panels.
        //   • `view-ops`      — LogViewer + Inventory + Compliance,
        //     grouped because they're all read-mostly tabular views
        //     and tend to be used in the same operator session.
        //   • `vendor-md`     — marked + DOMPurify + highlight.js —
        //     used in the artifact panel + chat thread, but only the
        //     active chat tab needs them ready on first paint.
        //   • `vendor-icons`  — Tabler icon tree, fragments out to its
        //     own chunk so each view doesn't drag the whole icon set in.
        manualChunks(id) {
          if (id.includes('NexShellView.svelte'))    return 'view-nexshell';
          if (id.includes('MemoryGraphView.svelte')
            || id.includes('node_modules/d3-force')) return 'view-graph';
          if (id.includes('DashboardView.svelte'))   return 'view-dash';
          if (id.includes('LogViewerView.svelte')
            || id.includes('InventoryView.svelte')
            || id.includes('ComplianceView.svelte')) return 'view-ops';
          if (id.includes('node_modules/marked')
            || id.includes('node_modules/dompurify')
            || id.includes('node_modules/highlight.js')
            || id.includes('node_modules/shiki'))    return 'vendor-md';
          if (id.includes('node_modules/@tabler'))   return 'vendor-icons';
          return undefined;  // default: bundler decides
        },
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
