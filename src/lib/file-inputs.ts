// ── file-inputs.ts — File attachment helpers ─────────────────────────────────
// Extracted from +page.svelte. Handles Tauri file picker, drag-drop, and paste.

import { invoke } from '@tauri-apps/api/core';

export interface FileInputOpts {
    isEN: boolean;
    getActiveTabId: () => string | null;
    getTab:  (id: string) => any;
    refresh: () => void;
    toast:   (msg: string, type: string) => void;
    setDragOverlay: (visible: boolean) => void;
}

// ── The attachment contract (v1.8.1) ─────────────────────────────────────────
//
// Every attached file is `{ name, content, type, mimeType?, previewUrl? }`,
// and `type` is what the rest of the app dispatches on:
//
//   type 'image' → `content` is base64; goes to the model as VISION input.
//   type 'text'  → `content` is TEXT; goes into the prompt under `--- ARCHIVOS ---`.
//
// A PDF is therefore `type: 'text'` (its extracted text) carrying
// `mimeType: 'application/pdf'` so the UI can still show a PDF chip. That
// split is the whole fix: the old code classified everything that was not
// `text/plain` as an image, so a PDF became a fake image whose base64 the
// text path (`filter(f => f.type === 'text')`) then skipped entirely. The
// model received NOTHING while the composer showed a chip — which is why
// attaching a PDF appeared to work but Lucy could not read it, and users
// fell back to pasting an absolute path.
//
// Keep `type` to exactly these two values. Adding a third ('pdf', 'doc') would
// silently drop those files from the prompt-building filter again.
export interface AttachedFile {
    name: string;
    content: string;
    type: 'image' | 'text';
    mimeType?: string;
    previewUrl?: string;
}

/** True when the backend/browser mime says this is a picture. */
function isImageMime(mime: string): boolean {
    return typeof mime === 'string' && mime.startsWith('image/');
}

/** True for a file we must route through the backend PDF text extractor. */
function isPdf(name: string, mime?: string): boolean {
    return mime === 'application/pdf' || /\.pdf$/i.test(name || '');
}

// ── attach ────────────────────────────────────────────────────────────────────
// Opens the native Tauri multi-file picker and appends selected files to the tab.
export async function attach(tabId: string, opts: FileInputOpts): Promise<void> {
    const { isEN, getTab, refresh, toast } = opts;
    try {
        const archivos = await invoke<any[]>('pick_multiple_files');
        if (!archivos || !archivos.length) return;
        const t = getTab(tabId);
        if (!Array.isArray(t.attachedFiles)) t.attachedFiles = [];
        let agregados = 0;
        for (const r of archivos) {
            const [name, content, mime] = r as [string, string, string];

            // The backend reports unreadable files in-band rather than dropping
            // them, so the user learns WHICH file failed and why.
            if (mime === '__error__') {
                toast(content || `${isEN ? 'Could not read' : 'No se pudo leer'} ${name}`, 'error');
                continue;
            }
            if (t.attachedFiles.some((f: any) => f.name === name)) continue;

            if (isImageMime(mime)) {
                t.attachedFiles.push({
                    name, content, type: 'image', mimeType: mime,
                    previewUrl: `data:${mime};base64,${content}`,
                });
            } else {
                // text/plain AND application/pdf land here: for both, `content`
                // is already plain text (the backend extracted the PDF).
                t.attachedFiles.push({ name, content, type: 'text', mimeType: mime });
            }
            agregados++;
        }
        if (agregados > 0) refresh();
    } catch (e) {
        toast(`${isEN ? 'Error attaching files' : 'Error adjuntando archivos'}: ${e}`, 'error');
    }
}

