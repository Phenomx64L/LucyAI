// ── smart-router.test.ts ──────────────────────────────────────────────────
//
// Tier B #1 — Regression guards for cost-aware routing additions:
//   • Economy mode demotes borderline tier-4 prompts to fast tier.
//   • Heavy-keyword alone (without large context) doesn't trigger Opus.
//   • Savings estimate is positive when downgrading, zero on same tier.
//
// We don't test the entire decision tree here — pre-existing routing
// behaviour is covered by manual QA. New code paths get pinned.

import { describe, it, expect } from 'vitest';
import { routeModel, detectHeavyPrompt, classifyRoutingIntent, type RoutingContext } from './smart-router';

function ctx(overrides: Partial<RoutingContext> = {}): RoutingContext {
    return {
        prompt: '',
        contextTokens: 200,
        ollamaOnline: false,
        smartRoutingEnabled: true,
        privacyMode: false,
        ...overrides,
    };
}

describe('routeModel — Tier B #1 economy mode', () => {
    it('routes a short audit keyword + small context to fast tier when economyMode=true', () => {
        // The keyword "audit" alone used to promote to Opus (tier 4). With
        // economy mode, short context (<400 tokens) demotes it back.
        const d = routeModel(ctx({
            prompt: 'audit my system',
            contextTokens: 150,
            economyMode: true,
        }));
        expect(d.tier).toBe(5);
        expect(d.reason).toMatch(/default fast tier/);
    });

    it('still promotes to tier 4 on truly heavy signals even in economy mode', () => {
        // Large context (>1500) overrides economy mode — at this size we
        // genuinely need deep reasoning regardless of cost preference.
        const d = routeModel(ctx({
            prompt: 'review this thoroughly',
            contextTokens: 2000,
            economyMode: true,
        }));
        expect(d.tier).toBe(4);
    });

    it('promotes to tier 4 in default mode on the SAME borderline prompt', () => {
        // Confirms the gate change is economyMode-gated, not blanket.
        const d = routeModel(ctx({
            prompt: 'audit my system',
            contextTokens: 150,
            economyMode: false,
        }));
        expect(d.tier).toBe(4);
    });

    it('demotes multi-intent classifier alone when economy mode is on', () => {
        // Default: detectedIntent='multi-intent' was enough to escalate.
        // Economy: needs additional signals.
        const d = routeModel(ctx({
            prompt: 'do two things and tell me the time',
            contextTokens: 200,
            detectedIntent: 'multi-intent',
            economyMode: true,
        }));
        expect(d.tier).toBe(5);
    });
});

describe('routeModel — Tier B #1 savings estimate', () => {
    it('reports positive savings when routing demotes from Opus baseline to Flash', () => {
        const d = routeModel(ctx({
            prompt: 'list files',
            contextTokens: 100,
            costlierBaseline: 'claude-opus-4-7',
        }));
        expect(d.estimatedSavingsUsd).toBeDefined();
        // Opus input is ~5× Sonnet and ~50× Flash. A turn that would have
        // gone to Opus but lands on Flash saves a meaningful (sub-cent but
        // positive) amount even at small contexts.
        expect(d.estimatedSavingsUsd as number).toBeGreaterThan(0);
    });

    it('reports zero savings when baseline equals chosen', () => {
        // Force same tier by passing the fast model as both baseline and
        // the prompt characteristics that the router would pick anyway.
        const d = routeModel(ctx({
            prompt: 'simple question',
            contextTokens: 50,
            costlierBaseline: 'gemini-3.1-flash-lite',
        }));
        expect(d.estimatedSavingsUsd).toBeDefined();
        // The router picks gemini-3.5-flash by default; pricing differs
        // slightly so we accept anything within a tenth of a cent.
        expect(Math.abs(d.estimatedSavingsUsd as number)).toBeLessThan(0.001);
    });

    it('omits savings field when caller did not pass a baseline', () => {
        const d = routeModel(ctx({ prompt: 'hello', contextTokens: 30 }));
        expect(d.estimatedSavingsUsd).toBeUndefined();
    });
});

