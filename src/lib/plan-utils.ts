// ── plan-utils.ts — PLAN/ACT/VERIFY and prompt analysis utilities ─────────────
// Pure functions — no Svelte, no Tauri. Safe to unit-test independently.

// ── toDryRunCmd ───────────────────────────────────────────────────────────────
// Wraps a command in a dry-run equivalent for the given execution engine.
export function toDryRunCmd(cmd: string, engine: string): string {
    if (!cmd) return cmd;
    const e = (engine || 'powershell').toLowerCase();
    if (e.startsWith('power') || e === 'local') {
        if (/-WhatIf\b/i.test(cmd)) return cmd;
        if (/\b(Stop|Restart|Remove|Set|Disable|Uninstall|Reset)-\w+/i.test(cmd)) {
            return cmd.trim() + ' -WhatIf';
        }
        return `Write-Host "DRY-RUN — would execute:"; Write-Host ${JSON.stringify(cmd)}`;
    }
    return `echo "DRY-RUN — would execute:" && echo ${JSON.stringify(cmd)}`;
}

// ── PlanStep ──────────────────────────────────────────────────────────────────
export interface PlanStep {
    raw:      string;
    risk:     string;
    target:   string;
    engine:   string;
    desc:     string;
    cmd:      string;
    verify:   string;
    rollback: string;
}

// ── parsePlanTags ─────────────────────────────────────────────────────────────
// Extracts <PLAN ...>...</PLAN> blocks from an LLM response.
export function parsePlanTags(text: string): PlanStep[] {
    if (!text || !text.includes('<PLAN')) return [];
    const out: PlanStep[] = [];
    const re = /<PLAN\s*([^>]*)>([\s\S]*?)<\/PLAN>/gi;
    let m: RegExpExecArray | null;
    while ((m = re.exec(text)) !== null) {
        const attrs = m[1] || '';
        const body  = m[2] || '';
        const getAttr = (name: string) => {
            const r = new RegExp(`${name}=["']([^"']+)["']`, 'i');
            return (attrs.match(r) || [])[1] || '';
        };
        const getChild = (tag: string) => {
            const r = new RegExp(`<${tag}>([\\s\\S]*?)<\\/${tag}>`, 'i');
            return ((body.match(r) || [])[1] || '').trim();
        };
        out.push({
            raw:      m[0],
            risk:     (getAttr('risk') || 'med').toLowerCase(),
            target:   getAttr('target') || 'local',
            engine:   (getAttr('engine') || 'powershell').toLowerCase(),
            desc:     getChild('DESC') || '(sin descripción)',
            cmd:      getChild('CMD'),
            verify:   getChild('VERIFY'),
            rollback: getChild('ROLLBACK'),
        });
    }
    return out;
}

