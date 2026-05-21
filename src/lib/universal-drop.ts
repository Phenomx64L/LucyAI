// ── universal-drop.ts — U7 Drag-to-Lucy detector ─────────────────────────
//
// Classifies a `DataTransfer` payload from a drop event into one of the
// kinds Lucy knows how to handle. This is the routing layer — the caller
// decides what to do with each kind (fill input, fetch URL, OCR image, etc.)
//
// Kinds detected (in priority order):
//   1. files       — at least one File entry (existing behavior preserved)
//   2. url         — a single URL pasted from browser address bar / link
//   3. image_uri   — an inline image (data: URL, or image/* MIME) from browser
//   4. text        — plain text selection longer than 8 chars
//   5. unknown     — caller falls back to default behavior
//
// The Tauri webview occasionally hides some types when dragging from
// outside the app — we read multiple .getData() keys to be resilient.

export type DropKind = 'files' | 'url' | 'image_uri' | 'text' | 'unknown';

export interface ClassifiedDrop {
    kind: DropKind;
    /** Original files (only populated when kind === 'files'). */
    files: File[];
    /** URL string (only when kind === 'url'). */
    url?: string;
    /** Image src — either http(s) or data: (only when kind === 'image_uri'). */
    imageSrc?: string;
    /** Plain text (only when kind === 'text'). */
    text?: string;
    /** Raw HTML if available, for caller debugging. */
    html?: string;
}

/** Quick URL test — http(s)/file/ftp + at least a host. */
function looksLikeUrl(s: string): boolean {
    const trimmed = s.trim();
    if (trimmed.length > 2000 || trimmed.includes('\n')) return false;
    return /^(https?|ftp|file):\/\/[^\s]+/i.test(trimmed);
}

function extractImageFromHtml(html: string): string | null {
    if (!html) return null;
    // First <img src="..."> wins
    const m = html.match(/<img[^>]+src=["']([^"']+)["']/i);
    return m ? m[1] : null;
}

/**
 * Classify a drop. Pure function — no side effects.
 *
 * Caller pattern:
 *   const dropped = classifyDrop(event.dataTransfer);
 *   switch (dropped.kind) { ... }
 */
export function classifyDrop(dt: DataTransfer | null): ClassifiedDrop {
    if (!dt) return { kind: 'unknown', files: [] };

    // 1) Files — always wins. Existing file-drop logic handles them.
    const files: File[] = [];
    if (dt.files && dt.files.length > 0) {
        for (let i = 0; i < dt.files.length; i++) {
            const f = dt.files.item(i);
            if (f) files.push(f);
        }
    }
    if (files.length > 0) {
        return { kind: 'files', files };
    }

    // 2) Text payloads
    const urlData = (dt.getData('text/uri-list') || '').trim();
    const plainData = (dt.getData('text/plain') || '').trim();
    const htmlData  = (dt.getData('text/html') || '').trim();

    // URL detection: prefer text/uri-list (browser drag-from-address-bar), fall back to plain
    const candidateUrl = urlData || (looksLikeUrl(plainData) ? plainData : '');
    if (candidateUrl) {
        return {
            kind: 'url',
            files,
            url: candidateUrl,
            html: htmlData || undefined,
        };
    }

    // 3) Image from HTML payload (browser drag of an image)
    const imgSrc = extractImageFromHtml(htmlData);
    if (imgSrc) {
        return {
            kind: 'image_uri',
            files,
            imageSrc: imgSrc,
            html: htmlData,
        };
    }

    // 4) Plain text (substantial enough to be intentional)
    if (plainData.length >= 8) {
        return {
            kind: 'text',
            files,
            text: plainData,
            html: htmlData || undefined,
        };
    }

    return { kind: 'unknown', files, html: htmlData || undefined };
}

/**
 * Suggest a default prompt template for each non-file drop kind. The caller
 * can use this to seed the input box. Returns null when the kind doesn't
 * map to a single sensible default (files, unknown).
 */
export function defaultPromptForKind(kind: DropKind, payload: ClassifiedDrop, isEN = false): string | null {
    switch (kind) {
        case 'url':
            return isEN
                ? `Fetch and summarize this URL: ${payload.url}`
                : `Trae y resume esta URL: ${payload.url}`;
        case 'image_uri':
            return isEN
                ? `Describe what's in this image: ${payload.imageSrc}`
                : `Describe qué hay en esta imagen: ${payload.imageSrc}`;
        case 'text': {
            const preview = (payload.text || '').slice(0, 800);
            return isEN
                ? `Explain this:\n\n${preview}`
                : `Explícame esto:\n\n${preview}`;
        }
        default:
            return null;
    }
}
