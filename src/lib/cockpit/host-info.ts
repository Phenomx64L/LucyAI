// ── host-info.ts — el nombre REAL de la máquina donde corre Lucy ────────────
//
// El cockpit se construyó sobre una maqueta y el nombre del equipo de esa
// maqueta —"PRECISION-X"— quedó escrito a mano en cinco sitios: la píldora y el
// texto de carga de Inventario, tres de Compliance (incluida la cabecera del
// informe exportado) y el pie del shell.
//
// El Dashboard era el único que lo resolvía, y solo porque `apply()` sobrescribe
// su valor inicial con `hostname` del backend. El resto mostraba una constante:
// en una máquina llamada de otra forma, afirmaban con total seguridad el nombre
// equivocado. En un informe de cumplimiento eso no es un detalle estético —
// dice a qué equipo se le pasaron los controles.
//
// Una sola fuente, cacheada: el nombre no cambia mientras la app vive, así que
// se pide una vez y todos leen del mismo store.

import { writable, get } from 'svelte/store';
import { getSystemHealthJson } from '$lib/lucy-api';

/**
 * Nombre del equipo local. Arranca vacío a propósito — un placeholder con
 * aspecto de hostname es exactamente el error que este módulo corrige.
 * Los consumidores deben mostrar un guion o el texto que corresponda mientras
 * llega, nunca un nombre inventado.
 */
export const localHostname = writable<string>('');

let _inFlight: Promise<string> | null = null;

/**
 * Resuelve el nombre real. Idempotente y con deduplicación de peticiones: varios
 * módulos montándose a la vez comparten una sola llamada al backend.
 */
export async function ensureLocalHostname(): Promise<string> {
    const cached = get(localHostname);
    if (cached) return cached;
    if (_inFlight) return _inFlight;

    _inFlight = (async () => {
        try {
            const h: any = await getSystemHealthJson();
            const name = String(h?.hostname || '').trim();
            // '---' es lo que devuelve el backend cuando sysinfo no sabe el
            // nombre; tratarlo como válido reintroduciría el problema en otra forma.
            if (name && name !== '---') {
                localHostname.set(name);
                return name;
            }
        } catch { /* sin backend (preview) — se queda vacío */ }
        return '';
    })();

    try { return await _inFlight; } finally { _inFlight = null; }
}

/** Para mostrar: el nombre real, o el sustituto que decida quien llama. */
export function hostLabel(name: string, fallback = '—'): string {
    return name && name.trim() ? name : fallback;
}
