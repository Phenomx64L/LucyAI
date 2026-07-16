// ── voice.ts — Speech recognition + TTS utilities ────────────────────────────
// Extracted from +page.svelte. All browser APIs; no Tauri deps.

export interface VoiceOpts {
    getActiveLang: () => { stt: string; tts: string };
    getTab:  (id: string) => any;
    addMsg:  (tabId: string, obj: any) => void;
    refresh: () => void;
    toast:   (msg: string, type: string) => void;
}

// ── initRecognition ───────────────────────────────────────────────────────────
// Creates a SpeechRecognition instance wired to the given tab.
// Returns null when the browser/WebView has no SpeechRecognition support.
export function initRecognition(tabId: string, opts: VoiceOpts): any | null {
    const { getActiveLang, getTab, addMsg, refresh } = opts;

    // Multi-prefix for WebView2 compatibility
    const SR: any = (window as any).SpeechRecognition
        || (window as any).webkitSpeechRecognition
        || (window as any).mozSpeechRecognition
        || (window as any).msSpeechRecognition;
    if (!SR) return null;

    const rec = new SR();
    rec.lang = getActiveLang().stt;
    rec.continuous = false;   // false is more stable in WebViews — we restart manually
    rec.interimResults = true;
    rec.maxAlternatives = 1;

    rec.onstart = () => {
        const x = getTab(tabId);
        if (!x) return;
        x.isListening = true;
        x.usedVoice = true;
        if (!x._committed) x._committed = x.inputValue.trim();
        refresh();
    };

    rec.onresult = (ev: any) => {
        const x = getTab(tabId);
        if (!x) return;
        let finalText = '';
        let interimText = '';
        for (let i = ev.resultIndex; i < ev.results.length; i++) {
            const transcript = ev.results[i][0].transcript;
            if (ev.results[i].isFinal) finalText += transcript;
            else interimText += transcript;
        }
        if (finalText) x._committed = ((x._committed || '') + ' ' + finalText).trim();
        x.inputValue = ((x._committed || '') + (interimText ? ' ' + interimText : '')).trim();
        refresh();
    };

    rec.onend = () => {
        const x = getTab(tabId);
        if (!x) return;
        x.inputValue = (x._committed || '').trim();
        if (x._shouldListen && !x.isProcessing) {
            try { rec.start(); return; } catch (e) { /* restart failed */ }
        }
        x.isListening = false;
        x._committed = '';
        refresh();
    };

    rec.onerror = (ev: any) => {
        const x = getTab(tabId);
        if (!x) return;
        x.isListening = false;
        x._shouldListen = false;
        x.inputValue = (x._committed || '').trim();
        x._committed = '';
        if (ev.error === 'not-allowed' || ev.error === 'permission-denied') {
            addMsg(tabId, {
                role: 'lucy',
                html: `<div class="mn">⊕ Micrófono sin permiso</div>Ve a <b>Inicio → Configuración → Privacidad y seguridad → Micrófono</b> y activa el acceso para aplicaciones de escritorio.`,
                style: 'border-left-color:#f59e0b;'
            });
        } else if (ev.error === 'network') {
            addMsg(tabId, {
                role: 'lucy',
                html: `<div class="mn">⊕ Error de red</div>El reconocimiento de voz requiere conexión a internet.`,
                style: 'border-left-color:#f59e0b;'
            });
        }
        // 'no-speech' is silent — the user simply didn't speak
        refresh();
    };

    return rec;
}

// ── toggleMic ─────────────────────────────────────────────────────────────────
// Starts or stops the microphone for a given tab.
export async function toggleMic(tabId: string, opts: VoiceOpts): Promise<void> {
    const { getTab, addMsg, refresh, toast } = opts;
    const t = getTab(tabId);
    if (!t || !t.recognition) {
        toast('El reconocimiento de voz no está disponible en este equipo', 'error');
        return;
    }
    // In WebView2 (Tauri), getUserMedia must be called first to trigger the OS permission prompt
    if (!t.isListening && (navigator.mediaDevices as any)?.getUserMedia) {
        try {
            const stream = await navigator.mediaDevices.getUserMedia({ audio: true });
            stream.getTracks().forEach(track => track.stop()); // release immediately
        } catch (_permErr) {
            addMsg(tabId, {
                role: 'lucy',
                html: `<div class="mn">⊕ Micrófono sin permiso</div>Windows bloqueó el acceso al micrófono para esta app. Ve a <b>Inicio → Configuración → Privacidad y seguridad → Micrófono</b>, activa <b>"Permitir que las aplicaciones de escritorio accedan al micrófono"</b> y reinicia Lucy.`,
                style: 'border-left-color:#f59e0b;'
            });
            refresh();
            return;
        }
    }
    if (t.isListening) {
        t._shouldListen = false;
        t.recognition.stop();
    } else {
        if (window.speechSynthesis) window.speechSynthesis.cancel();
        t._shouldListen = true;
        t._committed = t.inputValue.trim(); // preserve existing text
        try {
            t.recognition.start();
        } catch (_e) {
            t._shouldListen = false;
            t.isListening = false;
            toast('Error al iniciar el micrófono. Intenta de nuevo.', 'error');
        }
    }
    refresh();
}