// ── v1.4.5: Spanish/Portuguese keywords + subtask density ────────────────
describe('routeModel — v1.4.5 multi-language + multi-task heuristics', () => {
    function ctx(p: Partial<RoutingContext>): RoutingContext {
        return {
            prompt: p.prompt ?? '',
            contextTokens: p.contextTokens ?? 100,
            ollamaOnline: false,
            smartRoutingEnabled: true,
            ...p,
        };
    }

    it('routes Spanish "auditoría" prompt to heavy tier', () => {
        const d = routeModel(ctx({
            prompt: 'Hazme una auditoría de seguridad del equipo',
        }));
        expect(d.tier).toBe(4);
        expect(d.modelId).toMatch(/sonnet|opus/);
    });

    it('routes Spanish "informe ejecutivo" prompt to heavy tier', () => {
        const d = routeModel(ctx({
            prompt: 'Genera un informe ejecutivo para el CISO',
        }));
        expect(d.tier).toBe(4);
    });

    it('routes Portuguese "auditoria" prompt to heavy tier', () => {
        const d = routeModel(ctx({
            prompt: 'Faz uma auditoria de compliance do servidor',
        }));
        expect(d.tier).toBe(4);
    });

    it('routes a 5-subtask enumeration to heavy tier even without heavy keywords', () => {
        const d = routeModel(ctx({
            prompt: 'Quiero el estado de parches, software instalado, ' +
                    'puertos abiertos, servicios anómalos, usuarios con privilegios, ' +
                    'y un reporte en PDF',
        }));
        expect(d.tier).toBe(4);
        expect(d.reason).toMatch(/subtasks/);
    });

    it('does NOT promote a single short request to heavy tier', () => {
        const d = routeModel(ctx({ prompt: 'lista los procesos abiertos' }));
        expect(d.tier).not.toBe(4);
    });

    it('economy mode requires 5+ subtasks (not 4) to promote', () => {
        const fourTasks = 'estado de parches, software instalado, puertos abiertos, servicios anómalos';
        const d4 = routeModel(ctx({ prompt: fourTasks, economyMode: true }));
        expect(d4.tier).not.toBe(4);
        const fiveTasks = fourTasks + ', usuarios con privilegios';
        const d5 = routeModel(ctx({ prompt: fiveTasks, economyMode: true }));
        expect(d5.tier).toBe(4);
    });
});

describe('detectHeavyPrompt — UI nudge helper', () => {
    it('returns null for short trivial prompts', () => {
        expect(detectHeavyPrompt('hello')).toBeNull();
        expect(detectHeavyPrompt('lista los procesos')).toBeNull();
    });

    it('flags audit keyword', () => {
        const r = detectHeavyPrompt('Necesito un informe ejecutivo de auditoría de seguridad para el CISO');
        expect(r).toMatch(/análisis|informe|audit/i);
    });

    it('flags multi-subtask prompts', () => {
        const r = detectHeavyPrompt(
            'Quiero estado de parches, software instalado, ' +
            'puertos abiertos, servicios anómalos, usuarios con privilegios'
        );
        expect(r).toMatch(/subtareas/);
    });

    it('flags very large context', () => {
        const r = detectHeavyPrompt('do the thing please, very long context'.repeat(20), 4000);
        expect(r).toMatch(/contexto grande/);
    });

    it('returns null when prompt is generic', () => {
        const r = detectHeavyPrompt('cómo está el clima hoy y qué hora es', 50);
        expect(r).toBeNull();
    });
});

