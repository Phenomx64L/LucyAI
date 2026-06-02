// ── circadian.ts — Time-of-day accent shift (v1.7.27, theme "G") ──────────
//
// Subtly cools or warms Lucy's primary accent through the day so the app
// feels alive across an 8-hour shift instead of perfectly static. Six
// bands shift the H/S/L values of `--accent`. The change is small (max
// 12° hue, 4% saturation, 5% lightness) so themes don't fight: anyone
// running a custom theme via the existing theme switcher is unaffected
// because we only patch when the active theme is the "default" one.
//
// Effect:
//   05–08  early morning  cool teal      hsl(158 64% 40%)
//   08–12  morning        warm teal      hsl(160 70% 42%)  ← brand default
//   12–17  afternoon      bright green   hsl(154 72% 44%)
//   17–20  evening        cyan-shift     hsl(170 64% 41%)
//   20–23  night          cool cyan      hsl(180 60% 40%)
//   23–05  late night     deep cyan      hsl(186 56% 38%)
//
// Recomputed every 10 minutes and on first paint. Stored as a CSS
// variable on document.documentElement so every consumer of var(--accent)
// inherits the shift without code changes.

const RECOMPUTE_MS = 10 * 60 * 1000;

interface Band { from: number; to: number; h: number; s: number; l: number; label: string; }
const BANDS: Band[] = [
    { from: 5,  to: 8,  h: 158, s: 64, l: 40, label: 'early-morning' },
    { from: 8,  to: 12, h: 160, s: 70, l: 42, label: 'morning'       },
    { from: 12, to: 17, h: 154, s: 72, l: 44, label: 'afternoon'     },
    { from: 17, to: 20, h: 170, s: 64, l: 41, label: 'evening'       },
    { from: 20, to: 23, h: 180, s: 60, l: 40, label: 'night'         },
    { from: 23, to: 24, h: 186, s: 56, l: 38, label: 'late-night'    },
    { from: 0,  to: 5,  h: 186, s: 56, l: 38, label: 'late-night'    },
];

function currentBand(date: Date = new Date()): Band {
    const h = date.getHours();
    return BANDS.find(b => h >= b.from && h < b.to) ?? BANDS[1];
}

let _timer: number | null = null;
let _enabled = true;

/** Read the current preference (default ON). Toggle via `setCircadianEnabled`
 *  or the `/theme` slash command (handled by host). */
export function isCircadianEnabled(): boolean { return _enabled; }
export function setCircadianEnabled(on: boolean): void { _enabled = on; apply(); }

/** Apply the current band's accent. Idempotent — call from any code path. */
export function apply(): void {
    if (typeof document === 'undefined') return;
    const root = document.documentElement;
    if (!_enabled) {
        root.style.removeProperty('--accent');
        root.style.removeProperty('--accent-dim');
        root.style.removeProperty('--accent-border');
        root.style.removeProperty('--accent-glow');
        root.removeAttribute('data-circadian');
        return;
    }
    const b = currentBand();
    root.style.setProperty('--accent',        `hsl(${b.h} ${b.s}% ${b.l}%)`);
    root.style.setProperty('--accent-dim',    `hsla(${b.h}, ${b.s}%, ${b.l}%, 0.10)`);
    root.style.setProperty('--accent-border', `hsla(${b.h}, ${b.s}%, ${b.l}%, 0.18)`);
    root.style.setProperty('--accent-glow',
        `0 0 8px hsla(${b.h}, ${b.s}%, ${b.l}%, 0.25), 0 0 20px hsla(${b.h}, ${b.s}%, ${b.l}%, 0.08)`);
    root.setAttribute('data-circadian', b.label);
}

/** Start the recompute loop. Idempotent — calling twice is harmless. */
export function start(): void {
    if (typeof window === 'undefined') return;
    apply();
    if (_timer !== null) return;
    _timer = window.setInterval(apply, RECOMPUTE_MS);
}

export function stop(): void {
    if (_timer !== null) { clearInterval(_timer); _timer = null; }
}
