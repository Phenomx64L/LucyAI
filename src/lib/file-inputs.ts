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

// ── attach ────────────────────────────────────────────────────────────────────
// Opens the native Tauri multi-file picker and appends selected files to the tab.
export async function attach(tabId: string, opts: FileInputOpts): Promise<void> {
    const { isEN, getTab, refresh, toast } = opts;
    try {
        const archivos = await invoke<any[]>('pick_multiple_files');
        if (!archivos || !archivos.length) return;
        const t = getTab(tabId);
        let agregados = 0;
        for (const r of archivos) {
            if (r[2] === 'text/plain') {
                if (!t.attachedFiles.some((f: any) => f.name === r[0])) {
                    t.attachedFiles.push({ name: r[0], content: r[1], type: 'text' });
                    agregados++;
                }
            } else {
                const u = `data:${r[2]};base64,${r[1]}`;
                if (!t.attachedFiles.some((f: any) => f.name === r[0])) {
                    t.attachedFiles.push({ name: r[0], content: r[1], type: 'image', mimeType: r[2], previewUrl: u });
                    agregados++;
                }
            }
        }
        if (agregados > 0) refresh();
    } catch (e) {
        toast(`${isEN ? 'Error attaching files' : 'Error adjuntando archivos'}: ${e}`, 'error');
    }
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
export async function handleFileDrop(e: DragEvent, tabId: string, opts: Pick<FileInputOpts, 'getTab' | 'refresh'>): Promise<void> {
    const { getTab, refresh } = opts;
    const t = getTab(tabId);
    if (!t) return;
    const files = Array.from(e.dataTransfer?.files || []);
    if (!files.length) return;
    if (!Array.isArray(t.attachedFiles)) t.attachedFiles = [];
    for (const f of files) {
        try {
            const isImg = f.type.startsWith('image/');
            const reader = new FileReader();
            const data = await new Promise<string | ArrayBuffer | null>((res, rej) => {
                reader.onload = () => res(reader.result);
                reader.onerror = rej;
                isImg ? reader.readAsDataURL(f) : reader.readAsText(f);
            });
            if (isImg) {
                t.attachedFiles.push({ name: f.name, content: String(data).split(',')[1] || '', type: 'image', mimeType: f.type, previewUrl: data });
            } else {
                t.attachedFiles.push({ name: f.name, content: String(data).slice(0, 200_000), type: 'text' });
            }
        } catch (err) { console.warn('drop file failed', f.name, err); }
    }
    refresh();
}

// ── onDrop ────────────────────────────────────────────────────────────────────
// Global drop handler — hides the drag overlay and delegates to file reading.
export function onDrop(e: DragEvent, opts: FileInputOpts): void {
    const { getActiveTabId, getTab, refresh, setDragOverlay } = opts;
    setDragOverlay(false);
    const activeTabId = getActiveTabId();
    if (!activeTabId || !e.dataTransfer?.files?.length) return;
    const t = getTab(activeTabId);
    Array.from(e.dataTransfer.files).forEach(file => {
        const r = new FileReader();
        if (file.type.startsWith('image/')) {
            r.onload = (ev: any) => {
                if (!t.attachedFiles.some((f: any) => f.name === file.name)) {
                    t.attachedFiles.push({ name: file.name, content: ev.target.result.split(',')[1], type: 'image', mimeType: file.type, previewUrl: ev.target.result });
                    refresh();
                }
            };
            r.readAsDataURL(file);
        } else {
            // Cap text files at 200KB to prevent OOM on large log files
            const MAX_TEXT = 200_000;
            r.onload = (ev: any) => {
                if (!t.attachedFiles.some((f: any) => f.name === file.name)) {
                    const text = String(ev.target.result || '').slice(0, MAX_TEXT);
                    t.attachedFiles.push({ name: file.name, content: text, type: 'text' });
                    refresh();
                }
            };
            r.readAsText(file);
        }
    });
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