// ── v1.7.231 #9 — intent classifier feeding the router's local-sizing ──────
describe('classifyRoutingIntent', () => {
    it('labels short greetings / trivia as short-greeting', () => {
        expect(classifyRoutingIntent('hola')).toBe('short-greeting');
        expect(classifyRoutingIntent('gracias!')).toBe('short-greeting');
        expect(classifyRoutingIntent('qué hora es')).toBe('short-greeting');
        expect(classifyRoutingIntent('how are you?')).toBe('short-greeting');
    });

    it('does NOT treat a long task that merely starts with "ok" as a greeting', () => {
        const long = 'ok ahora audita el equipo completo y genera un informe ejecutivo en PDF con todo';
        expect(classifyRoutingIntent(long)).not.toBe('short-greeting');
    });

    it('labels code-generation requests as code-gen', () => {
        expect(classifyRoutingIntent('dame un script en python que liste procesos')).toBe('code-gen');
        expect(classifyRoutingIntent('write a powershell script to free disk space')).toBe('code-gen');
        expect(classifyRoutingIntent('crea una función de bash para backups')).toBe('code-gen');
    });

    it('labels multi-line log / stack-trace pastes as log-paste', () => {
        const log = [
            '2026-06-25 10:21:03 INFO starting service',
            '2026-06-25 10:21:04 WARN retry 1',
            '2026-06-25 10:21:05 ERROR connection refused',
            '  at net.Socket.connect (net.js:1141)',
            '  at TCPConnectWrap.afterConnect (net.js:1207)',
            '2026-06-25 10:21:06 FATAL giving up',
            'process exited with code 1',
        ].join('\n');
        expect(classifyRoutingIntent(log)).toBe('log-paste');
    });

    it('returns undefined for ambiguous prompts so router heuristics still run', () => {
        expect(classifyRoutingIntent('explícame cómo funciona DNS')).toBeUndefined();
        expect(classifyRoutingIntent('ls -la /var/log')).toBeUndefined(); // shell → router heuristic
        expect(classifyRoutingIntent('')).toBeUndefined();
    });

    // Over-match guards (adversarial review). A heavy/analysis task that merely
    // OPENS with a greeting-collision word must NOT be downgraded to a greeting.
    it('does NOT classify a heavy task that starts with a greeting word as a greeting', () => {
        for (const p of [
            'date: audit the patch compliance for all 12 hosts',
            'test and review the new firewall ruleset for prod',
            'status: analyze the prod incident root cause now',
            'time to diagnose the latency to the prod-db cluster',
        ]) {
            expect(classifyRoutingIntent(p)).not.toBe('short-greeting');
        }
    });

    it('defers shell-verb diagnostics (ping/test-connection) to the router heuristic', () => {
        expect(classifyRoutingIntent('ping 8.8.8.8')).toBeUndefined();
        expect(classifyRoutingIntent('ping prod-db-01.internal -c 100')).toBeUndefined();
        expect(classifyRoutingIntent('test-connection prod-db')).toBeUndefined();
    });

    it('does NOT over-fire code-gen on non-code prose', () => {
        expect(classifyRoutingIntent('make this code review faster please')).not.toBe('code-gen'); // review = heavy
        expect(classifyRoutingIntent('create a new user account in the python team')).not.toBe('code-gen'); // filler too long
    });
});

// ── #9 wiring: the router right-sizes the LOCAL model by intent (privacy) ───
describe('routeModel — intent-driven local sizing (privacy mode)', () => {
    const base = (prompt: string): RoutingContext => ({
        prompt,
        contextTokens: 0,
        ollamaOnline: true,
        // a small general model + a larger code-tuned model (enrichLocalModel
        // fills sizeBillion/capabilities from the id inside the router)
        localModels: [{ id: 'llama3.2:3b' }, { id: 'qwen2.5-coder:7b' }],
        smartRoutingEnabled: true,
        privacyMode: true,
        detectedIntent: classifyRoutingIntent(prompt),
    });

    it('routes a greeting to the SMALLEST local model', () => {
        const d = routeModel(base('hola'));
        expect(d.modelId).toBe('local-llama3.2:3b');
    });

    it('routes a code-gen request to the CODER local model', () => {
        const d = routeModel(base('dame un script en python que liste procesos'));
        expect(d.modelId).toBe('local-qwen2.5-coder:7b');
    });

    it('still resolves to a local model when the intent is omitted (regression guard)', () => {
        const d = routeModel({ ...base('hola'), detectedIntent: undefined });
        expect(d.modelId.startsWith('local-')).toBe(true);
    });
});