// ── renderPlanCard ────────────────────────────────────────────────────────────
// Builds the interactive plan card HTML injected into Lucy messages.
export function renderPlanCard(plan: PlanStep, planId: string): string {
    const riskCfg = ({
        high: { fg: '#ef4444', bg: 'rgba(239,68,68,.08)',    bd: '#ef4444', label: 'RIESGO ALTO'  },
        med:  { fg: '#d97706', bg: 'rgba(217,119,6,.08)',    bd: '#fbbf24', label: 'RIESGO MEDIO' },
        low:  { fg: '#10b981', bg: 'rgba(16,185,129,.08)',   bd: '#34d399', label: 'RIESGO BAJO'  },
    } as Record<string, any>)[plan.risk] ?? { fg: '#64748b', bg: 'rgba(100,116,139,.08)', bd: '#94a3b8', label: 'RIESGO ?' };
    const esc = (s: any) => String(s || '').replace(/</g, '&lt;').replace(/>/g, '&gt;');
    const targetLabel = plan.target === 'local' ? 'Local' : `Remote (${esc(plan.target)})`;
    return `<div class="plan-card" data-plan-card-id="${planId}" style="margin:10px 0;padding:12px;border-left:4px solid ${riskCfg.bd};background:${riskCfg.bg};border-radius:4px;font-size:12px;">
        <div style="display:flex;align-items:center;gap:10px;margin-bottom:8px;">
            <span style="font-weight:700;color:${riskCfg.fg};letter-spacing:0.5px;font-size:11px;">⚑ PLAN · ${riskCfg.label}</span>
            <span style="color:var(--txt2,#94a3b8);font-size:10px;">${targetLabel} · ${esc(plan.engine)}</span>
        </div>
        <div style="margin-bottom:10px;color:var(--txt,#e5e7eb);font-size:13px;">${esc(plan.desc)}</div>
        <div style="margin-bottom:6px;"><span style="color:var(--txt2,#94a3b8);font-size:10px;">▸ CMD</span><pre style="margin:3px 0;padding:6px 8px;background:rgba(0,0,0,.25);border-radius:3px;font-size:11px;color:#e5e7eb;white-space:pre-wrap;">${esc(plan.cmd)}</pre></div>
        ${plan.verify   ? `<div style="margin-bottom:6px;"><span style="color:var(--txt2,#94a3b8);font-size:10px;">▸ VERIFY</span><pre style="margin:3px 0;padding:6px 8px;background:rgba(0,0,0,.18);border-radius:3px;font-size:11px;color:#cbd5e1;white-space:pre-wrap;">${esc(plan.verify)}</pre></div>` : ''}
        ${plan.rollback ? `<div style="margin-bottom:6px;"><span style="color:var(--txt2,#94a3b8);font-size:10px;">▸ ROLLBACK</span><pre style="margin:3px 0;padding:6px 8px;background:rgba(0,0,0,.18);border-radius:3px;font-size:11px;color:#cbd5e1;white-space:pre-wrap;">${esc(plan.rollback)}</pre></div>` : ''}
        <div style="display:flex;gap:6px;margin-top:10px;flex-wrap:wrap;">
            <button data-plan-id="${planId}" data-plan-action="execute" style="padding:5px 12px;background:${riskCfg.fg};color:#fff;border:none;border-radius:3px;font-size:11px;font-weight:600;cursor:pointer;">▶ Ejecutar</button>
            <button data-plan-id="${planId}" data-plan-action="dryrun"  style="padding:5px 12px;background:transparent;color:#93c5fd;border:1px solid #3b82f6;border-radius:3px;font-size:11px;cursor:pointer;">⚙ Dry-Run</button>
            <button data-plan-id="${planId}" data-plan-action="cancel"  style="padding:5px 12px;background:transparent;color:#94a3b8;border:1px solid #64748b;border-radius:3px;font-size:11px;cursor:pointer;">✕ Cancelar</button>
        </div>
    </div>`;
}