// ── readDroppedFile ───────────────────────────────────────────────────────────
// Shared reader for the two drag-and-drop entry points.
//
// `tauri.conf.json` sets `dragDropEnabled: false`, so a drop arrives as an
// HTML5 `File` with no filesystem path — the picker's backend path cannot be
// reused. PDFs are therefore base64-encoded here and extracted by
// `extract_pdf_text_from_bytes`. Before this, dropped PDFs went through
// `readAsText`, which produced mojibake from the binary and fed that to the
// model.
async function readDroppedFile(file: File): Promise<AttachedFile | null> {
    const MAX_TEXT = 200_000; // cap text so a huge log cannot OOM the webview

    if (file.type.startsWith('image/')) {
        const dataUrl = await new Promise<string>((res, rej) => {
            const r = new FileReader();
            r.onload = () => res(String(r.result || ''));
            r.onerror = () => rej(r.error);
            r.readAsDataURL(file);
        });
        return {
            name: file.name, content: dataUrl.split(',')[1] || '',
            type: 'image', mimeType: file.type, previewUrl: dataUrl,
        };
    }

    if (isPdf(file.name, file.type)) {
        const dataUrl = await new Promise<string>((res, rej) => {
            const r = new FileReader();
            r.onload = () => res(String(r.result || ''));
            r.onerror = () => rej(r.error);
            r.readAsDataURL(file);   // base64 without a manual byte loop
        });
        const b64 = dataUrl.split(',')[1] || '';
        const text = await invoke<string>('extract_pdf_text_from_bytes', { name: file.name, dataB64: b64 });
        return { name: file.name, content: text, type: 'text', mimeType: 'application/pdf' };
    }

    const text = await new Promise<string>((res, rej) => {
        const r = new FileReader();
        r.onload = () => res(String(r.result || ''));
        r.onerror = () => rej(r.error);
        r.readAsText(file);
    });
    return { name: file.name, content: text.slice(0, MAX_TEXT), type: 'text', mimeType: file.type || 'text/plain' };
}

// ── removeFile ────────────────────────────────────────────────────────────────
export function removeFile(tabId: string, name: string, opts: Pick<FileInputOpts, 'getTab' | 'refresh'>): void {
    const { getTab, refresh } = opts;
    const t = getTab(tabId);
    t.attachedFiles = t.attachedFiles.filter((f: any) => f.name !== name);
    refresh();
}

// ── handleFileDrop ────────────────────────────────────────────────────────────
// Reads files from a drag-over drop event for a specific tab.
export interface PendingDrop { name: string; promise: Promise<AttachedFile | null>; }

/**
 * Start reading EVERY dropped file immediately, synchronously.
 *
 * This must be called from inside the drop handler's synchronous execution.
 * Chromium/WebView2 tears down the drag data store as soon as the event
 * handler returns, and the `File` objects in `dataTransfer.files` stop being
 * readable — a later `FileReader` call fails with
 * `NotFoundError: A requested file or directory could not be found…`.
 *
 * That bit Lucy in two places, and both failed SILENTLY because the readers
 * had no `onerror`:
 *   • `+page.svelte`'s drop wrapper awaited `maybeInstallSkillFromDrop(e)` and
 *     only then handed the event over — always past the deadline.
 *   • the old `await` loop started file N+1's read only after file N finished,
 *     so multi-file drops lost everything except the first.
 *
 * `readDroppedFile` kicks its `FileReader` off inside a Promise executor, which
 * runs synchronously — so mapping over the files here starts every read before
 * we yield to the event loop. The returned promises can then be awaited safely
 * at any point afterwards.
 */
export function startReadingDrop(dt: DataTransfer | null | undefined): PendingDrop[] {
    return Array.from(dt?.files || []).map(f => ({ name: f.name, promise: readDroppedFile(f) }));
}

/** Await reads started by `startReadingDrop` and attach whatever succeeded. */
export async function collectDroppedFiles(
    tabId: string,
    pending: PendingDrop[],
    opts: Pick<FileInputOpts, 'getTab' | 'refresh'> & Partial<Pick<FileInputOpts, 'toast'>>,
): Promise<void> {
    const { getTab, refresh, toast } = opts;
    const t = getTab(tabId);
    if (!t || !pending.length) return;
    if (!Array.isArray(t.attachedFiles)) t.attachedFiles = [];
    for (const p of pending) {
        try {
            const parsed = await p.promise;
            if (!parsed) continue;
            if (t.attachedFiles.some((a: any) => a.name === parsed.name)) continue;
            t.attachedFiles.push(parsed);
        } catch (err) {
            // Surface it. A silent console warning is what let the original
            // drag-and-drop breakage hide for so long.
            console.warn('drop file failed', p.name, err);
            toast?.(`${p.name}: ${err}`, 'error');
        }
    }
    refresh();
}

