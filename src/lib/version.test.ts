import { readFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { describe, expect, it } from 'vitest';

const raiz = fileURLToPath(new URL('../../', import.meta.url));
const lee = (p: string) => readFileSync(raiz + p, 'utf8');

describe('la versión de Lucy', () => {
    it('dice lo mismo en los tres sitios que la declaran', () => {
        // TRES Y NO UNA porque cada una la lee una herramienta distinta: npm
        // lee `package.json`, cargo lee `Cargo.toml` y el bundler de Tauri lee
        // `tauri.conf.json` — de ahí saca el número que acaba en el nombre del
        // .msi y en «Programas y características» de Windows.
        //
        // Y DIVERGEN. Al llegar a la 2.0.1, `package.json` y `Cargo.toml` iban
        // por 1.8.0 mientras tres componentes decían 1.7.236, 1.7.236 y 1.7.66.
        // El CHANGELOG cuenta que ya había pasado antes con `LUCY_VERSION`.
        //
        // Lo que se ve cuando fallan no es un error: es un instalador que dice
        // una versión y una aplicación que dice otra, y a partir de ahí ningún
        // reporte de fallo se puede situar en el tiempo.
        const pkg = JSON.parse(lee('package.json')).version as string;
        const conf = JSON.parse(lee('src-tauri/tauri.conf.json')).version as string;
        const cargo = /^version\s*=\s*"([^"]+)"/m.exec(lee('src-tauri/Cargo.toml'))?.[1];

        expect(pkg).toMatch(/^\d+\.\d+\.\d+$/);
        expect(conf, 'tauri.conf.json no coincide con package.json').toBe(pkg);
        expect(cargo, 'src-tauri/Cargo.toml no coincide con package.json').toBe(pkg);
    });

    it('no se ha vuelto a escribir a mano en ningún componente', () => {
        // Los tres fallbacks leen `__LUCY_VERSION__`, que Vite compila desde
        // `package.json`. Si alguien vuelve a poner un literal, este test lo
        // dice ANTES de que se quede atrás — que es lo único que hace falta,
        // porque en ejecución `getVersion()` lo tapa y nadie lo nota.
        const sitios = [
            'src/lib/SetupOverlay.svelte',
            'src/lib/cockpit/CockpitConfig.svelte',
            'src/lib/TutorialOverlay.svelte',
        ];
        const malos: string[] = [];
        for (const f of sitios) {
            for (const m of lee(f).matchAll(
                /(?:LUCY_VERSION|appVersion|currentVersion)\s*=\s*(?:\$state\(\s*)?'(\d[\d.]*)'/g,
            )) {
                malos.push(`${f}: '${m[1]}'`);
            }
        }
        expect(malos, `versión escrita a mano:\n${malos.join('\n')}`).toEqual([]);
    });
});
