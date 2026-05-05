// vitest.config.js — minimal vitest setup for Lucy's pure-logic test suite.
//
// We deliberately keep this separate from vite.config.js so the production
// build path is untouched. Tests live next to their source as `*.test.ts`.
//
// Coverage is opt-in via `npm test -- --coverage` to keep the default run fast.

import { defineConfig } from 'vitest/config';

export default defineConfig({
    resolve: {
        alias: {
            // Match SvelteKit's default $lib alias so test files can use the
            // same import paths as the app code.
            $lib: new URL('./src/lib', import.meta.url).pathname,
        },
    },
    test: {
        // Tests live in src/lib/*.test.ts. Vitest picks them up automatically.
        include: ['src/lib/**/*.test.ts'],
        environment: 'node',
        // Each pure-logic module is independent — parallelize by file.
        isolate: true,
        // Sane defaults: hide passing test logs, surface failures only.
        reporters: ['default'],
    },
});