export async function handleFileDrop(
    e: DragEvent,
    tabId: string,
    opts: Pick<FileInputOpts, 'getTab' | 'refresh'> & Partial<Pick<FileInputOpts, 'toast'>>,
): Promise<void> {
    // Reads start here, synchronously, while dataTransfer is still alive.
    const pending = startReadingDrop(e.dataTransfer);
    await collectDroppedFiles(tabId, pending, opts);
}

// ── onDrop ────────────────────────────────────────────────────────────────────
// Global drop handler — hides the drag overlay and delegates to file reading.
export function onDrop(e: DragEvent, opts: FileInputOpts): void {
    const { getActiveTabId, setDragOverlay } = opts;
    setDragOverlay(false);
    const activeTabId = getActiveTabId();
    if (!activeTabId) return;
    // Same shared reader as handleFileDrop — the two handlers drifted apart
    // once and only one of them ever learned about PDFs.
    const pending = startReadingDrop(e.dataTransfer);
    void collectDroppedFiles(activeTabId, pending, opts);
}

// ── onPaste ───────────────────────────────────────────────────────────────────
// Clipboard paste handler — captures screenshots and images from native apps.
export async function onPaste(e: ClipboardEvent, opts: FileInputOpts): Promise<void> {
    const { getActiveTabId, getTab, refresh } = opts;
    const activeTabId = getActiveTabId();
    if (!activeTabId) return;
    const t = getTab(activeTabId);
    if (!t) return;
    let handled = false;

    try {
        // Attempt 1: clipboardData.items (screenshots, browser copies)
        const items = (e.clipboardData || (window as any).clipboardData)?.items;
        if (items) {
            for (let i = 0; i < items.length; i++) {
                const item = items[i];
                if (!item || typeof item.type !== 'string') continue;
                const mimeType = item.type; // capture SYNCHRONOUSLY before any await/callback
                if (mimeType.indexOf('image') !== -1) {
                    const blob = item.getAsFile();
                    if (!blob) continue;
                    handled = true;
                    const r = new FileReader();
                    r.onerror = () => {};
                    r.onload = (ev: any) => {
                        try {
                            if (!ev?.target?.result) return;
                            const ext = mimeType.split('/')[1] || 'png';
                            if (!Array.isArray(t.attachedFiles)) t.attachedFiles = [];
                            t.attachedFiles.push({ name: `Cap_${Date.now()}.${ext}`, content: ev.target.result.split(',')[1], type: 'image', mimeType, previewUrl: ev.target.result });
                            refresh();
                        } catch (_) {}
                    };
                    r.readAsDataURL(blob);
                }
            }
        }

        // Attempt 2: navigator.clipboard.read() — images copied from Explorer/native apps
        if (!handled && navigator.clipboard?.read) {
            try {
                const clipItems = await navigator.clipboard.read();
                for (const ci of clipItems) {
                    for (const mimeType of ci.types) {
                        if (mimeType.startsWith('image/')) {
                            const blob = await ci.getType(mimeType);
                            const r = new FileReader();
                            r.onerror = () => {};
                            r.onload = (ev: any) => {
                                try {
                                    if (!ev?.target?.result) return;
                                    const ext = mimeType.split('/')[1] || 'png';
                                    if (!Array.isArray(t.attachedFiles)) t.attachedFiles = [];
                                    t.attachedFiles.push({ name: `Img_${Date.now()}.${ext}`, content: ev.target.result.split(',')[1], type: 'image', mimeType, previewUrl: ev.target.result });
                                    refresh();
                                } catch (_) {}
                            };
                            r.readAsDataURL(blob);
                            handled = true;
                        }
                    }
                }
            } catch (_) { /* permission denied or not available in this context */ }
        }
    } catch (err) {
        console.warn('[Lucy] onPaste error (ignorado):', err);
    }

    if (handled) e.preventDefault();
}
