// ── plan-detect.ts — pure detectors for the PLAN/ACT/VERIFY flow ─────────────
//
// Extracted from +page.svelte (refactor, v1.7.197). Pure string analysis used
// by executePlan to decide whether a step needs elevation or silently failed.

/** Does this output indicate the command failed for lack of admin rights? */
export function detectElevationError(text: string | null | undefined): boolean {
    if (!text) return false;
    return /PermissionDenied|Acceso\s+denegado|Access\s+is\s+denied|Access\s+denied|requires?\s+elevation|UnauthorizedAccess|No\s+se\s+puede\s+abrir\s+el\s+servicio.*en\s+el\s+equipo|CouldNot(Stop|Start|Set|Restart|Pause|Resume)Service|necesita.*admin|Run\s+as\s+administrator/i.test(String(text));
}

/**
 * Compare CMD intent against VERIFY output to catch the case where the command
 * "succeeded" (no exception) but didn't actually do what was asked. Returns a
 * human-readable diagnostic string, or null if no mismatch found.
 */
export function detectPlanLogicalFailure(cmd: string | null | undefined, verifyOut: string | null | undefined): string | null {
    if (!cmd || !verifyOut) return null;
    const c = String(cmd).toLowerCase();
    const v = String(verifyOut).toLowerCase();

    // Service control mismatches
    if (/\bstop-service\b|\bstop-process\b/.test(c) && /\brunning\b/.test(v)) {
        return 'El comando intentó DETENER pero VERIFY muestra que sigue ACTIVO.';
    }
    if (/\bstart-service\b/.test(c) && /\bstopped\b/.test(v) && !/running/.test(v)) {
        return 'El comando intentó ARRANCAR pero VERIFY muestra que sigue DETENIDO.';
    }
    if (/\brestart-service\b/.test(c) && /\bstopped\b/.test(v) && !/running/.test(v)) {
        return 'El comando intentó REINICIAR pero VERIFY muestra el servicio DETENIDO (sólo paró, no arrancó).';
    }
    // Disable / Enable mismatches
    if (/\bdisable-/.test(c) && /\benabled\s*:\s*true\b|\bstatus\s*:\s*enabled\b/i.test(verifyOut)) {
        return 'El comando intentó DESHABILITAR pero VERIFY muestra que sigue HABILITADO.';
    }
    // File deletion mismatches (when VERIFY uses Test-Path)
    if (/\bremove-item\b|\bdel\s/.test(c) && /\btest-path/i.test(cmd + ' ' + verifyOut) && /\btrue\b/.test(v)) {
        return 'El comando intentó BORRAR pero VERIFY muestra que el archivo/carpeta sigue existiendo.';
    }
    return null;
}
