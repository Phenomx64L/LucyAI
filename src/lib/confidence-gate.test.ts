import { describe, it, expect } from 'vitest';
import { scoreConfidence, renderConfidenceBadge } from './confidence-gate';

describe('confidence-gate', () => {
    it('returns low confidence on empty / short answers', () => {
        expect(scoreConfidence('').band).toBe('low');
        expect(scoreConfidence('Sí.').band).toBe('low');
    });

    it('rates a confident answer with code and paths as high', () => {
        const text = `La respuesta es clara: usa el siguiente comando en C:\\Windows\\System32:

\`\`\`powershell
Get-Process | Where-Object {$_.CPU -gt 100}
\`\`\`

Esto está verificado en https://learn.microsoft.com/powershell.`;
        const r = scoreConfidence(text);
        expect(r.band).toBe('high');
        expect(r.score).toBeGreaterThanOrEqual(0.70);
        expect(r.suggestRerun).toBe(false);
    });

    it('flags hedge-heavy answer as low confidence', () => {
        const text = `Creo que probablemente esto podría ser la causa, pero no estoy seguro. Tal vez quieras revisar el log primero. Aparentemente el servicio falla al iniciar, me imagino que es por una dependencia.`;
        const r = scoreConfidence(text);
        expect(r.band).toBe('low');
        expect(r.suggestRerun).toBe(true);
    });

    it('flags explicit failure as low confidence', () => {
        const text = `No encontré ninguna entrada relevante en los logs. Tampoco hay datos en la base de datos sobre ese error. No pude reproducir el problema.`;
        const r = scoreConfidence(text);
        expect(r.band).toBe('low');
        expect(r.suggestRerun).toBe(true);
    });

    it('ignores THOUGHT/TOOL/REMEMBER scaffolding when scoring', () => {
        const text = `<THOUGHT>Tal vez creo que probablemente no sé esto</THOUGHT>La respuesta es: el proceso se llama svchost.exe y está en C:\\Windows.`;
        const r = scoreConfidence(text);
        // Hedges are in THOUGHT only — should NOT pull score down.
        expect(r.band).not.toBe('low');
    });

    it('renders no badge for high confidence', () => {
        const r = scoreConfidence(`La respuesta es definitivamente X, verificado en logs.`);
        expect(renderConfidenceBadge(r)).toBe('');
    });

    it('renders badge for low confidence', () => {
        const r = scoreConfidence(`No encontré nada, tal vez tampoco hay datos.`);
        const html = renderConfidenceBadge(r);
        expect(html).toContain('Confianza');
        expect(html).toContain('confidence-gate-badge');
    });

    it('returns reasons array up to 3 items', () => {
        const r = scoreConfidence(`Creo que probablemente no sé. Tal vez es posible. No encontré datos. Quizá.`);
        expect(r.reasons.length).toBeLessThanOrEqual(3);
    });

    it('bonus for structured headers', () => {
        const a = scoreConfidence(`Esta respuesta tiene contenido pero sin estructura.`);
        const b = scoreConfidence(`Esta respuesta tiene contenido.\n\n## Sección\n\n## Otra sección\n\nMás texto.`);
        expect(b.score).toBeGreaterThan(a.score);
    });
});
