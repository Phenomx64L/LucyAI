// ── file-inputs.test.ts ───────────────────────────────────────────────────
//
// v1.8.1 regression net for the attachment pipeline.
//
// The bug this guards: `attach()` classified everything that was not
// `text/plain` as `type: 'image'`. A PDF therefore became a fake image, and
// the prompt builder in `+page.svelte` — which collects file text with
// `filter(f => f.type === 'text')` — skipped it. The composer showed an
// attachment chip while the model received nothing, so Lucy "could not read"
// attached PDFs and users had to paste an absolute path instead.
//
// The invariant these tests pin: ANY non-image attachment must end up as
// `type: 'text'` with its text in `content`, because that is the only shape
// the prompt builder feeds to the model.

import { describe, it, expect, vi, beforeEach, afterEach } from 'vitest';

const mockInvoke = vi.fn();
vi.mock('@tauri-apps/api/core', () => ({
    invoke: (cmd: string, args?: unknown) => mockInvoke(cmd, args),
}));

const { attach, startReadingDrop, collectDroppedFiles, onDrop, handleFileDrop } =
    await import('./file-inputs');

/** Minimal tab + opts doubles — attach() only touches these. */
function harness() {
    const tab = { attachedFiles: [] as any[] };
    const toasts: Array<{ msg: string; type: string }> = [];
    return {
        tab,
        toasts,
        opts: {
            isEN: false,
            getActiveTabId: () => 't1',
            getTab: () => tab,
            refresh: () => {},
            toast: (msg: string, type: string) => { toasts.push({ msg, type }); },
            setDragOverlay: () => {},
        },
    };
}

describe('attach — mime → type classification', () => {
    beforeEach(() => { mockInvoke.mockReset(); });

    it('keeps an extracted PDF as type "text" so the prompt builder includes it', async () => {
        const h = harness();
        mockInvoke.mockResolvedValue([
            ['guide.pdf', 'Texto extraido del PDF', 'application/pdf'],
        ]);

        await attach('t1', h.opts as any);

        expect(h.tab.attachedFiles).toHaveLength(1);
        const f = h.tab.attachedFiles[0];
        // THE invariant — 'image' here would silently drop it from the prompt.
        expect(f.type).toBe('text');
        expect(f.mimeType).toBe('application/pdf');
        expect(f.content).toBe('Texto extraido del PDF');
        // A PDF is not a picture: no thumbnail data must be fabricated.
        expect(f.previewUrl).toBeUndefined();
    });

    it('classifies real images as type "image" with a data: preview', async () => {
        const h = harness();
        mockInvoke.mockResolvedValue([['shot.png', 'QUJD', 'image/png']]);

        await attach('t1', h.opts as any);

        const f = h.tab.attachedFiles[0];
        expect(f.type).toBe('image');
        expect(f.mimeType).toBe('image/png');
        expect(f.previewUrl).toBe('data:image/png;base64,QUJD');
    });

    it('classifies plain text as type "text"', async () => {
        const h = harness();
        mockInvoke.mockResolvedValue([['app.log', 'linea 1\nlinea 2', 'text/plain']]);

        await attach('t1', h.opts as any);

        expect(h.tab.attachedFiles[0].type).toBe('text');
        expect(h.tab.attachedFiles[0].content).toBe('linea 1\nlinea 2');
    });

    it('surfaces unreadable files as a toast instead of attaching them', async () => {
        // The backend reports failures in-band with the __error__ mime. Before
        // this, an unreadable file was logged server-side and dropped, leaving
        // the user with no signal at all.
        const h = harness();
        mockInvoke.mockResolvedValue([
            ['broken.bin', "'broken.bin' no es texto legible", '__error__'],
            ['ok.txt', 'contenido', 'text/plain'],
        ]);

        await attach('t1', h.opts as any);

        expect(h.tab.attachedFiles).toHaveLength(1);
        expect(h.tab.attachedFiles[0].name).toBe('ok.txt');
        expect(h.toasts).toHaveLength(1);
        expect(h.toasts[0].type).toBe('error');
        expect(h.toasts[0].msg).toContain('broken.bin');
    });

    it('does not attach the same file twice', async () => {
        const h = harness();
        mockInvoke.mockResolvedValue([['a.txt', 'x', 'text/plain']]);
        await attach('t1', h.opts as any);
        await attach('t1', h.opts as any);
        expect(h.tab.attachedFiles).toHaveLength(1);
    });

    it('mixes documents and images in one selection', async () => {
        const h = harness();
        mockInvoke.mockResolvedValue([
            ['guide.pdf', 'texto', 'application/pdf'],
            ['shot.png', 'QUJD', 'image/png'],
            ['notes.md', '# hola', 'text/plain'],
        ]);

        await attach('t1', h.opts as any);

        const byType = h.tab.attachedFiles.map((f: any) => f.type);
        expect(byType).toEqual(['text', 'image', 'text']);
        // Two of the three must reach the model through the text path.
        expect(h.tab.attachedFiles.filter((f: any) => f.type === 'text')).toHaveLength(2);
    });
});

