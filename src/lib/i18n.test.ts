import { readFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import { afterEach, describe, expect, it } from 'vitest';
import { get } from 'svelte/store';
import { FRASES } from './i18n-table';
import { IDIOMAS, huecos, lang, normaliza, ponIdioma, trad, tradf, tr } from './i18n';

describe('i18n', () => {
    it('devuelve el español cuando la frase no está en la tabla', () => {
        // ESTO ES LO QUE PERMITE CONVERTIR POR PANTALLAS. Una frase sin entrada
        // sale en español, no vacía ni con la clave cruda, así que una pantalla
        // a medio convertir se ve como estaba en vez de rota.
        ponIdioma('de');
        expect(tr('esta frase no existe en ninguna tabla')).toBe(
            'esta frase no existe en ninguna tabla',
        );
        ponIdioma('es');
    });

    it('acepta los códigos viejos que ya hay guardados', () => {
        // `SetupOverlay` escribía lo que tuviera el selector —'es-MX', 'en-US'—
        // y `CockpitConfig` comparaba con `=== 'en'`. Un 'en-US' guardado por
        // uno hacía que el otro se creyera en español.
        expect(normaliza('es-MX')).toBe('es');
        expect(normaliza('en-US')).toBe('en');
        expect(normaliza('pt-BR')).toBe('pt');
        expect(normaliza(null)).toBe('es');
        expect(normaliza('klingon')).toBe('es');
    });

    it('cambiar de idioma es reactivo', () => {
        // La razón de que esto sea una tienda y no una constante leída al
        // cargar el módulo, que es lo que hacía `cockpit/i18n.ts` — y su propio
        // comentario admitía que obligaba a recargar la aplicación.
        const vistos: string[] = [];
        const fin = trad.subscribe((f) => vistos.push(f('Guardar')));
        ponIdioma('en');
        ponIdioma('de');
        fin();
        ponIdioma('es');
        expect(new Set(vistos).size).toBeGreaterThan(1);
    });

    it('los huecos se rellenan por nombre y sobreviven al idioma', () => {
        const f = get(tradf);
        expect(f('Faltan {n} equipos', { n: 3 })).toBe('Faltan 3 equipos');
        // Un hueco sin valor se queda VISIBLE. Borrarlo esconde el fallo justo
        // donde hay que verlo.
        expect(f('Faltan {n} equipos', {})).toBe('Faltan {n} equipos');
    });

    it('ninguna frase se queda a medio traducir', () => {
        const malas: string[] = [];
        for (const [es, fila] of Object.entries(FRASES)) {
            if (fila.length !== IDIOMAS.length - 1 || fila.some((v) => !v || !v.trim())) {
                malas.push(es);
            }
        }
        expect(malas, `${malas.length} sin los cuatro idiomas:\n${malas.slice(0, 10).join('\n')}`)
            .toEqual([]);
    });

    it('las plantillas conservan sus huecos en los cinco idiomas', () => {
        // EL FALLO QUE ESTO EVITA no rompe la compilación ni deja la tabla
        // incompleta: sale en pantalla como una frase a la que le falta el
        // número, o con un `{n}` crudo en medio.
        const rotas: string[] = [];
        for (const [es, fila] of Object.entries(FRASES)) {
            const quiero = huecos(es).join(',');
            fila.forEach((v, i) => {
                if (huecos(v).join(',') !== quiero) {
                    rotas.push(`[${IDIOMAS[i + 1]}] ${es} → ${v}`);
                }
            });
        }
        expect(rotas, `${rotas.length} traducciones con los huecos cambiados:\n${rotas.slice(0, 10).join('\n')}`)
            .toEqual([]);
    });

    it('la clave de cada fila es española de verdad', () => {
        // Una entrada cuya clave ya está en inglés significa que alguien cosechó
        // el ternario al revés, y entonces `t('Save')` funciona por accidente
        // mientras `t('Guardar')` devuelve «Guardar» para siempre.
        const sospechosas = Object.entries(FRASES).filter(
            ([es, fila]) => es === fila[0] && /^[\x20-\x7E]+$/.test(es) && es.split(' ').length > 2,
        );
        expect(
            sospechosas.length,
            `claves iguales a su inglés:\n${sospechosas.slice(0, 8).map(([e]) => e).join('\n')}`,
        ).toBeLessThanOrEqual(40);
    });

    it('toda frase envuelta en $trad está en la tabla', () => {
        // LA GUARDA QUE IMPORTA DE VERDAD, y la que evita la única forma en que
        // este trabajo puede EMPEORAR la aplicación: convertir un
        // `isEN ? 'Save' : 'Guardar'` en `$trad('Guardar')` sin que «Guardar»
        // esté en la tabla. No falla nada, no avisa nada — y quien tenía Lucy
        // en inglés empieza a ver esa frase en español.
        //
        // `trad` devuelve el español cuando no encuentra la clave, que es lo
        // correcto para una pantalla sin convertir. Aquí no: aquí alguien ya
        // decidió que esa frase se traduce.
        const raiz = fileURLToPath(new URL('..', import.meta.url));
        const faltan: string[] = [];
        const vistas = new Set<string>();
        for (const f of ficheros(raiz)) {
            // El propio módulo NO: sus comentarios traen ejemplos de uso
            // —`{$tradf('Faltan {n}', { n: 3 })}`— y un ejemplo no es una
            // llamada. Exigirle entrada obligaría a meter en la tabla una
            // frase que no se pinta en ninguna parte.
            if (f.endsWith('i18n.ts')) continue;
            const s = readFileSync(f, 'utf8');
            for (const m of s.matchAll(/\$?\btradf?\(\s*'((?:[^'\\]|\\.)*)'/g)) {
                const clave = m[1].replace(/\\'/g, "'").replace(/\\\\/g, '\\');
                if (vistas.has(clave)) continue;
                vistas.add(clave);
                if (!FRASES[clave]) faltan.push(`${f.slice(raiz.length)}: ${clave}`);
            }
        }
        // Hay que ver ALGO: si el rastreador deja de encontrar llamadas —porque
        // cambió el nombre, o la extensión, o la ruta— este test pasaría
        // siempre diciendo que todo está bien.
        expect(vistas.size, 'el rastreador no encontró ninguna llamada a trad').toBeGreaterThan(50);
        expect(faltan, `${faltan.length} envueltas sin entrada:\n${faltan.slice(0, 10).join('\n')}`)
            .toEqual([]);
    });

    afterEach(() => ponIdioma('es'));
});

function ficheros(dir: string): string[] {
    const out: string[] = [];
    for (const e of readdirSync(dir, { withFileTypes: true })) {
        const p = join(dir, e.name);
        if (e.isDirectory()) out.push(...ficheros(p));
        else if (/\.(svelte|ts)$/.test(e.name) && !e.name.includes('.test.')) out.push(p);
    }
    return out;
}
