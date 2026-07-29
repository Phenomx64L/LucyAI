// ── format-metrics.ts — display formatting for the Dashboard's metric chips ──
//
// Pure functions, extracted from CockpitDashboard.svelte so their boundaries
// are testable. Both exist because of the same class of defect: a value that
// was measured correctly and then rounded into uselessness at the last step.
// The interesting cases are all at the unit switch, which is exactly where a
// reader's intuition and the code disagree.

export interface Rate {
    /** The number to print. String when it carries a fixed decimal. */
    n: number | string;
    /** Unit label to print next to it. */
    u: 'kbps' | 'Mbps';
}

/**
 * Throughput in a unit that can actually represent the value.
 *
 * Mbps at one decimal has a floor of 0.05 Mbps. An idle desktop sits an order
 * of magnitude below that — measured 0.005 down / 0.062 up on a real host — so
 * the panel read "↓ 0.0 ↑ 0.0" permanently and looked broken rather than quiet.
 *
 * The backend already keeps 3 decimals (`system.rs`, and the remote sampler in
 * CockpitDashboard) specifically so this function has 1 kbps to work with. If
 * either side drops back to 2 decimals the display silently re-flattens: the
 * value would still be "correct", just always a multiple of 10 kbps.
 */
export function fmtRate(mbps: unknown): Rate {
    const v = Number(mbps);
    if (!Number.isFinite(v) || v <= 0) return { n: 0, u: 'kbps' };
    if (v < 1) return { n: Math.round(v * 1000), u: 'kbps' };
    return { n: v.toFixed(1), u: 'Mbps' };
}

/**
 * Uptime at the resolution the question needs.
 *
 * The health payload carries `uptime_h` as whole hours, and that is what used
 * to be displayed. Below an hour it renders "0 h", which reads identically to
 * "unknown" — and that is precisely the window where an operator is asking
 * "did this box just reboot?". Verified against a host at 1.69 h: the old path
 * printed "1 h" and, an hour earlier, "0 h".
 *
 * `uptime_s` is preferred. Remote hosts (`hosts.rs`) still report only hours,
 * so the fallback is not dead code.
 */
export function fmtUptime(health: { uptime_s?: unknown; uptime_h?: unknown } | null | undefined): string {
    const s = Number(health?.uptime_s);
    if (!Number.isFinite(s) || s < 0) {
        const h = Number(health?.uptime_h);
        return `${Number.isFinite(h) && h >= 0 ? h : 0} h`;
    }
    // Never round a just-booted machine up to a whole minute: "<1 min" is a
    // different fact from "1 min" when you are diagnosing a reboot loop.
    if (s < 60) return '<1 min';
    const m = Math.floor(s / 60);
    if (m < 60) return `${m} min`;
    const hrs = Math.floor(m / 60);
    if (hrs < 24) return m % 60 ? `${hrs} h ${m % 60} min` : `${hrs} h`;
    const d = Math.floor(hrs / 24);
    return hrs % 24 ? `${d} d ${hrs % 24} h` : `${d} d`;
}
