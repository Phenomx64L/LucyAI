import { describe, it, expect } from 'vitest';
import { detectElevationError, detectPlanLogicalFailure } from './plan-detect';

describe('detectElevationError', () => {
    it('detects English + Spanish elevation failures', () => {
        expect(detectElevationError('Access is denied')).toBe(true);
        expect(detectElevationError('Acceso denegado')).toBe(true);
        expect(detectElevationError('UnauthorizedAccessException')).toBe(true);
        expect(detectElevationError('CouldNotStopService')).toBe(true);
        expect(detectElevationError('Run as administrator')).toBe(true);
    });
    it('returns false for normal output / empty', () => {
        expect(detectElevationError('Service stopped successfully')).toBe(false);
        expect(detectElevationError('')).toBe(false);
        expect(detectElevationError(null)).toBe(false);
    });
});

describe('detectPlanLogicalFailure', () => {
    it('catches a stop that left the service running', () => {
        expect(detectPlanLogicalFailure('Stop-Service Spooler', 'Status: Running'))
            .toMatch(/sigue ACTIVO/);
    });
    it('catches a start that left the service stopped', () => {
        expect(detectPlanLogicalFailure('Start-Service Spooler', 'Status: Stopped'))
            .toMatch(/sigue DETENIDO/);
    });
    it('catches a delete where Test-Path still returns true', () => {
        expect(detectPlanLogicalFailure('Remove-Item x.txt', 'Test-Path x.txt -> True'))
            .toMatch(/sigue existiendo/);
    });
    it('returns null when intent and verify agree', () => {
        expect(detectPlanLogicalFailure('Stop-Service Spooler', 'Status: Stopped')).toBeNull();
        expect(detectPlanLogicalFailure('', 'anything')).toBeNull();
        expect(detectPlanLogicalFailure('Stop-Service x', '')).toBeNull();
    });
});
