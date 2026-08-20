// ── i18n.ts — la interfaz de Lucy en cinco idiomas ───────────────────────────
//
// EL ESPAÑOL ES LA CLAVE. No hay identificadores inventados: `trad('Guardar')`
// busca «Guardar» en la tabla y, si no está, devuelve «Guardar». Eso es lo que
// permite convertir la aplicación pantalla por pantalla sin que las que faltan
// se vean rotas — y es lo mismo que ya funciona en el shell nativo.
//
// QUÉ SUSTITUYE. Había `cockpit/i18n.ts` con `t(es, en)`: dos idiomas, leído
// una vez al cargar el módulo, y su propio comentario admitía que cambiar de
// idioma exigía recargar la aplicación. Y en paralelo, dos mil trescientos
// `isEN ? 'English' : 'Español'` repartidos por setenta y cinco ficheros, con
// `isEN` enhebrado como prop por toda la jerarquía de componentes.
//
// El inglés de esos ternarios NO se ha retraducido: está escrito a mano por
// quien conoce el producto, y se ha cosechado tal cual para sembrar la tabla.
// A los modelos solo se les pidió portugués, francés y alemán.
//
// CÓMO SE USA
//   En marcado (reactivo, cambia sin recargar):   {$trad('Guardar')}
//   Con huecos:                                   {$tradf('Faltan {n}', { n: 3 })}
//   En un .ts suelto o dentro de un callback:     tr('Guardar')
//
// La versión de tienda es reactiva y la suelta no. Las dos existen porque los
// dos sitios existen: un `.svelte` se repinta cuando cambia el idioma, y un
// módulo de lógica que arma un mensaje de error no está suscrito a nada.
//
// SE LLAMA `trad` Y NO `t`, que es lo que pedía el dedo. `t` está declarado como
// variable local en veintinueve ficheros —`const t = getTab(tabId)`, cuarenta y
// cuatro veces solo en `+page.svelte`— y es parámetro en otros treinta y dos.
// Una tienda se lee con `$t`, y `$t` dentro de una función donde `t` es otra
// cosa es pelearse con el compilador por un nombre que da igual. `trad` está
// libre en los doscientos dieciocho ficheros; se comprobó antes de elegirlo.

import { derived, get, writable } from 'svelte/store';
import { FRASES } from './i18n-table';

export const IDIOMAS = ['es', 'en', 'pt', 'fr', 'de'] as const;
export type Lang = (typeof IDIOMAS)[number];

/** Cómo se llama cada uno EN SÍ MISMO. Un selector de idioma que dice
 *  «Alemán» solo sirve a quien ya lee español, que es justo quien no lo
 *  necesita. */
export const NOMBRES: Record<Lang, string> = {
    es: 'Español',
    en: 'English',
    pt: 'Português',
    fr: 'Français',
    de: 'Deutsch',
};

const CLAVE_LS = 'lucy_user_lang';

/** Acepta lo que ya hay guardado: 'es', 'en', 'es-MX', 'en-US'. */
export function normaliza(v: string | null | undefined): Lang {
    const dos = (v || '').slice(0, 2).toLowerCase();
    return (IDIOMAS as readonly string[]).includes(dos) ? (dos as Lang) : 'es';
}

function leeGuardado(): Lang {
    try {
        return normaliza(localStorage.getItem(CLAVE_LS));
    } catch {
        // Sin `localStorage` —SSR, o el almacenamiento bloqueado— el español es
        // la respuesta correcta: es el idioma de las claves.
        return 'es';
    }
}

export const lang = writable<Lang>(leeGuardado());

/**
 * Cambia el idioma y lo deja escrito.
 *
 * SE GUARDA EL CÓDIGO CORTO, no `es-MX`. Lo que había mezclaba las dos formas
 * —`SetupOverlay` escribía lo que tuviera el selector y `CockpitConfig` miraba
 * `=== 'en'`— así que un `en-US` guardado por uno hacía que el otro se creyera
 * en español. Se normaliza al leer para que lo viejo siga sirviendo.
 */
export function ponIdioma(l: Lang): void {
    lang.set(l);
    try {
        localStorage.setItem(CLAVE_LS, l);
    } catch {
        /* sin almacenamiento: el cambio vale para esta sesión y ya */
    }
}

function traduce(es: string, l: Lang): string {
    if (l === 'es') return es;
    const fila = FRASES[es];
    if (!fila) return es;
    const i = IDIOMAS.indexOf(l) - 1; // el español es la clave, no una columna
    return fila[i] || es;
}

/**
 * Rellena los huecos con nombre de una plantilla ya traducida.
 *
 * CON NOMBRE Y NO POR POSICIÓN. Al traducir, el hueco se mueve: «hace {n} días»
 * es «vor {n} Tagen» en alemán y «{n} days ago» en inglés, y en portugués la
 * frase entera se reordena. Con `%s` posicionales, la primera traducción que
 * cambie el orden pone el número donde va el nombre.
 *
 * Un hueco que no se pasa se deja TAL CUAL, visible. Borrarlo esconde el fallo
 * justo donde hay que verlo: en la pantalla, en la frase que salió mal.
 */
function rellena(plantilla: string, valores: Record<string, unknown>): string {
    return plantilla.replace(/\{([a-zA-Z_][a-zA-Z0-9_]*)\}/g, (todo, nombre) =>
        Object.prototype.hasOwnProperty.call(valores, nombre)
            ? String(valores[nombre])
            : todo,
    );
}

/** Reactivo. Para el marcado: `{$t('Guardar')}`. */
export const trad = derived(lang, ($l) => (es: string) => traduce(es, $l));

/** Reactivo, con huecos: `{$tf('Faltan {n}', { n: 3 })}`. */
export const tradf = derived(
    lang,
    ($l) =>
        (es: string, valores: Record<string, unknown>): string =>
            rellena(traduce(es, $l), valores),
);

/** No reactivo. Para `.ts` sueltos y para lo que se arma dentro de un callback. */
export function tr(es: string): string {
    return traduce(es, get(lang));
}

export function trf(es: string, valores: Record<string, unknown>): string {
    return rellena(traduce(es, get(lang)), valores);
}

/** Los huecos de una plantilla, para el test que comprueba que no se pierden. */
export function huecos(s: string): string[] {
    return (s.match(/\{[a-zA-Z_][a-zA-Z0-9_]*\}/g) || []).sort();
}