// ── isMultiIntentPrompt ───────────────────────────────────────────────────────
// Heuristic: returns true if the user's text asks for ≥2 independent things.
// Used to skip the quick-tool short-circuit when there's a compound request.
//
// v1.7.49 — Expanded after a user-reported bug where the prompt
// "genera un reporte detallado del estado de mi maquina, tanto a nivel
// seguridad como de rendimiento, el reporte depositalo en mi escritorio"
// short-circuited through the sysinfo quick-tool branch in `+page.svelte`.
// That branch only runs `get_system_health`, dumps the raw output, and
// returns — so the security analysis, the synthesis report, and the
// requested writefile-to-desktop never happened. The prompt was clearly
// multi-intent (gather + analyse two dimensions + write to disk) but the
// regex didn't catch the verbs ("genera", "depositalo") or the
// compound-noun pattern ("reporte detallado").
//
// New detectors added below:
//   • Report-generation verbs: genera / produce / elabora / compila /
//     redacta / construye + their English equivalents.
//   • File-output verbs and targets: deposita / guarda en / exporta /
//     save to / desktop / escritorio / .md / .pdf / etc.
//   • Compound analysis dimensions: when the prompt joins ≥2 analysis
//     axes with "tanto X como Y", "X y Y", "seguridad y rendimiento",
//     etc. these are multi-step by definition.
export function isMultiIntentPrompt(text: string): boolean {
    if (!text || typeof text !== 'string') return false;
    const p = text.toLowerCase();
    // 1. Sequencing connectors that imply "do X, then Y"
    const seq = /\b(?:y\s+(?:luego|despu[eé]s|tambi[eé]n|haz|busca|verifica|comprueba|checa|investiga|consulta|compara)|luego|despu[eé]s|tras\s+eso|antes\s+(?:de\s+|checa|verifica|haz)|una\s+vez|con\s+eso|entonces|adem[aá]s|posteriormente|then|after\s+that|once\s+you)\b/i;
    if (seq.test(p)) return true;
    // 2. Multiple imperative verbs (≥2) → multi-step intent. v1.7.49 expanded
    //    the verb set to include report-generation, document production, and
    //    file-output language.
    const verbs = /\b(verifica|busca|investiga|checa|chequea|consulta|compara|analiza|haz|hazlo|dame|mu[eé]strame|lista|ejecuta|corre|instala|actualiza|descarga|guarda|crea|edita|abre|env[ií]a|prueba|valida|revisa|inspecciona|detecta|genera|gen[eé]rame|produce|prod[uú]ceme|elabora|elab[oó]rame|compila|comp[ií]lame|redacta|red[aá]ctame|construye|constr[uú]yeme|deposita|dep[oó]sit[aá]lo|exporta|exp[oó]rtalo|env[ií]alo|gu[aá]rdalo|search|check|verify|investigate|analyze|compare|list|run|create|edit|fetch|download|install|update|generate|produce|build|compile|export|deposit|save\s+to|write\s+to)\b/g;
    const matches = p.match(verbs);
    if (matches && matches.length >= 2) return true;
    // 3. Web/research request paired with hardware/system action
    const wantsWeb   = /\b(internet|web|google|busca\s+en\s+l[ií]nea|search\s+online|investiga\s+en|search\s+the\s+web|navega)\b/i.test(p);
    const wantsLocal = /\b(specs?|especificaciones|hardware|sistema|gpu|cpu|memoria|disco|configuraci[oó]n|configuracion)\b/i.test(p);
    if (wantsWeb && wantsLocal) return true;
    // 4. v1.7.49 — File-output intent: ANY mention of writing the result to
    //    disk / a file / a folder / the desktop is automatically multi-step
    //    because it requires a `writefile` TOOL on top of whatever else the
    //    prompt asks. The short-circuit branches in +page.svelte never
    //    invoke writefile, so without this guard the user's "save to disk"
    //    requirement is silently dropped.
    const wantsFileOutput = /\b(escritorio|desktop|en\s+(?:un\s+)?archivo|to\s+(?:a\s+)?file|guarda\s+(?:en|el|la|lo)|guardar\s+(?:en|el|la|lo|como)|save\s+(?:as|to|it)|exporta(?:lo)?|expor[tt]ar|export(?:\s+it|\s+to)?|deposita(?:lo)?|dep[oó]sit[aá](?:lo|melo|me)?|dejame\s+(?:un|el)|d[eé]jalo\s+en|en\s+(?:mi\s+)?(?:carpeta|escritorio|documentos|downloads|descargas)|\.(?:md|txt|pdf|json|csv|html|docx?|xlsx?)\b|c:\\\\)/i;
    if (wantsFileOutput.test(p)) return true;
    // 5. v1.7.49 — Compound analysis dimensions: "tanto X como Y" /
    //    "tanto a nivel X como Y" / explicit joiners of ≥2 axes like
    //    seguridad, rendimiento, salud, performance, integridad, etc.
    //    A request that names ≥2 analysis dimensions cannot be answered
    //    by a single quick-tool short-circuit (sysinfo alone covers
    //    rendimiento, not seguridad).
    if (/\btanto\b[\s\S]{1,80}\bcomo\b/i.test(p)) return true;
    const analysisAxes = /\b(seguridad|rendimiento|performance|salud|integridad|cumplimiento|compliance|forense|forensic|amenazas?|threats?|vulnerabilidades?|vulnerabilit(?:y|ies)|configuraci[oó]n|hardening|disponibilidad|availability|estabilidad|backup|copia\s+de\s+seguridad)\b/g;
    const axesMatches = p.match(analysisAxes);
    if (axesMatches && new Set(axesMatches).size >= 2) return true;
    // 6. v1.7.49 — "Reporte detallado / completo / exhaustivo / ejecutivo /
    //    integral" almost always means "gather many signals + synthesize".
    //    A single sysinfo dump never satisfies a request phrased that way.
    if (/\b(reporte|informe|report)\s+(?:detallado|completo|exhaustivo|ejecutivo|integral|comprehensive|detailed|full|executive)\b/i.test(p)) return true;
    return false;
}