// ── Drag and drop ────────────────────────────────────────────────────────────
//
// The second ingest path, and the one that was broken from the start while
// failing silently — the readers had no `onerror`, so a dead drop looked like
// "sometimes you have to paste the absolute path instead".
//
// The fix rests on a TIMING invariant that no type-checker or linter can see
// (ARCHITECTURE gotcha 11): Chromium/WebView2 tears down the drag data store
// as soon as the drop handler returns, so every `FileReader` must be kicked
// off inside the handler's synchronous run. Put one `await` above the read and
// the whole feature dies again — with every gate still green.
//
// These tests model that teardown directly: a read STARTED while the store is
// alive completes, one started afterwards rejects with `NotFoundError`, just
// as the real thing does. That makes the invariant fail loudly instead of
// silently.

/** A stand-in for a dropped `File`. `readDroppedFile` only reads these. */
interface FakeFile { name: string; type: string; body: string; broken?: boolean; }

const file = (name: string, type: string, body = `content of ${name}`): FakeFile =>
    ({ name, type, body });

/** Is Chromium's drag data store still alive? Flipped by the tests. */
let dragStoreAlive = true;
/** Names of files whose read has been KICKED OFF, in order. */
let readsStarted: string[] = [];

class FakeFileReader {
    result: string | null = null;
    error: unknown = null;
    onload: (() => void) | null = null;
    onerror: (() => void) | null = null;

    private begin(f: FakeFile, produce: (f: FakeFile) => string) {
        readsStarted.push(f.name);
        // Readability is captured HERE, at kick-off — modelling the real
        // constraint that a read bound to a live store survives the teardown
        // while a later one cannot.
        const readable = dragStoreAlive && !f.broken;
        queueMicrotask(() => {
            if (!readable) {
                this.error = new Error(
                    'NotFoundError: A requested file or directory could not be found at the time an operation was processed.',
                );
                this.onerror?.();
                return;
            }
            this.result = produce(f);
            this.onload?.();
        });
    }
    readAsText(f: FakeFile) { this.begin(f, x => x.body); }
    readAsDataURL(f: FakeFile) {
        this.begin(f, x => `data:${x.type};base64,${btoa(x.body)}`);
    }
}

