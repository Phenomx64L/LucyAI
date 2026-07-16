// ── cockpit/i18n.ts — idioma del cockpit (Fase A, v1.7.234) ──────────────────
// Lee el MISMO setting que V1 (`lucy_user_lang`, p.ej. 'es-MX' | 'en-US') una
// vez al cargar el módulo. Limitación conocida: cambiar el idioma en caliente
// requiere recargar la app para que el cockpit lo refleje (V1 sí es reactiva);
// aceptable porque el cambio de idioma es un evento raro.
//
// Uso:  import { t, isEN } from './i18n';   →   t('Hola', 'Hello')

export const isEN: boolean = (() => {
    try {
        return (localStorage.getItem('lucy_user_lang') || 'es').startsWith('en');
    } catch {
        return false;
    }
})();

export const t = (es: string, en: string): string => (isEN ? en : es);
