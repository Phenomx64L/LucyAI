// vitest.config.js — vitest setup for Lucy.
//
// Keeps the production build path (vite.config.js) untouched. Tests live
// next to their source as `*.test.ts`.
//
// Coverage is opt-in via `npm test -- --coverage` to keep the default run fast.
//
// Sprint 5 follow-up: registers the Svelte plugin so component tests using
// @testing-library/svelte (e.g. StatusBar.test.ts) can mount real components.
// Files that import .svelte modules MUST set `// @vitest-environment jsdom`
// at the top — see StatusBar.test.ts for the pattern. Default env stays
// 'node' so the bulk of the suite (pure logic) keeps its fast startup.

import { defineConfig } from 'vitest/config';
import { svelte } from '@sveltejs/vite-plugin-svelte';

export default defineConfig({
    plugins: [svelte({ hot: false })],
    resolve: {
        alias: {
            // Match SvelteKit's default $lib alias so test files can use the
            // same import paths as the app code.
            $lib: new URL('./src/lib', import.meta.url).pathname,
        },
        // Svelte 5 ships dual server/browser builds. Without `browser` here
        // vitest picks the SSR variant and `mount()` throws
        // lifecycle_function_unavailable. The `conditions` array tells the
        // resolver to prefer browser exports when both are advertised by a
        // package's `exports` field.
        conditions: ['browser'],
    },
    test: {
        include: ['src/lib/**/*.test.ts'],
        environment: 'node',
        isolate: true,
        reporters: ['default'],
        // v1.4.15 — polyfills window.matchMedia for jsdom suites that
        // load components using svelte/motion (tweened, spring). Must
        // run before ES import hoisting so module-load-time MediaQuery
        // construction inside StatusBar.svelte doesn't blow up.
        setupFiles: ['./vitest.setup.ts'],
    },
});
