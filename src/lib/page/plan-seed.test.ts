import { describe, it, expect } from 'vitest';
import { extractPlanSteps } from './plan-seed';

describe('extractPlanSteps — the failures that motivated it', () => {
    it('REJECTS output-format metadata (the "Formato: Markdown" turn)', () => {
        // Verbatim shape of what the Plan panel showed as the plan for a
        // four-part system diagnostic. Neither line is a step.
        const reasoning = [
            'Voy a preparar el informe.',
            '- Destino: Chat (no se especificó archivo)',
            '- Formato: Markdown',
        ].join('\n');

        expect(extractPlanSteps(reasoning)).toEqual([]);
    });

    it('REJECTS a duplicated pair (the stalled turn)', () => {
        // The loop re-emitted the same two lines; four rows made repetition
        // look like progress.
        const reasoning = [
            '- Leer el estado de los servicios es una acción de solo lectura',
            '- Reiniciar o iniciar servicios REQUIERE permisos de Administrador',
            '- Leer el estado de los servicios es una acción de solo lectura',
            '- Reiniciar o iniciar servicios REQUIERE permisos de Administrador',
        ].join('\n');

        // Two distinct items survive — the plan is real, just not four steps long.
        expect(extractPlanSteps(reasoning)).toEqual([
            'Leer el estado de los servicios es una acción de solo lectura',
            'Reiniciar o iniciar servicios REQUIERE permisos de Administrador',
        ]);
    });

    it('ACCEPTS the real plan from the first successful run', () => {
        const reasoning = [
            'Puntos de datos a recolectar:',
            '- Uso de CPU y RAM actual (sysinfo / PowerShell CIM)',
            '- Top procesos por consumo de recursos (tasklist / PowerShell)',
            '- Conexiones de red activas (Listening / Established)',
            '- Errores del Event Log en las últimas 24 horas (System/Application)',
        ].join('\n');

        expect(extractPlanSteps(reasoning)).toHaveLength(4);
        expect(extractPlanSteps(reasoning)[0]).toContain('CPU y RAM');
    });
});

describe('extractPlanSteps — contiguity', () => {
    it('does not stitch bullets from different sections into one plan', () => {
        // A report's structure is not a sequence of steps. Taking the longest
        // contiguous run keeps the panel from inventing an order.
        const report = [
            'Resumen:',
            '- CPU al 65%',
            '- RAM al 39%',
            '',
            'Prosa intermedia que rompe la lista.',
            '',
            'Hallazgos:',
            '- Windows Update falla',
            '- TPM con errores',
            '- Apagado inesperado',
        ].join('\n');

        const steps = extractPlanSteps(report);
        expect(steps).toEqual(['Windows Update falla', 'TPM con errores', 'Apagado inesperado']);
    });

    it('treats blank lines inside a list as part of the same run', () => {
        const loose = ['- Primer paso', '', '- Segundo paso', '', '- Tercer paso'].join('\n');
        expect(extractPlanSteps(loose)).toHaveLength(3);
    });
});

describe('extractPlanSteps — shape rules', () => {
    it('returns [] for text with no list at all', () => {
        expect(extractPlanSteps('Voy a revisar el equipo y te cuento.')).toEqual([]);
    });

    it('returns [] for a single item — one bullet is not a plan', () => {
        expect(extractPlanSteps('- Revisar servicios')).toEqual([]);
    });

    it('handles null and empty input', () => {
        expect(extractPlanSteps(null)).toEqual([]);
        expect(extractPlanSteps(undefined)).toEqual([]);
        expect(extractPlanSteps('')).toEqual([]);
    });

    it('accepts the supported list styles', () => {
        for (const list of [
            '1. Revisar servicios\n2. Analizar logs',
            '1) Revisar servicios\n2) Analizar logs',
            '1- Revisar servicios\n2- Analizar logs',
            '- Revisar servicios\n- Analizar logs',
            '• Revisar servicios\n• Analizar logs',
            '* Revisar servicios\n* Analizar logs',
        ]) {
            expect(extractPlanSteps(list), list).toHaveLength(2);
        }
    });

    it('does NOT treat "1 - " (space before the dash) as a list item', () => {
        // Pins the narrow reading. Widening it would make "10 - 15 GB de RAM"
        // parse as a step, which is the opposite of what this module is for.
        expect(extractPlanSteps('1 - Revisar servicios\n2 - Analizar logs')).toEqual([]);
    });

    it('strips markdown emphasis and trailing punctuation', () => {
        const steps = extractPlanSteps('- **Revisar** `servicios`.\n- Analizar logs;');
        expect(steps).toEqual(['Revisar servicios', 'Analizar logs']);
    });

    it('drops bare URLs', () => {
        expect(extractPlanSteps('- https://ejemplo.com/doc\n- https://otro.com')).toEqual([]);
    });

    it('caps at 8 steps', () => {
        const many = Array.from({ length: 12 }, (_, i) => `- Paso número ${i + 1}`).join('\n');
        expect(extractPlanSteps(many)).toHaveLength(8);
    });

    it('ignores items outside the 4–120 char bounds', () => {
        const long = 'x'.repeat(200);
        expect(extractPlanSteps(`- abc\n- ${long}\n- Revisar servicios\n- Analizar logs`)).toEqual([
            'Revisar servicios', 'Analizar logs',
        ]);
    });

    it('keeps a metadata-LOOKING step whose prefix is too long to be a key', () => {
        // The metadata rule must not eat real steps that happen to contain a
        // colon. Only a SHORT capitalised prefix reads as `Clave: valor`.
        const steps = extractPlanSteps([
            '- Revisar el estado de los servicios detenidos: listar y clasificar',
            '- Analizar los logs del sistema',
        ].join('\n'));
        expect(steps).toHaveLength(2);
    });
});
