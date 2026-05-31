// @vitest-environment jsdom
// ── StatusBar.test.ts ─────────────────────────────────────────────────────
//
// Sprint 5 follow-up — Component-level test for the UI-7 cache badge.
// Verifies that the rendered DOM reacts correctly to the cache stats
// returned by the (mocked) Tauri backend.
//
// Why @testing-library/svelte: gives us a real DOM mounted in jsdom, with
// `render()` and `findByTestId()` semantics that survive Svelte's async
// reactivity. Plain `new Component({ target })` would work too but you'd
// have to chase microtasks manually.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';
import { render, waitFor, cleanup } from '@testing-library/svelte';
// v1.4.15 — vitest.setup.ts polyfills window.matchMedia BEFORE any
// hoisted import runs, which is required because StatusBar.svelte
// pulls svelte/motion (constructs MediaQuery at module load).

// ── Mocks ─────────────────────────────────────────────────────────────────
// The Tauri `invoke` shape: StatusBar calls it for prompt_guard_status AND
// get_cache_stats on a timer. We intercept both. Each test installs the
// scenario it wants via mockInvoke.mockImplementation.
const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
    invoke: (cmd: string, args?: unknown) => mockInvoke(cmd, args),
}));

// Svelte stores used inside StatusBar that we don't care about for these
// tests. The real density-mode module reads from localStorage at import
// time — replacing it with a minimal stub keeps jsdom clean.
vi.mock('$lib/density-mode', () => ({
    densityMode: { subscribe: (fn: (v: string) => void) => { fn('focus'); return () => {}; } },
    cycleDensityMode: () => {},
    // v1.5.5 — densityFine slider removed from StatusBar; mock kept tiny.
}));

// Import AFTER the mocks are registered. `default` because StatusBar is a
// Svelte component module.
import StatusBar from './StatusBar.svelte';

// Default invoke router: returns sensible defaults for non-cache calls so
// individual tests only need to override the cache stats they care about.
function defaultInvokeRouter(cacheOverride: Partial<{
    input_tokens_total: number;
    cache_creation_total: number;
    cache_read_total: number;
    calls_with_cache_activity: number;
    calls_total_anthropic: number;
}> = {}) {
    return (cmd: string) => {
        if (cmd === 'prompt_guard_status') {
            return Promise.resolve({ status: 'feature_disabled', note: null });
        }
        if (cmd === 'get_cache_stats') {
            return Promise.resolve({
                input_tokens_total: 0,
                cache_creation_total: 0,
                cache_read_total: 0,
                calls_with_cache_activity: 0,
                calls_total_anthropic: 0,
                ...cacheOverride,
            });
        }
        return Promise.resolve(null);
    };
}

// Minimal props — StatusBar's `appReady && !showSetupOverlay` guard would
// otherwise hide everything we want to test.
const baseProps = {
    hostName: 'test-host',
    lucyConfig: { name: 'Lucy' },
    activeTab: null,
    appVersion: '1.4.0-test',
    appReady: true,
    showSetupOverlay: false,
};

describe('StatusBar — UI-7 cache hit badge', () => {
    beforeEach(() => {
        mockInvoke.mockReset();
    });

    afterEach(() => {
        // Tear down the rendered DOM so the next test's findByTestId doesn't
        // match a leftover badge from the previous render. Without this, the
        // first test's element is whatever wins all subsequent lookups.
        cleanup();
        mockInvoke.mockReset();
    });

    it('hides the badge when no cache activity has occurred', async () => {
        mockInvoke.mockImplementation(defaultInvokeRouter()); // all zeros
        const { queryByTestId } = render(StatusBar, baseProps);

        // Give the onMount poll a tick to land.
        await waitFor(() => {
            expect(mockInvoke).toHaveBeenCalledWith('get_cache_stats', undefined);
        });

        // Badge should NOT render — a fresh session shouldn't show "0% cached".
        expect(queryByTestId('cache-badge-pct')).toBeNull();
    });

    it('renders the badge with the right percentage when cache_read > 0', async () => {
        // 6000 cache_read out of 10000 total = 60% (cok tier).
        mockInvoke.mockImplementation(defaultInvokeRouter({
            input_tokens_total: 4000,
            cache_read_total: 6000,
            calls_with_cache_activity: 3,
            calls_total_anthropic: 5,
        }));

        const { findByTestId } = render(StatusBar, baseProps);

        const badge = await findByTestId('cache-badge-pct');
        expect(badge.textContent).toMatch(/60%/);
        // 60 ≥ 30 → cok class (healthy/green)
        expect(badge.classList.contains('cok')).toBe(true);
    });

    it('uses the yellow tier (cy) for 10–29% hit rate', async () => {
        // 1500 / (8500 + 1500) = 15%
        mockInvoke.mockImplementation(defaultInvokeRouter({
            input_tokens_total: 8500,
            cache_read_total: 1500,
            calls_with_cache_activity: 2,
            calls_total_anthropic: 4,
        }));

        const { findByTestId } = render(StatusBar, baseProps);
        const badge = await findByTestId('cache-badge-pct');
        expect(badge.textContent).toMatch(/15%/);
        expect(badge.classList.contains('cy')).toBe(true);
    });

    it('uses the muted tier (cm) for <10% hit rate', async () => {
        // 200 / (9800 + 200) = 2%
        mockInvoke.mockImplementation(defaultInvokeRouter({
            input_tokens_total: 9800,
            cache_read_total: 200,
            calls_with_cache_activity: 1,
            calls_total_anthropic: 3,
        }));

        const { findByTestId } = render(StatusBar, baseProps);
        const badge = await findByTestId('cache-badge-pct');
        expect(badge.textContent).toMatch(/2%/);
        expect(badge.classList.contains('cm')).toBe(true);
    });
});
