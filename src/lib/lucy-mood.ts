// ── lucy-mood.ts — Living avatar state machine ───────────────────────────
//
// Drives a `data-lucy-mood` attribute on <html> that CSS hooks consume to
// give Lucy an ambient "presence" — subtle visual states tied to what she's
// doing or sensing. Decoupled from `data-state` (which is per-input):
// data-lucy-mood is GLOBAL, persists across UI areas, and signals the
// asistant's overall posture.
//
// States:
//   idle       — default, breathing slowly
//   thinking   — LLM is generating, cyan pulse
//   executing  — running tools/commands, golden shimmer
//   concerned  — anomaly detected, amber border pulse
//   confident  — verification passed, brief green flash
//   listening  — voice input active, pink ring
//
// Transition rules:
//   • Auto-decay back to 'idle' after N seconds if not refreshed
//   • Higher-severity states win (concerned > thinking)
//   • 'confident' is a transient — auto-clears after 1.5s
//
// API is minimal on purpose: setLucyMood(mood, opts?). The store-of-truth
// is the DOM attribute itself — no Svelte store needed; CSS reads it.

export type LucyMood = 'idle' | 'thinking' | 'executing' | 'concerned' | 'confident' | 'listening';

const PRIORITY: Record<LucyMood, number> = {
    idle: 0,
    thinking: 2,
    executing: 3,
    listening: 2,
    concerned: 5,    // highest — interrupts everything else
    confident: 1,    // transient feedback
};

const AUTO_DECAY_MS: Partial<Record<LucyMood, number>> = {
    thinking:  90_000,   // 90s — LLM call should never take this long
    executing: 180_000,  // 3min — long tools allowed
    listening: 30_000,
    confident: 1_500,
    concerned: 30_000,   // anomalies clear when investigation begins
};

let _current: LucyMood = 'idle';
let _decayTimer: ReturnType<typeof setTimeout> | null = null;

/** Read the current mood. */
export function getLucyMood(): LucyMood {
    return _current;
}

/**
 * Set Lucy's ambient mood. Higher-priority states pre-empt lower ones,
 * but you can pass `{ force: true }` to override the priority check
 * (useful when the previous state is conceptually done, e.g. moving from
 * 'thinking' to 'idle' even though idle has lower priority).
 */
export function setLucyMood(mood: LucyMood, opts?: { force?: boolean }): void {
    const force = opts?.force === true;
    if (!force && PRIORITY[mood] < PRIORITY[_current]) {
        // Lower-priority states don't preempt higher ones.
        // Caller must explicitly clear (force) the higher state first.
        return;
    }
    _current = mood;
    try { document.documentElement.dataset.lucyMood = mood; } catch {}

    if (_decayTimer) { clearTimeout(_decayTimer); _decayTimer = null; }
    const decay = AUTO_DECAY_MS[mood];
    if (decay && mood !== 'idle') {
        _decayTimer = setTimeout(() => {
            // Decay back to idle only if still in this state
            if (_current === mood) setLucyMood('idle', { force: true });
        }, decay);
    }
}

/** Initialize — sets idle on <html>. Call once at app start. */
export function startLucyMood(): void {
    setLucyMood('idle', { force: true });
}

/** Convenience: pulse 'confident' briefly then auto-decay. */
export function flashConfident(): void {
    setLucyMood('confident', { force: true });
}

/** Convenience: clear 'concerned' once user has acknowledged. */
export function clearConcern(): void {
    if (_current === 'concerned') setLucyMood('idle', { force: true });
}
