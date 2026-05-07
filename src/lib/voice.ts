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

    // Wait for voices if not yet loaded (needed in Tauri WebView).
    // BUG FIX: the old setTimeout(2s) race left a permanent 'voiceschanged' listener.
    // Each speak() call on a new tab added another orphan listener.
    let voces = window.speechSynthesis.getVoices();
    if (!voces.length) {
        await new Promise<void>(resolve => {
            let _settled = false;
            const onVoicesChanged = () => {
                if (_settled) return;
                _settled = true;
                voces = window.speechSynthesis.getVoices();
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

    const activeLang = opts.getActiveLang();
    const u = new SpeechSynthesisUtterance(limpio);
    u.lang = activeLang.tts;
    u.rate = 1.05;
    u.pitch = 1.0;

    const langPrefix = activeLang.tts.split('-')[0];
    const matchVoices = voces.filter(v => v.lang.startsWith(langPrefix));
    if (matchVoices.length) {
        u.voice = matchVoices.find(v => v.lang === activeLang.tts) || matchVoices[0];
    }

    window.speechSynthesis.speak(u);
}
