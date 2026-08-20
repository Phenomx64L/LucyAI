// ── constants — pure data extracted from +page.svelte ───────────────────────
//
// All of these are "build-time" values (no component state, no function refs
// from Svelte). Keeping them in a TS module:
//   - shaves ~140 lines off the route component
//   - lets unit tests / future components import them directly
//   - gives TypeScript a chance to type-check shapes (LangSpec, BackupKey, etc.)

// ─────────────────────────────────────────────────────────────────────────────
// SUPPORTED LANGUAGES — drive the UI text (`isEN`) AND the speech-recognition
// language tag for each tab. Order = order shown in the language picker.
// ─────────────────────────────────────────────────────────────────────────────
export interface LangSpec {
    code:  string;   // BCP-47 active tag (used everywhere as "userLang")
    label: string;   // human-readable name for the picker, with flag emoji
    stt:   string;   // SpeechRecognition.lang
    tts:   string;   // SpeechSynthesisUtterance.lang
}

export const LANGS: ReadonlyArray<LangSpec> = [
    { code: 'es-MX', label: '🇲🇽 Español (México)',   stt: 'es-MX', tts: 'es-MX' },
    { code: 'es-ES', label: '🇪🇸 Español (España)',   stt: 'es-ES', tts: 'es-ES' },
    { code: 'en-US', label: '🇺🇸 English (US)',        stt: 'en-US', tts: 'en-US' },
    { code: 'en-GB', label: '🇬🇧 English (UK)',        stt: 'en-GB', tts: 'en-GB' },
    // PORTUGAL Y NO BRASIL, decidido a propósito. La interfaz traducida usa las
    // formas europeas —«ecrã», «utilizador», «definições»— en las dos versiones
    // de Lucy: son unas mil ciento cincuenta frases ya escritas así. La etiqueta
    // decía «Brasil» y el texto era de Portugal, que es la peor de las dos
    // opciones. `stt` y `tts` van con ella para que la voz no quede en otro
    // acento que la interfaz.
    { code: 'pt-PT', label: '🇵🇹 Português (Portugal)', stt: 'pt-PT', tts: 'pt-PT' },
    { code: 'fr-FR', label: '🇫🇷 Français',           stt: 'fr-FR', tts: 'fr-FR' },
    { code: 'de-DE', label: '🇩🇪 Deutsch',            stt: 'de-DE', tts: 'de-DE' },
    { code: 'it-IT', label: '🇮🇹 Italiano',           stt: 'it-IT', tts: 'it-IT' },
    { code: 'ja-JP', label: '🇯🇵 日本語',             stt: 'ja-JP', tts: 'ja-JP' },
    { code: 'zh-CN', label: '🇨🇳 中文 (简体)',         stt: 'zh-CN', tts: 'zh-CN' },
];

// ─────────────────────────────────────────────────────────────────────────────
// BACKUP / RESTORE — keys that are eligible for export to a `.lucybackup` file.
// API keys, passwords, and SSH keys are intentionally excluded; they live in
// the OS keychain and must be re-entered after restore.
// ─────────────────────────────────────────────────────────────────────────────
export const BACKUP_KEYS: ReadonlyArray<string> = [
    'lucy_user_name',
    'lucy_user_lang',
    'lucy_runbooks_dir',
    'lucy_dark',
    'lucy_warp_theme',
    'lucy_quick_actions',
    'lucy_user_chips',
    'lucy_chips_collapsed',
    'lucy_personality',
    'lucy_zoom',
    'lucy_subagent',
    'lucy_verifier_mode',
    'lucy_verifier_model',
    'lucy_skill_favorites',
    'lucy_token_budget',
    'lucy_hosts',
    'lucy_density',
    'lucy_show_tutorial',
    'lucy_workspace_presets',
    'lucy_runbooks',
];

/** Backup envelope schema version. Bump this when restore-time logic changes. */
export const BACKUP_VERSION = 1;

// ─────────────────────────────────────────────────────────────────────────────
// LEGACY ICON MIGRATION — emojis & unicode symbols → palette keys
// Used by the iniciar() boot path to upgrade older `lucy_quick_actions` entries
// in localStorage to the new key-based icon system (see ICON_PALETTE in +page).
// ─────────────────────────────────────────────────────────────────────────────
export const LEGACY_ICON_MAP: Readonly<Record<string, string>> = {
    // unicode symbols (previous migration target)
    '⊡': 'activity', '◉': 'globe', '⊗': 'lock', '≡': 'clipboard', '⊘': 'trash',
    '◈': 'brain',    '⬡': 'shield','⚡': 'bolt', '⚙': 'wrench',  '◑': 'monitor', '◎': 'globe',
    // legacy emojis (pre-2025)
    '🖥️': 'activity', '🖥': 'activity', '🌐': 'globe', '🔒': 'lock', '📋': 'clipboard',
    '🗑️': 'trash',    '🗑': 'trash',    '🧠': 'brain', '🛡️': 'shield', '🛡': 'shield',
    '⚙️': 'wrench',   '📊': 'monitor',   '🔍': 'globe',
};

// ─────────────────────────────────────────────────────────────────────────────
// COST PRICING — must mirror src-tauri/src/utils/db.rs::calculate_cost
// Per-1K-token pricing. Update when providers change rates.
// ─────────────────────────────────────────────────────────────────────────────
export const COST_PER_1K = {
    anthropic_in:  0.003,
    anthropic_out: 0.015,
    openai_gpt4_in:  0.03,
    openai_gpt4_out: 0.06,
    openai_o1_in:    0.015,
    openai_o1_out:   0.06,
    gemini_in:  0.0005,
    gemini_out: 0.0015,
    nvidia_in_avg:  0.0008,
    nvidia_out_avg: 0.0024,
} as const;
