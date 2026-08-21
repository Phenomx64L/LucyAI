// ── app.d.ts — ambient declarations ──────────────────────────────────────────
//
// Lucy installs a handful of globals on `window`: debug hooks, bridges from
// rendered HTML back into the app (an `onclick="_lucyRunFix(…)"` in generated
// markup cannot reach a module-scoped function), and one browser API the TS
// DOM lib still does not ship.
//
// They were undeclared, so every read produced "Property 'X' does not exist on
// type 'Window & typeof globalThis'" — 17 of the errors this file removes. But
// the reason to write it down is not the count: this is the only place that
// says what Lucy's global surface IS. Adding one without adding it here means
// the next reader has to grep for it.
//
// Keep the types conservative. A declaration that overstates what a global
// holds is worse than none — it invents guarantees the assignment site never
// made.

export {};

declare global {
    /**
     * La versión de Lucy, compilada dentro del bundle por Vite desde
     * `package.json`. Ver la nota de `vite.config.js`.
     *
     * NO es `window.algo`: es una sustitución de texto que hace el compilador,
     * así que en el bundle final no queda ninguna variable — queda la cadena.
     * Por eso se declara aquí fuera de `Window`.
     *
     * Existe porque tres componentes llevaban la versión escrita a mano como
     * valor por defecto, con un comentario pidiendo mantenerlos sincronizados.
     * No se mantuvieron.
     */
    const __LUCY_VERSION__: string;

    interface Window {
        /**
         * Ring of UI-freeze samples. Appended by the freeze watchdog and read
         * by `/diag`; `window.__lucyFreezeLog = window.__lucyFreezeLog || []`
         * is the install, so it is only present after the first sample.
         */
        __lucyFreezeLog?: unknown[];

        /**
         * Bridge for the "apply this fix" buttons in generated Lucy markup.
         * The HTML is injected as a string, so its `onclick` cannot close over
         * a module-scoped handler — it calls through `window` instead.
         */
        _lucyRunFix?: (key: string) => Promise<unknown>;

        /** Same bridge shape, for the runbooks-directory picker button. */
        selectRunbooksDir?: () => Promise<unknown>;

        /**
         * Debug handle for the interrupted-agent checkpoint store — lets you
         * inspect and clear stale checkpoints from the devtools console.
         */
        __lucyCheckpoints?: {
            list: () => unknown;
            clear: (tabId: string | number) => unknown;
        };

        /** Memory id the graph view should scroll to on its next mount. */
        _lucyJumpToMemoryId?: string;

        /** NexShell hook: capture evidence when a command looks like an incident. */
        __lucy_capture_evidence?: (...args: any[]) => unknown;

        /**
         * Web Speech API. Still not in TypeScript's DOM lib, and Chromium only
         * exposes the `webkit`-prefixed constructor — hence both, and hence
         * `any`: typing the full SpeechRecognition surface here would be a
         * large fiction for two call sites that feature-detect it anyway.
         */
        SpeechRecognition?: any;
        webkitSpeechRecognition?: any;
    }
}