describe('drag and drop', () => {
    const realFileReader = globalThis.FileReader;

    beforeEach(() => {
        mockInvoke.mockReset();
        dragStoreAlive = true;
        readsStarted = [];
        (globalThis as any).FileReader = FakeFileReader;
    });
    afterEach(() => { (globalThis as any).FileReader = realFileReader; });

    const drop = (files: FakeFile[]) => ({ dataTransfer: { files } }) as any;

    it('starts every read before yielding to the event loop', () => {
        // THE invariant. Deliberately synchronous — there is no `await` above
        // this assertion, because in production there must not be one either.
        startReadingDrop(drop([file('a.txt', 'text/plain'), file('b.txt', 'text/plain'), file('c.txt', 'text/plain')]).dataTransfer);

        expect(readsStarted).toEqual(['a.txt', 'b.txt', 'c.txt']);
    });

    it('survives the data store being torn down the moment the handler returns', async () => {
        const pending = startReadingDrop(
            drop([file('a.txt', 'text/plain'), file('b.txt', 'text/plain')]).dataTransfer,
        );
        dragStoreAlive = false; // Chromium, as the drop handler returns.

        const got = await Promise.all(pending.map(p => p.promise));

        expect(got.map(f => f?.content)).toEqual(['content of a.txt', 'content of b.txt']);
    });

    it('onDrop attaches everything even though the store dies immediately', async () => {
        // The global handler used to hand the event over from inside
        // `maybeInstallSkillFromDrop(e).then(…)` — always past the deadline.
        const h = harness();
        onDrop(drop([file('a.txt', 'text/plain'), file('b.log', 'text/plain')]), h.opts as any);
        dragStoreAlive = false;

        await vi.waitFor(() => expect(h.tab.attachedFiles).toHaveLength(2));
        expect(h.tab.attachedFiles.map((f: any) => f.name)).toEqual(['a.txt', 'b.log']);
    });

    it('handleFileDrop keeps every file in a multi-file drop', async () => {
        // The old reader awaited file N before starting N+1, so by file two the
        // store was gone and a multi-file drop kept only the first.
        const h = harness();
        const e = drop([file('a.txt', 'text/plain'), file('b.txt', 'text/plain'), file('c.txt', 'text/plain')]);

        await handleFileDrop(e, 't1', h.opts as any);

        expect(h.tab.attachedFiles).toHaveLength(3);
    });

    it('routes a dropped PDF through the backend extractor as type "text"', async () => {
        const h = harness();
        mockInvoke.mockResolvedValue('Texto extraido del PDF soltado');

        await handleFileDrop(drop([file('guide.pdf', 'application/pdf', '%PDF-1.4 binary')]), 't1', h.opts as any);

        expect(mockInvoke).toHaveBeenCalledWith(
            'extract_pdf_text_from_bytes',
            { name: 'guide.pdf', dataB64: expect.any(String) },
        );
        // Same contract as the picker: extracted TEXT, pdf mime for the icon.
        expect(h.tab.attachedFiles[0]).toMatchObject({
            name: 'guide.pdf', type: 'text', mimeType: 'application/pdf',
            content: 'Texto extraido del PDF soltado',
        });
    });

    it('recognises a PDF by extension when the drop carries no mime type', async () => {
        // Windows drops frequently arrive with an empty `type`; without the
        // extension check the file would take the readAsText path and feed the
        // model mojibake from the raw binary.
        const h = harness();
        mockInvoke.mockResolvedValue('texto');

        await handleFileDrop(drop([file('manual.PDF', '', '%PDF-1.4')]), 't1', h.opts as any);

        expect(mockInvoke).toHaveBeenCalledWith('extract_pdf_text_from_bytes', expect.anything());
        expect(h.tab.attachedFiles[0].type).toBe('text');
    });

    it('turns a dropped image into type "image" with a preview', async () => {
        const h = harness();

        await handleFileDrop(drop([file('shot.png', 'image/png', 'PNGDATA')]), 't1', h.opts as any);

        const f = h.tab.attachedFiles[0];
        expect(f.type).toBe('image');
        expect(f.previewUrl).toBe(`data:image/png;base64,${btoa('PNGDATA')}`);
        expect(f.content).toBe(btoa('PNGDATA')); // base64 only, header stripped
    });

    it('surfaces a failed read instead of swallowing it', async () => {
        // The silence is the bug: no `onerror` meant a dead drop produced no
        // toast, no console error, nothing.
        const h = harness();
        dragStoreAlive = false; // the drop is already too late

        await handleFileDrop(drop([file('a.txt', 'text/plain')]), 't1', h.opts as any);

        expect(h.tab.attachedFiles).toHaveLength(0);
        expect(h.toasts).toHaveLength(1);
        expect(h.toasts[0].type).toBe('error');
        expect(h.toasts[0].msg).toContain('a.txt');
    });

    it('keeps the readable files when one of them fails', async () => {
        const h = harness();
        const bad: FakeFile = { ...file('locked.txt', 'text/plain'), broken: true };

        await handleFileDrop(drop([file('a.txt', 'text/plain'), bad, file('c.txt', 'text/plain')]), 't1', h.opts as any);

        expect(h.tab.attachedFiles.map((f: any) => f.name)).toEqual(['a.txt', 'c.txt']);
        expect(h.toasts[0].msg).toContain('locked.txt');
    });

    it('does not attach a file already on the tab', async () => {
        const h = harness();
        h.tab.attachedFiles.push({ name: 'a.txt', content: 'ya estaba', type: 'text' });

        await handleFileDrop(drop([file('a.txt', 'text/plain')]), 't1', h.opts as any);

        expect(h.tab.attachedFiles).toHaveLength(1);
        expect(h.tab.attachedFiles[0].content).toBe('ya estaba');
    });

    it('caps a huge text file so one drop cannot OOM the webview', async () => {
        const h = harness();

        await handleFileDrop(drop([file('huge.log', 'text/plain', 'x'.repeat(250_000))]), 't1', h.opts as any);

        expect(h.tab.attachedFiles[0].content).toHaveLength(200_000);
    });

    it('ignores a drop that carries no files at all', async () => {
        const h = harness();
        expect(startReadingDrop(null)).toEqual([]);
        expect(startReadingDrop(undefined)).toEqual([]);
        await collectDroppedFiles('t1', [], h.opts as any);
        expect(h.tab.attachedFiles).toHaveLength(0);
    });
});
