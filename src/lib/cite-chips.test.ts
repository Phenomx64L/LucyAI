import { describe, it, expect } from 'vitest';
import { addCiteChips } from './cite-chips';

describe('cite-chips: addCiteChips', () => {
    it('wraps a Windows file path', () => {
        const out = addCiteChips('Reading C:\\Users\\eleue\\config.json now.');
        expect(out).toContain('class="cite-chip cite-file"');
        expect(out).toContain('data-cite-value="C:\\Users\\eleue\\config.json"');
    });

    it('wraps a POSIX absolute path with extension', () => {
        const out = addCiteChips('Inspecting /var/log/syslog.txt today.');
        expect(out).toContain('cite-file');
        expect(out).toContain('data-cite-value="/var/log/syslog.txt"');
    });

    it('wraps relative dotted paths', () => {
        const out = addCiteChips('See ./src/lib/foo.ts for details.');
        expect(out).toContain('cite-file');
        expect(out).toContain('./src/lib/foo.ts');
    });

    it('wraps Spanish memoria #N references', () => {
        const out = addCiteChips('Como vimos en memoria #42 antes.');
        expect(out).toContain('cite-memory');
        expect(out).toContain('data-cite-value="42"');
    });

    it('wraps English memory #N references', () => {
        const out = addCiteChips('As noted in memory #7 earlier.');
        expect(out).toContain('cite-memory');
        expect(out).toContain('data-cite-value="7"');
    });

    it('wraps [mem:N] bracket form', () => {
        const out = addCiteChips('Reference [mem:128] applies.');
        expect(out).toContain('cite-memory');
        expect(out).toContain('data-cite-value="128"');
    });

    it('wraps @host names', () => {
        const out = addCiteChips('Running on @server-prod returned ok.');
        expect(out).toContain('cite-host');
        expect(out).toContain('data-cite-value="server-prod"');
    });

    it('does NOT wrap email addresses', () => {
        const out = addCiteChips('Send to admin@example.com please.');
        expect(out).not.toContain('cite-host');
    });

    it('wraps plain http(s) URLs', () => {
        const out = addCiteChips('See https://example.com/x for spec.');
        expect(out).toContain('cite-url');
    });

    it('does NOT touch text inside <code>', () => {
        const html = '<code>memoria #42</code> outside memoria #43.';
        const out = addCiteChips(html);
        // The code block stays untouched
        expect(out.indexOf('<code>memoria #42</code>')).toBeGreaterThanOrEqual(0);
        // But the outside one IS wrapped
        expect(out).toContain('data-cite-value="43"');
    });

    it('does NOT touch text inside <pre>', () => {
        const html = '<pre>C:\\evil\\path.txt</pre> outside C:\\real\\file.log';
        const out = addCiteChips(html);
        expect(out).toContain('<pre>C:\\evil\\path.txt</pre>');
        expect(out).toContain('data-cite-value="C:\\real\\file.log"');
    });

    it('does NOT touch text inside <a> tags', () => {
        const html = '<a href="https://x.com">@foo</a> and @bar outside.';
        const out = addCiteChips(html);
        // <a> contents preserved
        expect(out).toMatch(/<a[^>]*>@foo<\/a>/);
        // @bar still wrapped
        expect(out).toContain('data-cite-value="bar"');
    });

    it('is idempotent — second pass produces same output', () => {
        const once  = addCiteChips('See @host1 and /tmp/x.log please.');
        const twice = addCiteChips(once);
        expect(twice).toEqual(once);
    });

    it('returns input unchanged when no entities present', () => {
        const input = 'Just plain text with nothing notable.';
        expect(addCiteChips(input)).toBe(input);
    });

    it('handles empty and null gracefully', () => {
        expect(addCiteChips('')).toBe('');
        // @ts-expect-error testing runtime tolerance
        expect(addCiteChips(null)).toBe(null);
        // @ts-expect-error testing runtime tolerance
        expect(addCiteChips(undefined)).toBe(undefined);
    });

    it('escapes quotes in path attribute values', () => {
        const out = addCiteChips('Path C:\\with"quote\\file.txt here.');
        // Should escape the quote inside data-cite-value
        expect(out).not.toContain('data-cite-value="C:\\with"quote');
    });

    it('does NOT corrupt user prose containing numbers (digit-restore regression)', () => {
        // Code-review caught: the placeholder/stash scheme used bare digits
        // as keys and restored via /(\d+)/g, which clobbered numbers in
        // ordinary prose. The fix is non-collidable \x01 sentinels. This
        // test passes Lucy text with "port 8080" and "error 42" alongside
        // a chip-worthy entity and asserts both the digits AND the chip
        // survive intact.
        const out = addCiteChips(
            'Saw error 42 on port 8080 while reading /var/log/syslog.log please.'
        );
        // Digits in prose must remain visible verbatim.
        expect(out).toContain('error 42');
        expect(out).toContain('port 8080');
        // The file path must still get wrapped.
        expect(out).toContain('data-cite-value="/var/log/syslog.log"');
        // Control-char sentinels must NEVER leak to the user-visible output.
        expect(out).not.toMatch(/[\x01\x02]/);
    });
});
