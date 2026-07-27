import { describe, it, expect } from 'vitest';
import {
    createTestContext,
    createMutableTestContext,
    type AgentContext,
} from './agent-context';

describe('createTestContext', () => {
    it('defaults mirror the component initial state', () => {
        const ctx = createTestContext();

        // These are the values +page.svelte starts with. If the component's
        // defaults change and these do not, every test using this double is
        // asserting against a world that no longer exists.
        expect(ctx.config.name).toBe('');
        expect(ctx.config.smartRouting).toBe(false);
        expect(ctx.config.privacyMode).toBe(false);
        expect(ctx.lang).toBe('es');
        expect(ctx.personality).toBe('balanced');
        expect(ctx.verifierMode).toBe('off');
        expect(ctx.hostName).toBe('---');
        expect(ctx.activeTabId).toBeNull();
        expect(ctx.sessionSpendUsd).toBe(0);
        expect(ctx.cockpitUi).toBe(true);
    });

    it('applies overrides without disturbing the rest', () => {
        const ctx = createTestContext({ lang: 'en', personality: 'concise' });

        expect(ctx.lang).toBe('en');
        expect(ctx.personality).toBe('concise');
        expect(ctx.verifierMode).toBe('off');
    });

    it('models a headless caller: no cockpit, no active tab', () => {
        const ctx = createTestContext({ cockpitUi: false, activeTabId: null });

        expect(ctx.cockpitUi).toBe(false);
        expect(ctx.activeTabId).toBeNull();
    });

    it('carries unnamed config keys through the index signature', () => {
        const ctx = createTestContext({ config: { name: 'Ada', experimentalFlag: true } });

        expect(ctx.config.name).toBe('Ada');
        expect(ctx.config.experimentalFlag).toBe(true);
    });
});

describe('createMutableTestContext — live reads', () => {
    it('reflects a value changed AFTER the context was created', () => {
        // This is the property the whole port rests on. The production binding
        // uses getters so each access re-reads the component variable. A double
        // that froze its values would let a regression to a plain snapshot pass
        // unnoticed.
        const { ctx, set } = createMutableTestContext({ sessionSpendUsd: 0 });

        expect(ctx.sessionSpendUsd).toBe(0);
        set({ sessionSpendUsd: 4.25 });
        expect(ctx.sessionSpendUsd).toBe(4.25);
    });

    it('supports the spend-cap scenario end to end', () => {
        // The cap fires by comparing the LIVE session total against the limit.
        // Under a snapshot context the total would stay at its turn-start value
        // and the cap would never trigger — the exact bug getters prevent.
        const cap = 5;
        const { ctx, set } = createMutableTestContext({ sessionSpendUsd: 0 });
        const capReached = () => cap > 0 && ctx.sessionSpendUsd >= cap;

        expect(capReached()).toBe(false);
        set({ sessionSpendUsd: 2.1 });
        expect(capReached()).toBe(false);
        set({ sessionSpendUsd: 5.0 });
        expect(capReached()).toBe(true);
    });

    it('tracks a host switch mid-turn', () => {
        const { ctx, set } = createMutableTestContext();

        expect(ctx.hostName).toBe('---');
        set({ hostName: 'PROD-AD-01' });
        expect(ctx.hostName).toBe('PROD-AD-01');
    });

    it('tracks MCP servers being reloaded mid-turn', () => {
        const { ctx, set } = createMutableTestContext({ mcpServers: [] });

        expect(ctx.mcpServers).toHaveLength(0);
        set({ mcpServers: [{ name: 'files' }, { name: 'github' }] });
        expect(ctx.mcpServers.map((s: any) => s.name)).toEqual(['files', 'github']);
    });

    it('exposes every member as a getter, not a data property', () => {
        const { ctx } = createMutableTestContext();

        for (const key of Object.keys(ctx)) {
            const d = Object.getOwnPropertyDescriptor(ctx, key);
            expect(d?.get, `${key} must be a getter`).toBeTypeOf('function');
            expect(d?.value, `${key} must not be a frozen value`).toBeUndefined();
        }
    });

    it('starts from the same defaults as createTestContext', () => {
        const { ctx } = createMutableTestContext();
        const plain = createTestContext();

        expect(ctx.lang).toBe(plain.lang);
        expect(ctx.personality).toBe(plain.personality);
        expect(ctx.cockpitUi).toBe(plain.cockpitUi);
    });
});

describe('AgentContext shape', () => {
    it('is satisfiable by a getter-backed object (the production binding)', () => {
        // Mirrors how +page.svelte binds the port: getters over live variables.
        let lang = 'es';
        const bound: AgentContext = {
            config: { name: 'Iván' },
            get lang() { return lang; },
            personality: 'balanced',
            subAgentModel: '',
            verifierMode: 'off',
            hostName: '---',
            activeTabId: 'tab-1',
            sessionSpendUsd: 0,
            mcpServers: [],
            mcpSecrets: {},
            cockpitUi: true,
        };

        expect(bound.lang).toBe('es');
        lang = 'en';
        expect(bound.lang).toBe('en');
    });
});
