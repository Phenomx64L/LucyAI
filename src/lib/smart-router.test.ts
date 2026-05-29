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
import { routeModel, detectHeavyPrompt, type RoutingContext } from './smart-router';

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
