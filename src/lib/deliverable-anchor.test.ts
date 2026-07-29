// ── deliverable-anchor.test.ts ────────────────────────────────────────────
//
// v1.8.1 regression net. Pins the behaviour that fixes the observed failure:
// Lucy generated a full system health report, and one turn later answered
// "export this report to PDF" with "I have no report loaded in the context of
// our conversation" — because the report had been evicted by the conversation
// compaction it had itself triggered.

import { describe, it, expect } from 'vitest';
import { buildDeliverableAnchor, DEFAULT_ANCHOR_MAX } from './deliverable-anchor';

const report = 'INFORME DE SALUD DEL SISTEMA\n' + 'Hallazgo relevante. '.repeat(80);

describe('buildDeliverableAnchor', () => {
    it('re-injects a deliverable that fell out of history', () => {
        const out = buildDeliverableAnchor({ text: report }, ['Usuario: exporta este reporte en pdf']);
        expect(out).not.toBe('');
        expect(out).toContain('ÚLTIMO ENTREGABLE');
        expect(out).toContain('INFORME DE SALUD DEL SISTEMA');
    });

    it('stays silent while the deliverable is still verbatim in history', () => {
        // Paying for the same text twice would burn context for nothing.
        const out = buildDeliverableAnchor({ text: report }, ['Lucy: ' + report]);
        expect(out).toBe('');
    });

    it('re-grounds the model with the original request when known', () => {
        const out = buildDeliverableAnchor(
            { text: report, goal: 'utiliza tu skill generating-windows-system-health-and-security-report' },
            [],
        );
        expect(out).toContain('Petición original:');
        expect(out).toContain('generating-windows-system-health');
    });

    it('truncates oversized deliverables and says so', () => {
        const huge = 'x'.repeat(DEFAULT_ANCHOR_MAX + 5_000);
        const out = buildDeliverableAnchor({ text: huge }, []);
        expect(out).toContain('truncado');
        expect(out).toContain((DEFAULT_ANCHOR_MAX + 5_000).toLocaleString());
        // Body capped; the wrapper adds the header/footer on top.
        expect(out.length).toBeLessThan(DEFAULT_ANCHOR_MAX + 600);
    });

    it('does not truncate when the deliverable fits', () => {
        const out = buildDeliverableAnchor({ text: report }, []);
        expect(out).not.toContain('truncado');
    });

    it('returns nothing for absent, blank or whitespace-only deliverables', () => {
        expect(buildDeliverableAnchor(null, [])).toBe('');
        expect(buildDeliverableAnchor(undefined, [])).toBe('');
        expect(buildDeliverableAnchor({ text: '' }, [])).toBe('');
        expect(buildDeliverableAnchor({ text: '   \n  ' }, [])).toBe('');
    });

    it('emits nothing rather than a bodyless stub when the cap is non-positive', () => {
        expect(buildDeliverableAnchor({ text: report }, [], 0)).toBe('');
        expect(buildDeliverableAnchor({ text: report }, [], -10)).toBe('');
    });

    it('tolerates null entries in the history array', () => {
        // `rawContent` is routinely undefined on UI-only messages.
        const out = buildDeliverableAnchor({ text: report }, [null as any, undefined as any, '']);
        expect(out).toContain('ÚLTIMO ENTREGABLE');
    });
});