// ── TTS voice selection (v1.7.235) ───────────────────────────────────────────
// The old picker took `matchVoices[0]` — on Windows the Spanish list is usually
// headed by "Microsoft Raúl" (male, legacy SAPI), which is why Lucy spoke with
// the generic male OS voice even when Sabina/Helena (female) or the far better
// Edge "(Natural)" neural voices were installed. Now:
//   1. The user can PIN a voice (localStorage `lucy_tts_voice`, exact
//      voice.name; picker in Configuración → Modelos y comportamiento).
//   2. With no pin (or a pinned voice whose language no longer matches), a
//      ranking picks the best default: neural/Natural quality first, then
//      female given names (Lucy is female), exact locale over same-prefix.

const LS_TTS_VOICE = 'lucy_tts_voice';

// Female given names across Windows SAPI + Edge Natural voices (es/en). Used
// only as a RANKING hint — any voice can still be pinned explicitly.
const FEMALE_HINTS = [
    'dalia', 'sabina', 'helena', 'paloma', 'laura', 'elvira', 'camila', 'lucia', 'lucía',
    'isidora', 'andrea', 'yolanda', 'ximena', 'renata', 'catalina', 'paulina', 'francisca',
    'valentina', 'marcela', 'salome', 'salomé', 'sonia', 'carmen', 'mónica', 'monica',
    'jenny', 'aria', 'michelle', 'zira', 'eva', 'ana', 'emma', 'ava', 'sonia',
];
const MALE_HINTS = ['raul', 'raúl', 'pablo', 'jorge', 'gerardo', 'david', 'mark', 'guy', 'christopher', 'eric', 'roger'];

function rankVoice(v: SpeechSynthesisVoice, wantTts: string): number {
    const prefix = wantTts.split('-')[0];
    if (!v.lang.startsWith(prefix)) return -1;         // wrong language → out
    let s = v.lang === wantTts ? 40 : 20;              // exact locale > same prefix
    const n = v.name.toLowerCase();
    if (n.includes('natural') || n.includes('neural') || n.includes('online')) s += 30; // Edge neural ≫ SAPI
    if (FEMALE_HINTS.some(f => n.includes(f))) s += 25;
    if (MALE_HINTS.some(m => n.includes(m))) s -= 15;
    return s;
}

// Ensure getVoices() is populated (async in Tauri WebView). Bounded wait, no
// orphan listeners (the historical leak fix is preserved).
export async function ensureTtsVoices(): Promise<SpeechSynthesisVoice[]> {
    if (!window.speechSynthesis) return [];
    let voces = window.speechSynthesis.getVoices();
    if (!voces.length) {
        await new Promise<void>(resolve => {
            let _settled = false;
            const onVoicesChanged = () => {
                if (_settled) return;
                _settled = true;
                window.speechSynthesis.removeEventListener('voiceschanged', onVoicesChanged);
                resolve();
            };
            window.speechSynthesis.addEventListener('voiceschanged', onVoicesChanged);
            setTimeout(() => {
                if (_settled) return;
                _settled = true;
                window.speechSynthesis.removeEventListener('voiceschanged', onVoicesChanged);
                resolve();
            }, 2000);
        });
        voces = window.speechSynthesis.getVoices();
    }
    return voces;
}

// Resolve the voice speak() will use for `wantTts` — pinned if valid, else the
// ranked default. Exported so the Config picker can show the effective choice.
export function resolveTtsVoice(voces: SpeechSynthesisVoice[], wantTts: string): SpeechSynthesisVoice | undefined {
    let pinned: string | null = null;
    try { pinned = localStorage.getItem(LS_TTS_VOICE); } catch {}
    if (pinned) {
        const v = voces.find(x => x.name === pinned);
        // A pinned voice only applies while it matches the active language —
        // switching Lucy to English must not read English with a Spanish voice.
        if (v && v.lang.startsWith(wantTts.split('-')[0])) return v;
    }
    return voces
        .map(v => ({ v, s: rankVoice(v, wantTts) }))
        .filter(x => x.s >= 0)
        .sort((a, b) => b.s - a.s)[0]?.v;
}

// ── speak ─────────────────────────────────────────────────────────────────────
// TTS: strips HTML/markdown from text before speaking.
export async function speak(text: string, opts: Pick<VoiceOpts, 'getActiveLang'>): Promise<void> {
    if (!window.speechSynthesis) return;
    const limpio = text
        .replace(/<[^>]*>?/gm, '')
        .replace(/```[\s\S]*?```/g, ' Código. ')
        .replace(/`[^`]+`/g, '')
        .replace(/[*_#~]/g, '')
        .replace(/\n{2,}/g, '. ')
        .replace(/\n/g, ' ')
        .trim();
    if (!limpio) return;

    window.speechSynthesis.cancel();

    const voces = await ensureTtsVoices();

    const activeLang = opts.getActiveLang();
    const u = new SpeechSynthesisUtterance(limpio);
    u.lang = activeLang.tts;
    u.rate = 1.05;
    u.pitch = 1.0;

    const chosen = resolveTtsVoice(voces, activeLang.tts);
    if (chosen) u.voice = chosen;

    window.speechSynthesis.speak(u);
}
