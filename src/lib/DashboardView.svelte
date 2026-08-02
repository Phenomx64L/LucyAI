<script>
    // v1.4.25 — alert family extracted to a single global stylesheet.
    // Closes the v1.4.20 CSS-dedup migration backlog (5/5).
    import '$lib/styles/dashboard-alerts.css';
    import { onMount, onDestroy, createEventDispatcher } from 'svelte';
    import { invoke } from '@tauri-apps/api/core';
    import { countUp } from '$lib/actions';
    import { lucyConfirm } from '$lib/dialog-service';
    import BarChart3 from '@tabler/icons-svelte/icons/chart-bar';

    import Bell from '@tabler/icons-svelte/icons/bell';

    import AlertTriangle from '@tabler/icons-svelte/icons/alert-triangle';

    import TrendingUp from '@tabler/icons-svelte/icons/trending-up';

    import Heartbeat from '@tabler/icons-svelte/icons/activity-heartbeat';
    import { detectAnomaly } from '$lib/anomaly';
    import { reportAnomaly } from '$lib/anomaly-bridge';
    import { safeParseLS, safeSetLS } from '$lib/safe-ls';
    import CpuHeatmap from '$lib/CpuHeatmap.svelte';
    import { markHostReachable, activeAlerts, alertRules } from '$lib/stores';

    const dispatch = createEventDispatcher();

    // ── Props ────────────────────────────────────────────────────────────────
    export let hosts       = [];
    export let hostName    = '';
    // svelte-ignore export_let_unused
    export let lucyConfig  = {};
    export let userLang    = 'es-MX';
    export let isEN        = false;

    // ── Internal state ───────────────────────────────────────────────────────
    let dashMetrics        = null;
    let dashLoading        = false;
    let dashError          = '';
    let dashSelectedHost   = 'local';
    let dashRefreshTimer   = null;
    let dashLastUpdate     = '';
    let _dashStartId       = 0;   // guard contra race conditions en startDashboard()

    // ── Metrics history (sparklines) ─────────────────────────────────────────
    let metricsHistory     = {};
    const METRICS_HIST_MAX = 20;

    // ── Tier A #3 (sprint extra) — Capacity projection overlay ──────────
    // Lazy-loaded per host. Refreshes when the user clicks "predict" or on
    // a 10-min timer. We DO NOT call it on every metrics tick — the
    // underlying regression is over 7-14 days of samples, recomputing
    // every 5s would be wasteful.
    let capacityProjections = {};  // { [hostId]: ProjectionOverlay | null }
    let projectionLoading   = false;

    // ── Sprint C — D15/D17/D18 integration state ────────────────────────
    // All three fetch independently of metrics and refresh on slower cadences
    // (15-90s) because their underlying data changes less often.
    let openIncidents   = { open_count: 0, latest_title: '', latest_id: '' };
    let failedLogins    = { available: false, count_24h: 0, note: '' };
    /** PID → { first_seen, is_new_24h } — populated after each metrics tick */
    let processLineage  = new Map();

    async function refreshOpenIncidents() {
        try {
            const targetHost = dashSelectedHost === 'local' ? '' : dashSelectedHost;
            openIncidents = await invoke('dashboard_open_incidents', { hostName: targetHost });
        } catch {
            openIncidents = { open_count: 0, latest_title: '', latest_id: '' };
        }
    }

    // De-dupe guard: on open this fires twice (onMount + the host-init reactive
    // below), which used to spawn the Get-WinEvent PowerShell twice (the "two
    // windows"). Skip the redundant concurrent call.
    let _flInFlight = false;
    async function refreshFailedLogins() {
        // Only meaningful for local host today (no remote Get-WinEvent yet).
        if (dashSelectedHost !== 'local') {
            failedLogins = { available: false, count_24h: 0, note: 'Local only' };
            return;
        }
        if (_flInFlight) return;
        _flInFlight = true;
        try { failedLogins = await invoke('dashboard_failed_logins_24h'); }
        catch (e) { failedLogins = { available: false, count_24h: 0, note: String(e) }; }
        finally { _flInFlight = false; }
    }

    async function refreshProcessLineage(topProcesses) {
        if (!topProcesses?.length) { processLineage = new Map(); return; }
        const pids = topProcesses.map(p => Number(p.pid)).filter(p => p > 0);
        if (!pids.length) { processLineage = new Map(); return; }
        try {
            const rows = await invoke('dashboard_process_lineage_brief', { pids });
            const m = new Map();
            for (const r of rows) m.set(r.pid, r);
            processLineage = m;
        } catch { processLineage = new Map(); }
    }

    function fmtRelHours(unixSec) {
        const d = Math.floor(Date.now()/1000) - unixSec;
        if (d < 60)    return isEN ? 'just now' : 'ahora';
        if (d < 3600)  return `${Math.floor(d/60)}m`;
        if (d < 86400) return `${Math.floor(d/3600)}h`;
        return `${Math.floor(d/86400)}d`;
    }
    async function refreshCapacityProjection(hostId = dashSelectedHost) {
        if (projectionLoading) return;
        projectionLoading = true;
        try {
            const overlay = await invoke('capacity_projection', {
                hostId, days: 14, forecastDays: 7, forecastPoints: 24,
            });
            capacityProjections = { ...capacityProjections, [hostId]: overlay };
        } catch (e) {
            capacityProjections = { ...capacityProjections, [hostId]: null };
        } finally {
            projectionLoading = false;
        }
    }
    /** Format the OLS slope into "12d to 95%" or "stable" or "↘ shrinking". */
    function projectionLabel(regression, threshold = 95) {
        if (!regression || regression.samples_used < 5) return null;
        const slope = regression.slope; // %/day
        if (Math.abs(slope) < 0.05) return { text: '~ stable', cls: 'pj-stable' };
        if (slope < 0) return { text: '↘ shrinking', cls: 'pj-stable' };
        // Current value from the projection's first point
        const cur = regression.projection?.[0]?.[1] ?? 0;
        if (cur >= threshold) return { text: 'AT THRESHOLD', cls: 'pj-crit' };
        const days = (threshold - cur) / slope;
        if (days > 365) return { text: '>1y to ' + threshold + '%', cls: 'pj-stable' };
        if (days <= 7)  return { text: '↗ ' + Math.round(days) + 'd to ' + threshold + '%', cls: 'pj-crit' };
        if (days <= 30) return { text: '↗ ' + Math.round(days) + 'd to ' + threshold + '%', cls: 'pj-warn' };
        return            { text: '↗ ' + Math.round(days) + 'd to ' + threshold + '%', cls: 'pj-ok' };
    }

    // ── Proactive alerts ─────────────────────────────────────────────────────
    // `alertRules` is the shared persisted store, not a local — same fix as
    // `activeAlerts` above, and for a sharper reason: this component and the
    // Alertas Proactivas modal in +page.svelte both persisted to the SAME
    // localStorage key ('lucy_alert_rules') from SEPARATE in-memory arrays.
    // Whichever saved last wrote its stale copy over the other's, so a rule
    // added in one surface disappeared when you added one in the other.
    // `activeAlerts` is the shared store now, not a local. It used to be both:
    // a private array here that drove the bell badge, and an untouched store in
    // stores.ts that the Alertas Proactivas modal in +page.svelte read — and so
    // that modal's "Disparadas ahora" list was always empty.
    let showAlertsModal    = false;
    // Same annotation as the modal's copy in +page.svelte: `metric` widens to
    // `string` in a bare literal, and `AlertRule.metric` is a union. Now that
    // both editors write the same store, both had to say so.
    /** @type {{ hostId: string, metric: 'cpu'|'ram'|'disk', threshold: number, enabled: boolean }} */
    let alertForm          = { hostId:'all', metric:'cpu', threshold:85, enabled:true };

    // ── Helpers ──────────────────────────────────────────────────────────────

    function toast(msg, type = 'info') {
        dispatch('toast', { msg, type });
    }

    // ── Severity color encoding (UNIFIED across dashboard) ──────────────────
    // Standard thresholds: warn ≥60, critical ≥80 (CPU/RAM); disk uses 75/90.
    // Returns CSS var name OR hex (when needed for inline SVG sparklines).
    const _SEV_HEX = { critical:'#ef4444', warn:'#f59e0b', okGreen:'#10b981', okBlue:'#3b9eff' };
    function sevVar(v, okVar = 'var(--acc)', { warn = 60, crit = 80 } = {}) {
        if (v >= crit) return 'var(--red)';
        if (v >= warn) return 'var(--amber)';
        return okVar;
    }
    function sevHex(v, okHex = _SEV_HEX.okGreen, { warn = 60, crit = 80 } = {}) {
        if (v >= crit) return _SEV_HEX.critical;
        if (v >= warn) return _SEV_HEX.warn;
        return okHex;
    }
    // Disk uses higher thresholds — storage tolerates more before being critical.
    const diskSevVar = (v) => sevVar(v, 'var(--blue)', { warn: 75, crit: 90 });

    // ── Sprint C D14 — Per-host editable thresholds ─────────────────────
    // Keyed by `${hostId}__${metric}` → { warn, crit }. Defaults below mirror
    // what was hardcoded before — the user only sees a change if they
    // explicitly edit. Persisted in localStorage.
    const DEFAULT_THRESHOLDS = {
        cpu:  { warn: 60, crit: 80 },
        ram:  { warn: 60, crit: 80 },
        swap: { warn: 50, crit: 80 },
    };
    let customThresholds = safeParseLS('lucy_thresholds', {});
    function getThresholds(metric, hostId = dashSelectedHost) {
        const key = `${hostId}__${metric}`;
        return customThresholds[key] || DEFAULT_THRESHOLDS[metric] || { warn: 60, crit: 80 };
    }
    function setThresholds(metric, warn, crit, hostId = dashSelectedHost) {
        const key = `${hostId}__${metric}`;
        // Sanity: warn must be < crit, both in [1, 99].
        warn = Math.max(1, Math.min(99, Math.round(Number(warn) || 60)));
        crit = Math.max(warn + 1, Math.min(99, Math.round(Number(crit) || 80)));
        customThresholds = { ...customThresholds, [key]: { warn, crit } };
        safeSetLS('lucy_thresholds', customThresholds);
    }
    function resetThresholds(metric, hostId = dashSelectedHost) {
        const key = `${hostId}__${metric}`;
        const next = { ...customThresholds };
        delete next[key];
        customThresholds = next;
        safeSetLS('lucy_thresholds', customThresholds);
    }
    /** Returns the color var based on custom (or default) thresholds. */
    function sevVarFor(metric, value, okVar) {
        const t = getThresholds(metric);
        return sevVar(value, okVar, { warn: t.warn, crit: t.crit });
    }
    function sevHexFor(metric, value, okHex) {
        const t = getThresholds(metric);
        return sevHex(value, okHex, { warn: t.warn, crit: t.crit });
    }
    /** UI state: which threshold editor is open. null = none. */
    let editingThresholdsFor = null;  // 'cpu' | 'ram' | 'swap' | null

    // ── Sprint E D11 — Reorderable & hideable Dashboard sections ────────
    // Three sections live below the cards row: CPU cores heatmap, storage
    // list, and top processes. Each user has different priorities — an SRE
    // chasing disk pressure wants storage on top; a perf engineer wants
    // processes. Persist their preference and let them rearrange via drag.
    //
    // Why we DON'T allow reordering individual cards in the top row: those
    // are at-a-glance KPIs (CPU, RAM, System, Page file, Temps, Failed
    // logins, Network) and they auto-flow with CSS. Reordering within a
    // flex-wrap container is visually subtle anyway. Sections are the
    // unit the user actually cares about.
    const DEFAULT_SECTION_ORDER = ['cores', 'storage', 'processes'];
    let sectionOrder = safeParseLS('lucy_dashboard_section_order', DEFAULT_SECTION_ORDER);
    let hiddenSections = new Set(safeParseLS('lucy_dashboard_section_hidden', []));
    // Sanity: if storage adds new sections in a future build, append them.
    for (const k of DEFAULT_SECTION_ORDER) {
        if (!sectionOrder.includes(k)) sectionOrder = [...sectionOrder, k];
    }

    let _dragFrom = null;  // index being dragged
    function onSectionDragStart(ev, idx) {
        _dragFrom = idx;
        try { ev.dataTransfer.effectAllowed = 'move'; } catch {}
    }
    function onSectionDragOver(ev) { ev.preventDefault(); }
    function onSectionDrop(ev, toIdx) {
        ev.preventDefault();
        if (_dragFrom == null || _dragFrom === toIdx) { _dragFrom = null; return; }
        const next = [...sectionOrder];
        const [item] = next.splice(_dragFrom, 1);
        next.splice(toIdx, 0, item);
        sectionOrder = next;
        safeSetLS('lucy_dashboard_section_order', sectionOrder);
        _dragFrom = null;
    }
    function toggleSectionHidden(key) {
        const next = new Set(hiddenSections);
        if (next.has(key)) next.delete(key); else next.add(key);
        hiddenSections = next;
        safeSetLS('lucy_dashboard_section_hidden', [...next]);
    }
    function resetSectionLayout() {
        sectionOrder = DEFAULT_SECTION_ORDER.slice();
        hiddenSections = new Set();
        safeSetLS('lucy_dashboard_section_order', sectionOrder);
        safeSetLS('lucy_dashboard_section_hidden', []);
    }
    function sectionTitle(key) {
        switch (key) {
            case 'cores':     return isEN ? 'CPU Cores'              : 'Núcleos CPU';
            case 'storage':   return isEN ? 'Storage'                : 'Almacenamiento';
            case 'processes': return isEN ? 'Top Processes (by RAM)' : 'Top procesos (por RAM)';
            default:          return key;
        }
    }
    let _thrDraft = { warn: 60, crit: 80 };
    function openThresholdEditor(metric) {
        const t = getThresholds(metric);
        _thrDraft = { warn: t.warn, crit: t.crit };
        editingThresholdsFor = metric;
    }
    function saveThresholdDraft() {
        if (!editingThresholdsFor) return;
        setThresholds(editingThresholdsFor, _thrDraft.warn, _thrDraft.crit);
        editingThresholdsFor = null;
    }
    // CPU per-core: same thresholds, returns rgba for the bar fill (with ok-shade variation).
    function coreBarColor(v) {
        if (v >= 80) return 'rgba(239,68,68,.85)';
        if (v >= 60) return 'rgba(245,158,11,.80)';
        if (v >= 40) return 'rgba(16,185,129,.65)';
        return 'rgba(16,185,129,.45)';
    }

    // ── Dashboard lifecycle ──────────────────────────────────────────────────

    async function startDashboard() {
        stopDashboard();
        const myId = ++_dashStartId;           // token único para esta invocación
        dashMetrics = null; dashError = '';
        dashLoading = true;
        await refreshDash();
        if (myId !== _dashStartId) return;     // llamada más reciente ya tomó el control
        // Recursive setTimeout instead of setInterval: schedules the NEXT tick only
        // after the current refresh resolves. Prevents overlap when a remote-host
        // fetch takes longer than the 10s cadence (was producing duplicate
        // pushMetricsHistory writes + race conditions on slow hosts).
        _scheduleDashTick(myId);
    }

    /**
     * Internal: schedule the next dashboard refresh tick. Uses the same
     * `_dashStartId` token as startDashboard() so any in-flight tick becomes
     * a no-op the moment stopDashboard() / a new startDashboard() invalidates
     * the session.
     */
    function _scheduleDashTick(sessionId) {
        dashRefreshTimer = setTimeout(async () => {
            if (sessionId !== _dashStartId) return;     // session changed → bail
            await refreshDash();
            if (sessionId !== _dashStartId) return;     // session changed mid-fetch → bail
            _scheduleDashTick(sessionId);                // chain next
        }, 10000);
    }

    function stopDashboard() {
        _dashStartId++;                                  // invalidate pending ticks
        if (dashRefreshTimer) { clearTimeout(dashRefreshTimer); dashRefreshTimer = null; }
    }

    async function refreshDash() {
        dashLoading = true; dashError = '';
        // Race-safe: capture which host this fetch is FOR before any await.
        // If the user switches hosts mid-fetch, we must NOT write the previous
        // host's metrics into the new host's history slot (previously caused
        // mixed sparklines + bogus z-score anomalies on host switch).
        const fetchedFor = dashSelectedHost;
        let fetched = null;
        try {
            if (fetchedFor === 'local') {
                fetched = await invoke('get_system_health_json');
            } else {
                const h = hosts.find(x => x.id === fetchedFor);
                if (!h) { dashError = isEN ? 'Host not found.' : 'Host no encontrado.'; dashLoading = false; return; }
                let pwd = '';
                try { pwd = await invoke('get_host_credential', { hostId: h.id }); } catch(e){}
                if (h.type === 'windows') {
                    fetched = await invoke('get_remote_health_windows', { host:h.host, username:h.username, password:pwd });
                } else {
                    fetched = await invoke('get_remote_health_linux', { host:h.host, username:h.username, port:h.port||22, keyPath:h.sshKeyPath||null });
                }
            }
            // Bail if the user switched hosts during the fetch — discard result.
            if (fetchedFor !== dashSelectedHost) { dashLoading = false; return; }
            dashMetrics = fetched;
            dashLastUpdate = new Date().toLocaleTimeString(userLang, {hour:'2-digit',minute:'2-digit',second:'2-digit'});
            pushMetricsHistory(fetchedFor, dashMetrics);
            checkAlerts(fetchedFor, dashMetrics);
            // ── PostureStrip: mark host as reachable (reconnected v1.4.0) ──
            markHostReachable(fetchedFor, true);
            // Sprint C D18 — Hydrate process_lineage badges for the new top_processes
            refreshProcessLineage(dashMetrics.top_processes);
        } catch(e) {
            // Only set error if we're still on the host that originated this fetch
            if (fetchedFor === dashSelectedHost) {
                dashError = String(e);
                dashMetrics = null;
            }
            // ── PostureStrip: mark host as unreachable ──
            markHostReachable(fetchedFor, false);
        }
        dashLoading = false;
    }

    function onDashHostChange() {
        startDashboard();
    }

    // ── Sparklines / metrics history ─────────────────────────────────────────

    // SECURITY: defensive sanitizer for the `color` arg.
    // Today every caller passes a hardcoded hex from sevHex(), but if a future
    // refactor ever lets user data flow into this function, an unrestricted
    // string could inject arbitrary CSS/SVG via the @html sink that renders this.
    // Whitelist: 7-char hex (`#RRGGBB`) or 9-char hex (`#RRGGBBAA`) only.
    const _SPARK_COLOR_RE = /^#[0-9a-fA-F]{6}([0-9a-fA-F]{2})?$/;
    function _safeColor(c) {
        if (typeof c === 'string' && _SPARK_COLOR_RE.test(c)) return c;
        return '#10b981'; // fallback to brand green
    }

    function sparklineSvg(history, key, color = '#10b981', w = 70, h = 24) {
        const data = (history || []).map(h => h[key] ?? 0);
        if (data.length < 2) return '';
        const safeC = _safeColor(color);  // hex-validated
        const min = Math.min(...data), max = Math.max(...data);
        const range = max - min || 1;
        const pts = data.map((v, i) => {
            const x = (i / (data.length - 1)) * w;
            const y = h - ((v - min) / range) * (h - 4) - 2;
            return `${x.toFixed(1)},${y.toFixed(1)}`;
        }).join(' ');
        const last = pts.trim().split(' ').pop().split(',');
        // Compute approximate path length for stroke-dasharray draw animation.
        // (Cheap rough estimate — exact length would need getTotalLength on a real DOM node.)
        const approxLen = w * 1.6;
        // Build a closed area polygon for a subtle filled gradient under the line
        const areaPts = `0,${h} ${pts} ${w},${h}`;
        // Inline animation via SMIL would be more ergonomic, but CSS keyframes on the path
        // are more performant. Use a unique keyframe per render to retrigger draw on update.
        // SECURITY: drawKey is built from numeric Math.random() → no injection vector.
        const drawKey = `spkdraw-${Math.floor(Math.random() * 100000)}`;
        return `<svg width="${w}" height="${h}" viewBox="0 0 ${w} ${h}" xmlns="http://www.w3.org/2000/svg" class="spark-svg" aria-hidden="true">
            <defs>
              <linearGradient id="spkg-${drawKey}" x1="0" x2="0" y1="0" y2="1">
                <stop offset="0%"   stop-color="${safeC}" stop-opacity="0.32"/>
                <stop offset="100%" stop-color="${safeC}" stop-opacity="0"/>
              </linearGradient>
              <style>
                @keyframes ${drawKey} {
                  0%   { stroke-dashoffset: ${approxLen.toFixed(0)}; opacity: 0.2; }
                  60%  { opacity: 0.85; }
                  100% { stroke-dashoffset: 0; opacity: 0.85; }
                }
                @keyframes ${drawKey}-area {
                  0%   { opacity: 0; }
                  70%  { opacity: 0; }
                  100% { opacity: 1; }
                }
                @keyframes ${drawKey}-dot {
                  0%   { transform: scale(0); opacity: 0; }
                  85%  { transform: scale(0); opacity: 0; }
                  100% { transform: scale(1); opacity: 1; }
                }
                .spk-line-${drawKey} {
                  stroke-dasharray: ${approxLen.toFixed(0)};
                  stroke-dashoffset: ${approxLen.toFixed(0)};
                  animation: ${drawKey} 700ms cubic-bezier(0.16,1,0.3,1) forwards;
                }
                .spk-area-${drawKey} { opacity: 0; animation: ${drawKey}-area 900ms cubic-bezier(0.16,1,0.3,1) forwards; }
                .spk-dot-${drawKey}  { transform-origin: ${last[0]}px ${last[1]}px; transform: scale(0); animation: ${drawKey}-dot 900ms cubic-bezier(0.34,1.56,0.64,1) forwards; }
              </style>
            </defs>
            <polygon class="spk-area-${drawKey}" points="${areaPts}" fill="url(#spkg-${drawKey})"/>
            <polyline class="spk-line-${drawKey}" points="${pts}" fill="none" stroke="${safeC}" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round"/>
            <circle class="spk-dot-${drawKey}" cx="${last[0]}" cy="${last[1]}" r="2.5" fill="${safeC}"/>
        </svg>`;
    }

    function pushMetricsHistory(hostId, metrics) {
        if (!metrics) return;
        if (!metricsHistory[hostId]) metricsHistory[hostId] = [];
        metricsHistory[hostId].push({
            ts:   Date.now(),
            cpu:  metrics.cpu?.global ?? 0,
            ram:  metrics.memory?.percent ?? 0,
            disk: metrics.disks?.length ? Math.max(...metrics.disks.map(d => d.percent)) : 0
        });
        if (metricsHistory[hostId].length > METRICS_HIST_MAX) metricsHistory[hostId].shift();
        metricsHistory = { ...metricsHistory };
        safeSetLS('lucy_metrics_history', metricsHistory);
    }

    // ── Proactive alerts ─────────────────────────────────────────────────────

    function checkAlerts(hostId, metrics) {
        if (!metrics || !$alertRules.length) return;
        const hostLabel = hostId === 'local' ? 'Local' : (hosts.find(h => h.id === hostId)?.name ?? hostId);
        for (const rule of $alertRules.filter(r => r.enabled && (r.hostId === 'all' || r.hostId === hostId))) {
            let value = 0;
            if (rule.metric === 'cpu')  value = metrics.cpu?.global ?? 0;
            if (rule.metric === 'ram')  value = metrics.memory?.percent ?? 0;
            if (rule.metric === 'disk') value = metrics.disks?.length ? Math.max(...metrics.disks.map(d => d.percent)) : 0;
            const aId = `${rule.id}_${hostId}`;
            if (value >= rule.threshold) {
                if (!$activeAlerts.find(a => a.id === aId)) {
                    const al = { id: aId, ruleId: rule.id, hostId, hostLabel, metric: rule.metric.toUpperCase(), value: Math.round(value), threshold: rule.threshold, ts: new Date().toLocaleTimeString() };
                    $activeAlerts = [...$activeAlerts, al];
                    toast(isEN ? `\u26a0\ufe0f ${al.metric} on ${hostLabel}: ${al.value}%` : `\u26a0\ufe0f ${al.metric} en ${hostLabel}: ${al.value}%`, 'warn');
                    try {
                        if (typeof Notification !== 'undefined' && Notification.permission === 'granted') {
                            new Notification(`\u26a0\ufe0f Lucy \u2014 ${al.metric} alto`, { body: `${hostLabel}: ${al.value}% (umbral ${rule.threshold}%)` });
                        }
                    } catch(e) {}
                }
            } else {
                $activeAlerts = $activeAlerts.filter(a => a.id !== aId);
            }
        }
    }

    // `saveAlertRules()` is gone: `alertRules` is a `persistedWritable`, so it
    // writes 'lucy_alert_rules' on every assignment. Keeping a manual save
    // beside it was how the two copies overwrote each other.

    function agregarAlertRule() {
        const thr = Number(alertForm.threshold);
        if (!thr || thr < 1 || thr > 100) return;
        $alertRules = [...$alertRules, { id: `ar_${Date.now()}`, hostId: alertForm.hostId, metric: alertForm.metric, threshold: thr, enabled: true }];
        alertForm = { hostId: 'all', metric: 'cpu', threshold: 85, enabled: true };
    }

    function eliminarAlertRule(id) {
        $alertRules = $alertRules.filter(r => r.id !== id);
        $activeAlerts = $activeAlerts.filter(a => a.ruleId !== id);
    }

    // ── Lifecycle ────────────────────────────────────────────────────────────

    // ── Process table: sortable + right-click actions (D-Proc) ───────────────
    let procSortKey = 'mem_mb';   // 'name' | 'cpu' | 'mem_mb' | 'pid'
    let procSortDir = -1;         // 1 asc, -1 desc
    let procMenu = null;          // { x, y, proc } | null
    const SELF_PROC = 'lucy-svelte.exe';

    $: sortedProcs = (() => {
        const list = [...(dashMetrics?.top_processes ?? [])];
        list.sort((a, b) => {
            if (procSortKey === 'name') return procSortDir * String(a.name||'').localeCompare(String(b.name||''));
            return procSortDir * ((Number(a[procSortKey])||0) - (Number(b[procSortKey])||0));
        });
        return list;
    })();
    function setProcSort(key) {
        if (procSortKey === key) procSortDir = -procSortDir;
        else { procSortKey = key; procSortDir = key === 'name' ? 1 : -1; }
    }
    function openProcMenu(ev, p) { ev.preventDefault(); procMenu = { x: ev.clientX, y: ev.clientY, proc: p }; }
    function closeProcMenu() { procMenu = null; }
    async function killProc(p) {
        closeProcMenu();
        if (!p?.pid) return;
        // In-app confirm (not native window.confirm → no "localhost dice…" box).
        const _ok = await lucyConfirm(
            isEN ? `End "${p.name}"?` : `¿Finalizar "${p.name}"?`,
            {
                description: isEN ? `PID ${p.pid} · this may cause data loss in that app.` : `PID ${p.pid} · puede causar pérdida de datos en esa app.`,
                tone: 'danger',
                confirmLabel: isEN ? 'End task' : 'Finalizar',
                cancelLabel:  isEN ? 'Cancel' : 'Cancelar',
            });
        if (!_ok) return;
        try { await invoke('kill_process', { pid: Number(p.pid) }); toast(isEN ? `Ended ${p.name}` : `Finalizado ${p.name}`, 'info'); refreshDash(); }
        catch (e) { toast('✗ ' + String(e).slice(0, 140), 'warn'); }
    }
    async function revealProc(p) {
        closeProcMenu();
        if (!p?.path) { toast(isEN ? 'No file path for this process' : 'Sin ruta de archivo para este proceso', 'warn'); return; }
        try { await invoke('reveal_in_explorer', { path: p.path }); }
        catch (e) { toast('✗ ' + String(e).slice(0, 140), 'warn'); }
    }
    function askLucyAboutProc(p) {
        closeProcMenu();
        dispatch('askLucy', { text: isEN
            ? `What is the process "${p.name}" (PID ${p.pid})? Is it safe, and is its CPU/RAM usage normal?`
            : `¿Qué es el proceso "${p.name}" (PID ${p.pid})? ¿Es seguro y su uso de CPU/RAM es normal?` });
    }
    async function copyProcPid(p) {
        closeProcMenu();
        try { await invoke('copy_to_clipboard', { text: String(p.pid) }); toast(isEN ? 'PID copied' : 'PID copiado', 'info'); } catch {}
    }

    // ── Failed-logins drill-down (D-Login) ───────────────────────────────────
    let flDetailOpen = false;
    let flDetail = [];
    let flDetailLoading = false;
    async function openFlDetail() {
        flDetailOpen = true; flDetailLoading = true; flDetail = [];
        try { flDetail = await invoke('dashboard_failed_logins_detail'); }
        catch (e) { toast('✗ ' + String(e).slice(0, 140), 'warn'); }
        flDetailLoading = false;
    }

    onMount(() => {
        metricsHistory = safeParseLS('lucy_metrics_history', {});
        // No manual read of 'lucy_alert_rules': `persistedWritable` hydrates
        // from it at module load. Re-reading here is what pinned this
        // component to whatever was on disk at mount, ignoring anything the
        // modal had changed since.
        try { if (typeof Notification !== 'undefined' && Notification.permission === 'default') Notification.requestPermission().catch(() => {}); } catch(e) {}
        startDashboard();
        // Tier A #3 — Capacity projection: initial fetch + 10-min refresh.
        refreshCapacityProjection();
        const _projTimer = setInterval(() => refreshCapacityProjection(), 10 * 60 * 1000);
        // Sprint C — D15 banner refresh (15s) + D17 failed logins (90s)
        refreshOpenIncidents();
        refreshFailedLogins();
        const _incTimer  = setInterval(() => refreshOpenIncidents(), 15 * 1000);
        const _flTimer   = setInterval(() => refreshFailedLogins(), 90 * 1000);
        return () => {
            clearInterval(_projTimer);
            clearInterval(_incTimer);
            clearInterval(_flTimer);
        };
    });

    // Re-fetch the Sprint C integrations when the user switches host.
    let _lastIntegrationsHost = '';
    $: if (dashSelectedHost && dashSelectedHost !== _lastIntegrationsHost) {
        _lastIntegrationsHost = dashSelectedHost;
        refreshOpenIncidents();
        refreshFailedLogins();
    }

    // Re-fetch the projection when the user switches host (each host has
    // its own metric history → its own regression). The condition prevents
    // a refetch loop: only fire when the host actually changes AND we
    // don't already have data for it.
    let _lastProjectedHost = '';
    $: if (dashSelectedHost && dashSelectedHost !== _lastProjectedHost) {
        _lastProjectedHost = dashSelectedHost;
        if (!capacityProjections[dashSelectedHost]) refreshCapacityProjection(dashSelectedHost);
    }

    // ── Anomaly detection (statistical, no ML) ────────────────────────────
    // For each metric, derive z-score against the rolling history window.
    // Only "strong" / "extreme" anomalies surface to the UI to avoid noise.
    $: anomalyCpu = (() => {
        const h = metricsHistory[dashSelectedHost] || [];
        if (h.length < 4 || !dashMetrics?.cpu) return null;
        // Exclude the current sample from history (avoid self-bias)
        const past = h.slice(0, -1).map(s => s.cpu);
        const r = detectAnomaly(past, dashMetrics.cpu.global);
        return r.severity === 'normal' || r.severity === 'mild' ? null : r;
    })();
    $: anomalyRam = (() => {
        const h = metricsHistory[dashSelectedHost] || [];
        if (h.length < 4 || !dashMetrics?.memory) return null;
        const past = h.slice(0, -1).map(s => s.ram);
        const r = detectAnomaly(past, dashMetrics.memory.percent);
        return r.severity === 'normal' || r.severity === 'mild' ? null : r;
    })();
    $: anomalyDisk = (() => {
        const h = metricsHistory[dashSelectedHost] || [];
        if (h.length < 4 || !dashMetrics?.disks?.length) return null;
        const past = h.slice(0, -1).map(s => s.disk);
        const current = Math.max(...dashMetrics.disks.map(d => d.percent));
        const r = detectAnomaly(past, current);
        return r.severity === 'normal' || r.severity === 'mild' ? null : r;
    })();

    // ── Auto-incident bridge: report anomalies for debounced triggering ──
    // Hands off to $lib/anomaly-bridge which debounces and decides whether
    // the threshold has been sustained long enough to spin up an incident.
    // Don't fire anomaly bridge if hostName hasn't resolved yet ('---' or empty)
    // to avoid creating incidents with meaningless host identifiers.
    $: if (anomalyCpu && hostName && hostName !== '---') {
        const hName = dashSelectedHost === 'local' ? hostName : dashSelectedHost;
        reportAnomaly(dashSelectedHost, hName, 'cpu', anomalyCpu, 'dashboard');
    }
    $: if (anomalyRam && hostName && hostName !== '---') {
        const hName = dashSelectedHost === 'local' ? hostName : dashSelectedHost;
        reportAnomaly(dashSelectedHost, hName, 'ram', anomalyRam, 'dashboard');
    }
    $: if (anomalyDisk && hostName && hostName !== '---') {
        const hName = dashSelectedHost === 'local' ? hostName : dashSelectedHost;
        reportAnomaly(dashSelectedHost, hName, 'disk', anomalyDisk, 'dashboard');
    }

    onDestroy(() => {
        stopDashboard();
    });
</script>

<div class="view-wrap">
  <div class="view-hdr">
    <div class="view-title" style="display:flex;align-items:center;gap:6px;{dashSelectedHost!=='local'?(()=>{const hc=hosts.find(h=>h.id===dashSelectedHost);return hc?.color?`border-left:3px solid ${hc.color};padding-left:10px;`:'';})():''}"><BarChart3 size={13} stroke={2}/> {isEN ? 'System Dashboard' : 'Dashboard de Sistema'}</div>
    <div style="display:flex;align-items:center;gap:8px;margin-left:auto;flex-wrap:wrap;">
      <select class="view-select" bind:value={dashSelectedHost} on:change={onDashHostChange}>
        <option value="local">⊡ Local ({hostName})</option>
        {#each hosts as h}<option value={h.id}>{h.type==='windows'?'⊡':'◈'} {h.name}</option>{/each}
      </select>
      {#if dashRefreshTimer}
        <span class="dash-auto-badge" title={isEN ? 'Auto-refresh every 10s' : 'Auto-actualización cada 10s'}>
          <span class="dash-pulse"></span>auto
        </span>
      {/if}
      <button class="view-btn" on:click={refreshDash} disabled={dashLoading} title={isEN ? 'Refresh now' : 'Actualizar ahora'}>{dashLoading?'⏳':'↻'}</button>
      <button class="view-btn" on:click={() => showAlertsModal=true} title={isEN ? 'Configure proactive alerts' : 'Configurar alertas proactivas'}
        style="position:relative;display:flex;align-items:center;gap:4px;"><Bell size={13} stroke={1.8}/>{#if $activeAlerts.length}<span class="alert-badge-btn">{$activeAlerts.length}</span>{/if}</button>
      {#if dashLastUpdate}
        <span class="dash-last-update">{isEN ? 'Upd.' : 'Act.'} {dashLastUpdate}</span>
      {/if}
    </div>
  </div>
  {#if dashError}
    <div class="view-error" style="display:flex;align-items:center;gap:6px;"><AlertTriangle size={12} stroke={2}/> {dashError}</div>
  {:else if !dashMetrics && dashLoading}
    <div class="dash-skeleton">
      <div class="dash-cards">
        <div class="sk-card"><div class="sk-lbl"></div><div class="sk-val"></div><div class="sk-bar"></div><div class="sk-sub"></div></div>
        <div class="sk-card"><div class="sk-lbl"></div><div class="sk-val"></div><div class="sk-bar"></div><div class="sk-sub"></div></div>
        <div class="sk-card"><div class="sk-lbl"></div><div class="sk-val"></div><div class="sk-sub"></div><div class="sk-sub short"></div></div>
      </div>
      <div class="sk-section">
        <div class="sk-row"></div><div class="sk-row short"></div><div class="sk-row"></div><div class="sk-row short"></div>
      </div>
    </div>
  {:else if dashMetrics}
  <!-- Sprint C D15 — Open incidents banner for the selected host -->
  {#if openIncidents.open_count > 0}
  <div class="dc-incidents-banner" role="status">
    <span class="dc-banner-ico">⚠</span>
    <span>
      <b>{openIncidents.open_count}</b>
      {isEN
        ? `open incident${openIncidents.open_count === 1 ? '' : 's'} on this host`
        : `incidente${openIncidents.open_count === 1 ? '' : 's'} abierto${openIncidents.open_count === 1 ? '' : 's'} en este host`}
      {#if openIncidents.latest_title}
        · {isEN ? 'most recent' : 'más reciente'}: <em>{openIncidents.latest_title.slice(0, 80)}</em>
      {/if}
    </span>
    <button class="dc-banner-cta" on:click={() => dispatch('setview', { view: 'incidents' })}>
      {isEN ? 'Open incidents view →' : 'Abrir vista de incidentes →'}
    </button>
  </div>
  {/if}
  {#if $activeAlerts.length}
  <div class="alert-bar">
    {#each $activeAlerts as al}
    <div class="alert-item">
      <span class="alert-item-ico"><AlertTriangle size={13} stroke={2} style="color:var(--red)"/></span>
      <span><b>{al.metric}</b> {isEN ? 'on' : 'en'} <b>{al.hostLabel}</b>: <span style="color:var(--red);font-weight:700;">{al.value}%</span> ({isEN ? 'threshold' : 'umbral'} {al.threshold}%) · {al.ts}</span>
      <button class="alert-dismiss" on:click={() => $activeAlerts = $activeAlerts.filter(x=>x.id!==al.id)} title={isEN ? 'Dismiss' : 'Descartar'}>✕</button>
    </div>
    {/each}
  </div>
  {/if}
  <div class="dash-scroll">
    <div class="dash-cards">
      <div class="dash-card lucy-card-hover" class:anomaly-card={anomalyCpu}>
        <div class="dc-label">
          CPU
          <!-- D14 — Thresholds editor trigger -->
          <button class="dc-thr-btn" on:click={() => openThresholdEditor('cpu')}
                  title={isEN ? 'Edit warn/crit thresholds' : 'Editar umbrales warn/crit'}>⚙</button>
          {#if anomalyCpu}
            <span class="anomaly-badge"
                  class:extreme={anomalyCpu.severity === 'extreme'}
                  title={isEN ? `Statistical anomaly: ${anomalyCpu.message}` : `Anomalía estadística: ${anomalyCpu.message}`}>
              <Heartbeat size={10} stroke={2.5}/>
              {Number.isFinite(anomalyCpu.sigma) ? Math.abs(anomalyCpu.sigma).toFixed(1) + 'σ' : '∞σ'}
            </span>
          {/if}
        </div>
        <div style="display:flex;align-items:flex-end;justify-content:space-between;gap:8px;">
          <div>
            <div class="dc-value" style="color:{sevVarFor('cpu', dashMetrics.cpu.global, 'var(--acc)')}">
              <span use:countUp={{ target: dashMetrics.cpu.global, suffix: '%', duration: 900 }}></span>
            </div>
            <div class="dc-bar"><div class="dc-bar-fill" style="width:{dashMetrics.cpu.global}%;background:{sevVarFor('cpu', dashMetrics.cpu.global, 'var(--acc)')}"></div></div>
            <div class="dc-sub">{dashMetrics.cpu.cores} {isEN ? 'cores' : 'núcleos'}</div>
            <!-- Tier A #3 — OLS projection pill. Only shows when we have a
                 regression with ≥5 samples (≥14 days of data). -->
            {#if capacityProjections[dashSelectedHost]?.cpu}
              {@const _lbl = projectionLabel(capacityProjections[dashSelectedHost].cpu, 95)}
              {#if _lbl}<span class="dc-proj-pill {_lbl.cls}"
                  title={isEN
                    ? `Linear regression on last 14 days · slope ${capacityProjections[dashSelectedHost].cpu.slope.toFixed(2)}%/day · R²=${capacityProjections[dashSelectedHost].cpu.r_squared.toFixed(2)}`
                    : `Regresión lineal 14 días · pendiente ${capacityProjections[dashSelectedHost].cpu.slope.toFixed(2)}%/día · R²=${capacityProjections[dashSelectedHost].cpu.r_squared.toFixed(2)}`}>
                {_lbl.text}
              </span>{/if}
            {/if}
          </div>
          <div class="dc-sparkline">{@html sparklineSvg(metricsHistory[dashSelectedHost],'cpu',sevHexFor('cpu', dashMetrics.cpu.global))}</div>
        </div>
      </div>
      <div class="dash-card lucy-card-hover" class:anomaly-card={anomalyRam}>
        <div class="dc-label">
          RAM
          <button class="dc-thr-btn" on:click={() => openThresholdEditor('ram')}
                  title={isEN ? 'Edit warn/crit thresholds' : 'Editar umbrales warn/crit'}>⚙</button>
          {#if anomalyRam}
            <span class="anomaly-badge"
                  class:extreme={anomalyRam.severity === 'extreme'}
                  title={isEN ? `Statistical anomaly: ${anomalyRam.message}` : `Anomalía estadística: ${anomalyRam.message}`}>
              <Heartbeat size={10} stroke={2.5}/>
              {Number.isFinite(anomalyRam.sigma) ? Math.abs(anomalyRam.sigma).toFixed(1) + 'σ' : '∞σ'}
            </span>
          {/if}
        </div>
        <div style="display:flex;align-items:flex-end;justify-content:space-between;gap:8px;">
          <div>
            <div class="dc-value" style="color:{sevVarFor('ram', dashMetrics.memory.percent, 'var(--blue)')}">
              <span use:countUp={{ target: dashMetrics.memory.percent, suffix: '%', duration: 900 }}></span>
            </div>
            <div class="dc-bar"><div class="dc-bar-fill" style="width:{dashMetrics.memory.percent}%;background:{sevVarFor('ram', dashMetrics.memory.percent, 'var(--blue)')}"></div></div>
            <div class="dc-sub">{(dashMetrics.memory.used_mb/1024).toFixed(1)} / {(dashMetrics.memory.total_mb/1024).toFixed(1)} GB</div>
            {#if capacityProjections[dashSelectedHost]?.ram}
              {@const _lbl = projectionLabel(capacityProjections[dashSelectedHost].ram, 95)}
              {#if _lbl}<span class="dc-proj-pill {_lbl.cls}"
                  title={isEN
                    ? `Linear regression on last 14 days · slope ${capacityProjections[dashSelectedHost].ram.slope.toFixed(2)}%/day · R²=${capacityProjections[dashSelectedHost].ram.r_squared.toFixed(2)}`
                    : `Regresión lineal 14 días · pendiente ${capacityProjections[dashSelectedHost].ram.slope.toFixed(2)}%/día · R²=${capacityProjections[dashSelectedHost].ram.r_squared.toFixed(2)}`}>
                {_lbl.text}
              </span>{/if}
            {/if}
          </div>
          <div class="dc-sparkline">{@html sparklineSvg(metricsHistory[dashSelectedHost],'ram',sevHexFor('ram', dashMetrics.memory.percent, _SEV_HEX.okBlue))}</div>
        </div>
      </div>
      <div class="dash-card lucy-card-hover">
        <div class="dc-label">{isEN ? 'System' : 'Sistema'}</div>
        <div class="dc-value" style="font-size:13px;color:var(--txt);">{dashMetrics.hostname}</div>
        <div class="dc-sub">{dashMetrics.os}</div>
        <div class="dc-sub">Uptime: {dashMetrics.uptime_h}h</div>
        {#if metricsHistory[dashSelectedHost]?.length > 1}
        <div class="dc-sub" style="margin-top:4px;color:#4ade80;display:flex;align-items:center;gap:4px;"><TrendingUp size={11} stroke={2}/> {metricsHistory[dashSelectedHost].length} {isEN ? 'samples' : 'muestras'}</div>
        {/if}
      </div>

      <!-- D2 — Page file / swap card. Only renders when the host has swap
           configured; many Win11 desktops have it disabled. -->
      {#if dashMetrics.swap?.enabled}
        <div class="dash-card lucy-card-hover">
          <div class="dc-label">
            {isEN ? 'Page file' : 'Archivo paginación'}
            <button class="dc-thr-btn" on:click={() => openThresholdEditor('swap')}
                    title={isEN ? 'Edit warn/crit thresholds' : 'Editar umbrales warn/crit'}>⚙</button>
            <span class="dc-hint" title={isEN
              ? 'Swap / page file. High usage = real memory pressure, often more telling than RAM% alone.'
              : 'Swap / archivo de paginación. Uso alto = presión de memoria real, suele ser más revelador que solo RAM%.'}>ⓘ</span>
          </div>
          <div class="dc-value" style="color:{sevVarFor('swap', dashMetrics.swap.percent, 'var(--blue)')}">
            <span use:countUp={{ target: dashMetrics.swap.percent, suffix: '%', duration: 900 }}></span>
          </div>
          <div class="dc-bar"><div class="dc-bar-fill"
                style="width:{dashMetrics.swap.percent}%;background:{sevVarFor('swap', dashMetrics.swap.percent, 'var(--blue)')}"></div></div>
          <div class="dc-sub">
            {(dashMetrics.swap.used_mb/1024).toFixed(1)} / {(dashMetrics.swap.total_mb/1024).toFixed(1)} GB
          </div>
        </div>
      {/if}

      <!-- D4 — Temperatures card. Only renders when at least one usable
           sensor exists. Linux without lm-sensors → no card. -->
      {#if dashMetrics.temperatures?.available && dashMetrics.temperatures.sensors.length > 0}
        {@const _maxTemp = Math.max(...dashMetrics.temperatures.sensors.map(s => s.celsius))}
        {@const _tempColor = _maxTemp >= 85 ? 'var(--red)' : _maxTemp >= 70 ? 'var(--amber)' : 'var(--acc)'}
        <div class="dash-card lucy-card-hover">
          <div class="dc-label">
            {isEN ? 'Temperatures' : 'Temperaturas'}
            <span class="dc-hint" title={isEN
              ? 'CPU/GPU/SSD sensors. Thermal throttling above 85°C — silent perf killer.'
              : 'Sensores CPU/GPU/SSD. Thermal throttling sobre 85°C — degrada perf en silencio.'}>ⓘ</span>
          </div>
          <div class="dc-value" style="color:{_tempColor};">
            <span use:countUp={{ target: _maxTemp, suffix: '°C', duration: 900 }}></span>
            <span style="font-size:10px;color:var(--txt2);margin-left:4px;">max</span>
          </div>
          <div class="temp-list">
            {#each dashMetrics.temperatures.sensors as t}
              {@const _c = t.celsius >= 85 ? 'var(--red)' : t.celsius >= 70 ? 'var(--amber)' : 'var(--acc)'}
              <div class="temp-row" title={t.critical ? `critical: ${t.critical}°C` : ''}>
                <span class="temp-name">{t.label.length > 20 ? t.label.slice(0, 20) + '…' : t.label}</span>
                <span class="temp-val" style="color:{_c};">{t.celsius}°C</span>
              </div>
            {/each}
          </div>
        </div>
      {/if}

      <!-- Sprint C D17 — Failed logins (Windows Security log, last 24h) -->
      {#if failedLogins.available || (failedLogins.note && failedLogins.note !== 'Local only')}
        {@const _fl = failedLogins}
        {@const _color = _fl.count_24h >= 10 ? 'var(--red)' : _fl.count_24h >= 5 ? 'var(--amber)' : 'var(--acc)'}
        {@const _flClickable = _fl.available && _fl.count_24h > 0}
        <!-- svelte-ignore a11y_no_noninteractive_tabindex -->
        <div class="dash-card lucy-card-hover" class:dash-card-clickable={_flClickable}
             role={_flClickable ? 'button' : undefined} tabindex={_flClickable ? 0 : undefined}
             on:click={() => { if (_flClickable) openFlDetail(); }}
             on:keydown={(e) => { if (_flClickable && (e.key === 'Enter' || e.key === ' ')) { e.preventDefault(); openFlDetail(); } }}>
          <div class="dc-label">
            {isEN ? 'Failed logins (24h)' : 'Logins fallidos (24h)'}
            <span class="dc-hint" title={isEN
              ? 'Windows Security log event 4625. Reading this requires admin privileges.'
              : 'Evento 4625 del Security log de Windows. Leerlo requiere permisos admin.'}>ⓘ</span>
          </div>
          {#if _fl.available}
            <div class="dc-value" style="color:{_color};">
              <span use:countUp={{ target: _fl.count_24h, duration: 900 }}></span>
            </div>
            <div class="dc-sub">
              {#if _fl.count_24h === 0}
                {isEN ? 'No failed attempts' : 'Sin intentos fallidos'}
              {:else}
                <span class="fl-drill">{isEN ? 'Event ID 4625 · click to inspect →' : 'Event ID 4625 · clic para ver detalle →'}</span>
              {/if}
            </div>
          {:else}
            <div class="dc-value" style="font-size:13px;color:var(--txt2);">—</div>
            <div class="dc-sub" style="color:#94a3b8;">{_fl.note}</div>
          {/if}
        </div>
      {/if}

      <!-- D1 — Network throughput card. Always renders (every host has at
           least one interface); the first call shows 0/0 until a delta exists. -->
      {#if dashMetrics.network}
        <div class="dash-card lucy-card-hover">
          <div class="dc-label">
            {isEN ? 'Network' : 'Red'}
            <span class="dc-hint" title={isEN
              ? 'Throughput in Mbps. Spikes correlate with backups, deploys, or unexpected traffic.'
              : 'Throughput en Mbps. Picos correlacionan con backups, deploys o tráfico inesperado.'}>ⓘ</span>
          </div>
          <div class="net-rates">
            <div class="net-rate">
              <span class="net-arrow" style="color:var(--blue);">↓</span>
              <span class="net-val">{dashMetrics.network.recv_mbps.toFixed(1)}</span>
              <span class="net-unit">Mbps</span>
            </div>
            <div class="net-rate">
              <span class="net-arrow" style="color:var(--acc);">↑</span>
              <span class="net-val">{dashMetrics.network.send_mbps.toFixed(1)}</span>
              <span class="net-unit">Mbps</span>
            </div>
          </div>
          {#if dashMetrics.network.interfaces?.length}
            <div class="net-ifaces">
              {#each dashMetrics.network.interfaces.slice(0, 3) as iface}
                <div class="net-iface" title="{iface.name}: {(iface.total_recv/1e9).toFixed(2)}GB rx · {(iface.total_send/1e9).toFixed(2)}GB tx">
                  <span class="net-iface-name">{iface.name.length > 12 ? iface.name.slice(0, 12) + '…' : iface.name}</span>
                </div>
              {/each}
            </div>
          {/if}
        </div>
      {/if}
    </div>

    <!-- D14 — Threshold editor floating popover -->
    {#if editingThresholdsFor}
      <div class="dc-thr-modal" role="dialog" aria-label="Edit thresholds">
        <div class="dc-thr-modal-inner">
          <div class="dc-thr-modal-hdr">
            <strong>{editingThresholdsFor.toUpperCase()} {isEN ? 'thresholds' : 'umbrales'}</strong>
            <span class="dc-thr-host">@ {dashSelectedHost === 'local' ? 'local' : dashSelectedHost}</span>
            <button class="dc-thr-x" on:click={() => editingThresholdsFor = null}>✕</button>
          </div>
          <div class="dc-thr-row">
            <label for="dc-thr-warn">warn ≥</label>
            <input id="dc-thr-warn" type="number" min="1" max="98" bind:value={_thrDraft.warn}/>
            <span class="dc-thr-default">{isEN ? 'default' : 'default'}: {DEFAULT_THRESHOLDS[editingThresholdsFor]?.warn}</span>
          </div>
          <div class="dc-thr-row">
            <label for="dc-thr-crit">crit ≥</label>
            <input id="dc-thr-crit" type="number" min="2" max="99" bind:value={_thrDraft.crit}/>
            <span class="dc-thr-default">{isEN ? 'default' : 'default'}: {DEFAULT_THRESHOLDS[editingThresholdsFor]?.crit}</span>
          </div>
          <div class="dc-thr-actions">
            <button class="dc-thr-save" on:click={saveThresholdDraft}>{isEN ? 'Save' : 'Guardar'}</button>
            <button class="dc-thr-reset" on:click={() => { resetThresholds(editingThresholdsFor); editingThresholdsFor = null; }}>
              {isEN ? 'Reset to default' : 'Restaurar default'}
            </button>
          </div>
        </div>
      </div>
    {/if}

    <!-- D-Proc — process right-click menu -->
    {#if procMenu}
      <button class="proc-menu-backdrop" aria-label={isEN ? 'Close menu' : 'Cerrar menú'} on:click={closeProcMenu} on:contextmenu|preventDefault={closeProcMenu}></button>
      <div class="proc-menu" role="menu"
           style="left:{Math.min(procMenu.x, (typeof window!=='undefined'?window.innerWidth:9999) - 230)}px;top:{Math.min(procMenu.y, (typeof window!=='undefined'?window.innerHeight:9999) - 180)}px;">
        <div class="proc-menu-hdr">{procMenu.proc.name} · PID {procMenu.proc.pid}</div>
        <button class="proc-menu-item" on:click={() => askLucyAboutProc(procMenu.proc)}>🔎 {isEN ? 'Ask Lucy about this' : 'Preguntar a Lucy'}</button>
        {#if procMenu.proc.path}
          <button class="proc-menu-item" on:click={() => revealProc(procMenu.proc)}>📁 {isEN ? 'Open file location' : 'Abrir ubicación'}</button>
        {/if}
        <button class="proc-menu-item" on:click={() => copyProcPid(procMenu.proc)}>⧉ {isEN ? 'Copy PID' : 'Copiar PID'}</button>
        <button class="proc-menu-item proc-menu-danger" on:click={() => killProc(procMenu.proc)}>⛔ {isEN ? 'End task' : 'Finalizar tarea'}</button>
      </div>
    {/if}

    <!-- D-Login — failed-logins drill-down -->
    {#if flDetailOpen}
      <button class="fl-modal-backdrop" aria-label={isEN ? 'Close' : 'Cerrar'} on:click={() => flDetailOpen = false}></button>
      <div class="fl-modal" role="dialog" aria-label="Failed logins detail">
        <div class="fl-modal-hdr">
          <strong>{isEN ? 'Failed logins — last 24h' : 'Logins fallidos — últimas 24h'}</strong>
          <button class="fl-modal-x" on:click={() => flDetailOpen = false}>✕</button>
        </div>
        {#if flDetailLoading}
          <div class="fl-modal-empty">{isEN ? 'Loading…' : 'Cargando…'}</div>
        {:else if flDetail.length === 0}
          <div class="fl-modal-empty">{isEN ? 'No detailed events (needs admin to read the Security log).' : 'Sin eventos detallados (requiere admin para leer el Security log).'}</div>
        {:else}
          <div class="fl-modal-body">
            <table class="fl-table">
              <thead><tr><th>{isEN ? 'Time' : 'Hora'}</th><th>{isEN ? 'User' : 'Usuario'}</th><th>{isEN ? 'Source IP' : 'IP origen'}</th><th>{isEN ? 'Workstation' : 'Equipo'}</th><th>{isEN ? 'Type' : 'Tipo'}</th></tr></thead>
              <tbody>
                {#each flDetail as ev}
                  <tr>
                    <td>{ev.time}</td>
                    <td>{ev.user || '—'}</td>
                    <td class="fl-ip">{ev.source_ip && ev.source_ip !== '-' ? ev.source_ip : '—'}</td>
                    <td>{ev.workstation || '—'}</td>
                    <td>{ev.logon_type || '—'}</td>
                  </tr>
                {/each}
              </tbody>
            </table>
          </div>
        {/if}
      </div>
    {/if}

    <!-- Sprint E D11 — Section toolbar (reset layout) appears only if user
         has customized order or hidden anything. Drag handles live inside
         each section header below. -->
    {#if sectionOrder.join(',') !== DEFAULT_SECTION_ORDER.join(',') || hiddenSections.size > 0}
      <div class="dash-section-toolbar">
        <span class="dst-hint">
          {isEN ? '✎ Layout customized' : '✎ Layout personalizado'}
          {#if hiddenSections.size > 0}
            · {hiddenSections.size} {isEN ? 'hidden' : 'oculta' + (hiddenSections.size === 1 ? '' : 's')}
          {/if}
        </span>
        <button class="dst-reset" on:click={resetSectionLayout}>
          ↺ {isEN ? 'Reset to default' : 'Restaurar default'}
        </button>
      </div>
    {/if}

    {#each sectionOrder as sectionKey, sIdx (sectionKey)}
      {@const isHidden = hiddenSections.has(sectionKey)}
      {@const shouldRender = !isHidden && (
        (sectionKey === 'cores'     && dashMetrics.cpu.per_core?.length) ||
        (sectionKey === 'storage'   && dashMetrics.disks?.length) ||
        (sectionKey === 'processes' && dashMetrics.top_processes?.length)
      )}
      {#if shouldRender}
        <div class="dash-section"
             class:anomaly-section={sectionKey === 'storage' && anomalyDisk}
             draggable="true"
             on:dragstart={(e) => onSectionDragStart(e, sIdx)}
             on:dragover={onSectionDragOver}
             on:drop={(e) => onSectionDrop(e, sIdx)}
             role="region" aria-label={sectionTitle(sectionKey)}>
          <div class="ds-title">
            <span class="ds-drag-handle" title={isEN ? 'Drag to reorder' : 'Arrastra para reordenar'}>⋮⋮</span>
            <span class="ds-title-text">{sectionTitle(sectionKey)}</span>
            {#if sectionKey === 'storage' && anomalyDisk}
              <span class="anomaly-badge" class:extreme={anomalyDisk.severity === 'extreme'}
                    title={isEN ? `Statistical anomaly: ${anomalyDisk.message}` : `Anomalía estadística: ${anomalyDisk.message}`}>
                <Heartbeat size={10} stroke={2.5}/> {Number.isFinite(anomalyDisk.sigma) ? Math.abs(anomalyDisk.sigma).toFixed(1) + 'σ' : '∞σ'}
              </span>
            {/if}
            <button class="ds-hide-btn" on:click={() => toggleSectionHidden(sectionKey)}
                    title={isEN ? 'Hide this section' : 'Ocultar esta sección'}>👁</button>
          </div>

          {#if sectionKey === 'cores'}
            <CpuHeatmap
              cores={dashMetrics.cpu.per_core}
              topProcessPerCore={dashMetrics.cpu.top_process_per_core || []}
              showLabels={true}
              showPct={true}
            />
          {:else if sectionKey === 'storage'}
            {#each dashMetrics.disks as disk}
              {@const _low = disk.percent >= 90}
              <div class="disk-row" class:disk-low={_low}>
                <div class="disk-name">
                  {disk.name||disk.mount}
                  {#if _low}<span class="disk-low-tag" title={isEN ? 'Less than 10% free' : 'Menos del 10% libre'}>⚠ {isEN ? 'low' : 'poco'}</span>{/if}
                </div>
                <div class="disk-bar-wrap"><div class="disk-bar-fill" style="width:{disk.percent}%;background:{diskSevVar(disk.percent)}"></div></div>
                <div class="disk-pct" style="color:{disk.percent >= 75 ? diskSevVar(disk.percent) : 'var(--txt2)'}">{disk.percent}%</div>
                <div class="disk-size">{disk.used_gb}G / {disk.total_gb}G{#if disk.free_gb != null} · {disk.free_gb}G {isEN ? 'free' : 'libre'}{/if}</div>
              </div>
            {/each}
          {:else if sectionKey === 'processes'}
            <table class="proc-table">
              <thead><tr>
                <th class="proc-th" on:click={() => setProcSort('name')}>{isEN ? 'Process' : 'Proceso'}{procSortKey==='name'?(procSortDir<0?' ▾':' ▴'):''}</th>
                <th class="proc-th proc-th-num" on:click={() => setProcSort('cpu')}>CPU %{procSortKey==='cpu'?(procSortDir<0?' ▾':' ▴'):''}</th>
                <th class="proc-th proc-th-num" on:click={() => setProcSort('mem_mb')}>RAM MB{procSortKey==='mem_mb'?(procSortDir<0?' ▾':' ▴'):''}</th>
                <th class="proc-th proc-th-num" on:click={() => setProcSort('pid')}>PID{procSortKey==='pid'?(procSortDir<0?' ▾':' ▴'):''}</th>
              </tr></thead>
              <tbody>
                {#each sortedProcs as p}
                  {@const _lineage = processLineage.get(Number(p.pid))}
                <tr class="proc-row" class:proc-self={p.name === SELF_PROC}
                    on:contextmenu={(e) => openProcMenu(e, p)}
                    title={isEN ? 'Right-click for actions (end task, open location, ask Lucy)' : 'Clic derecho para acciones (finalizar, abrir ubicación, preguntar a Lucy)'}>
                  <td style="font-family:var(--mono);font-size:11px;color:var(--txt);">
                    {p.name}
                    {#if p.name === SELF_PROC}<span class="proc-self-tag">Lucy</span>{/if}
                    {#if _lineage?.is_new_24h}
                      <span class="dc-pid-new-badge"
                            title={isEN
                              ? `First seen ${fmtRelHours(_lineage.first_seen)} ago by Lucy's process lineage tracker`
                              : `Visto por primera vez hace ${fmtRelHours(_lineage.first_seen)} por el lineage tracker de Lucy`}>
                        ● {isEN ? 'new' : 'nuevo'}
                      </span>
                    {/if}
                  </td>
                  <td style="color:{sevVar(Number(p.cpu) || 0, 'var(--txt2)')}">{p.cpu}</td>
                  <td style="color:var(--blue)">{typeof p.mem_mb==='number'?p.mem_mb.toLocaleString():p.mem_mb}</td>
                  <td style="color:#64748b">{p.pid||'-'}</td>
                </tr>
                {/each}
              </tbody>
            </table>
          {/if}
        </div>
      {:else if isHidden}
        <!-- Hidden section: small ghost row with un-hide button. Lets the
             user bring it back without going through Settings. -->
        <div class="dash-section-ghost">
          <span>{sectionTitle(sectionKey)}</span>
          <button class="ds-unhide-btn" on:click={() => toggleSectionHidden(sectionKey)}>
            🙈 → 👁 {isEN ? 'Show' : 'Mostrar'}
          </button>
        </div>
      {/if}
    {/each}
  </div>
  {:else}
    <div class="view-loading"><span style="color:var(--txt3)">{isEN ? 'Select a host to view metrics' : 'Selecciona un host para ver métricas'}</span></div>
  {/if}
</div>

<style>
    /* ── View shared ──────────────────────────────── */
    .view-wrap{flex:1;display:flex;flex-direction:column;overflow:hidden;min-height:0;}
    .view-hdr{display:flex;align-items:center;padding:10px 16px;background:rgba(2,4,8,.6);border-bottom:1px solid var(--bdr);flex-shrink:0;gap:10px;}
    .view-title{font-size:13px;font-weight:700;color:var(--txt);white-space:nowrap;}
    .view-select{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt);font-size:12px;padding:4px 8px;cursor:pointer;outline:none;}
    .view-select:focus{border-color:var(--acc-b);}
    .view-btn{background:var(--bg3);border:1px solid var(--bdr);border-radius:5px;color:var(--txt2);font-size:12px;padding:4px 10px;cursor:pointer;transition:.15s;white-space:nowrap;}
    .view-btn:hover{background:var(--bdr2);color:var(--txt);}
    .view-btn:disabled{opacity:.35;cursor:not-allowed;}
    .view-error{margin:12px 16px;padding:10px 14px;background:rgba(239,68,68,.08);border:1px solid rgba(239,68,68,.2);border-radius:6px;font-size:12px;color:var(--red);}
    .view-loading{flex:1;display:flex;align-items:center;justify-content:center;gap:12px;font-size:13px;color:var(--txt3);}

    /* ── Dashboard auto-refresh badge ────────────── */
    .dash-auto-badge{display:inline-flex;align-items:center;gap:5px;font-size:10px;color:var(--acc);background:rgba(16,185,129,.07);border:1px solid rgba(16,185,129,.15);border-radius:10px;padding:2px 8px;white-space:nowrap;}
    .dash-pulse{width:6px;height:6px;border-radius:50%;background:var(--acc);animation:dash-pulse-anim 2s ease-in-out infinite;}
    @keyframes dash-pulse-anim{0%,100%{opacity:1;transform:scale(1);}50%{opacity:.4;transform:scale(.7);}}
    .dash-last-update{font-size:10px;color:#4a5a6a;white-space:nowrap;}

    /* ── Dashboard cards & layout ─────────────────── */
    .dash-scroll{flex:1;overflow-y:auto;padding:16px;}
    .dash-cards{display:grid;grid-template-columns:repeat(3,1fr);gap:12px;margin-bottom:16px;}
    .dash-card{background:rgba(0,0,0,.2);border:1px solid var(--bdr);border-radius:10px;padding:14px 16px;transition:border-color .3s,box-shadow .3s;}
    .dash-card:hover{border-color:rgba(255,255,255,.08);box-shadow:0 2px 16px rgba(0,0,0,.3);}
    .dc-label{font-size:10px;color:#7a9ab5;letter-spacing:.5px;text-transform:uppercase;font-weight:700;margin-bottom:6px;}
    .dc-value{font-size:28px;font-weight:400;margin-bottom:6px;line-height:1;}
    .dc-bar{height:3px;background:var(--bdr);border-radius:2px;margin-bottom:6px;overflow:hidden;}
    .dc-bar-fill{height:100%;border-radius:2px;transition:width .8s cubic-bezier(.4,0,.2,1);}
    .dc-sub{font-size:11px;color:#94a3b8;margin-top:2px;}
    .dc-sparkline{opacity:.85;flex-shrink:0;align-self:flex-end;margin-bottom:2px;}
    /* Tier A #3 — Projection pill: regression-based forecast badge */
    .dc-proj-pill{
        display:inline-block;font-size:9px;font-weight:700;letter-spacing:.3px;
        padding:1px 6px;border-radius:8px;margin-top:4px;cursor:help;
        font-variant-numeric:tabular-nums;
    }
    .dc-proj-pill.pj-stable{background:rgba(148,163,184,.14);color:#94a3b8;}
    .dc-proj-pill.pj-ok    {background:rgba(16,185,129,.14);color:#10b981;}
    .dc-proj-pill.pj-warn  {background:rgba(245,158,11,.16);color:#f59e0b;}
    .dc-proj-pill.pj-crit  {background:rgba(239,68,68,.18);color:#ef4444;}

    /* D2/D4/D1 — Coverage cards (Page file / Temperatures / Network) */
    .dc-hint {
        display:inline-block;width:14px;height:14px;line-height:14px;
        text-align:center;font-size:10px;color:var(--txt3);
        border:1px solid #2a3a4a;border-radius:50%;
        cursor:help;margin-left:4px;vertical-align:middle;
    }
    .dc-hint:hover{color:var(--txt2);border-color:var(--bdr);}
    /* Temperature list inside the Temperatures card */
    .temp-list{display:flex;flex-direction:column;gap:3px;margin-top:6px;font-family:var(--mono);font-size:10px;}
    .temp-row{display:flex;justify-content:space-between;align-items:center;gap:8px;}
    .temp-name{color:var(--txt2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
    .temp-val{font-weight:600;flex-shrink:0;font-variant-numeric:tabular-nums;}
    /* Network throughput card layout */
    .net-rates{display:flex;gap:12px;align-items:baseline;margin-top:4px;flex-wrap:wrap;}
    .net-rate{display:flex;align-items:baseline;gap:4px;}
    .net-arrow{font-size:14px;font-weight:700;}
    .net-val{font-size:22px;font-weight:300;color:var(--txt);font-variant-numeric:tabular-nums;}
    .net-unit{font-size:10px;color:var(--txt2);letter-spacing:.3px;}
    .net-ifaces{display:flex;gap:4px;margin-top:8px;flex-wrap:wrap;}
    .net-iface{font-size:9px;padding:1px 6px;border-radius:8px;background:rgba(255,255,255,.04);color:var(--txt2);font-family:var(--mono);cursor:help;}
    .net-iface-name{letter-spacing:.2px;}

    /* ── Dashboard sections ───────────────────────── */
    .dash-section{background:rgba(0,0,0,.15);border:1px solid var(--bdr);border-radius:8px;padding:12px 14px;margin-bottom:12px;cursor:default;transition:border-color .12s, box-shadow .12s;}
    .dash-section[draggable="true"]:hover{border-color:rgba(255,255,255,.10);}
    .dash-section[draggable="true"]:active{box-shadow:0 6px 20px -6px rgba(16,185,129,.30);}
    .ds-title{font-size:11px;color:#7a9ab5;font-weight:700;letter-spacing:.3px;text-transform:uppercase;margin-bottom:10px;display:flex;align-items:center;gap:6px;}
    .ds-drag-handle{
        color:var(--txt3);cursor:grab;user-select:none;
        font-size:14px;letter-spacing:-2px;
        opacity:0.5;transition:opacity .12s, color .12s;
    }
    .ds-drag-handle:hover{opacity:1;color:var(--acc, #10b981);}
    .ds-drag-handle:active{cursor:grabbing;}
    .ds-title-text{flex:1;}
    .ds-hide-btn{
        background:transparent;border:0;padding:0;margin-left:auto;
        color:var(--txt3);font-size:13px;cursor:pointer;
        opacity:0.4;transition:opacity .12s, color .12s;
    }
    .ds-hide-btn:hover{opacity:1;color:var(--amber, #f59e0b);}
    .dash-section-toolbar{
        display:flex;align-items:center;gap:8px;
        padding:6px 12px;margin-bottom:8px;
        background:rgba(16,185,129,.04);border:1px dashed rgba(16,185,129,.20);
        border-radius:6px;font-size:11px;color:var(--txt2);
    }
    .dst-hint{flex:1;}
    .dst-reset{
        background:transparent;border:1px solid rgba(255,255,255,.08);
        color:var(--txt2);font:inherit;font-size:10px;
        padding:2px 8px;border-radius:4px;cursor:pointer;
    }
    .dst-reset:hover{background:rgba(255,255,255,.04);color:var(--txt);}
    .dash-section-ghost{
        display:flex;align-items:center;justify-content:space-between;
        padding:6px 12px;margin-bottom:8px;
        background:rgba(0,0,0,.10);border:1px dashed rgba(255,255,255,.08);
        border-radius:6px;font-size:10px;color:var(--txt2);
        text-transform:uppercase;letter-spacing:.4px;
    }
    .ds-unhide-btn{
        background:transparent;border:1px solid rgba(255,255,255,.10);
        color:var(--txt2);font:inherit;font-size:10px;
        padding:2px 8px;border-radius:4px;cursor:pointer;
    }
    .ds-unhide-btn:hover{background:rgba(16,185,129,.10);color:var(--acc, #10b981);border-color:rgba(16,185,129,.30);}

    /* CPU cores: now rendered by CpuHeatmap component (v1.4.0) */

    /* ── Disks ─────────────────────────────────────── */
    .disk-row{display:grid;grid-template-columns:100px 1fr 44px 80px;align-items:center;gap:10px;margin-bottom:8px;}
    .disk-name{font-size:12px;color:var(--txt2);font-family:var(--mono);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;}
    .disk-bar-wrap{height:6px;background:var(--bdr);border-radius:3px;overflow:hidden;}
    .disk-bar-fill{height:100%;border-radius:3px;transition:width .4s ease;}
    .disk-pct{font-size:11px;font-weight:600;text-align:right;}
    .disk-size{font-size:10px;color:#7a9ab5;font-family:var(--mono);}

    /* ── Process table ─────────────────────────────── */
    .proc-table{width:100%;border-collapse:collapse;font-size:12px;}
    .proc-table th{background:var(--bg4);color:#7a9ab5;padding:5px 10px;text-align:left;font-size:10px;font-weight:700;letter-spacing:.3px;text-transform:uppercase;}
    .proc-table td{padding:5px 10px;border-bottom:1px solid rgba(26,32,48,.4);}
    .proc-table tr:last-child td{border-bottom:none;}

    /* ── Alerts ────────────────────────────────────── */
    /* v1.4.25 — .alert-bar moved to src/lib/styles/dashboard-alerts.css. */
    /* Sprint C D15 — Open incidents banner (amber, less urgent than active alerts) */
    .dc-incidents-banner {
        display: flex; align-items: center; gap: 10px;
        background: rgba(245,158,11,.08);
        border-bottom: 1px solid rgba(245,158,11,.20);
        padding: 8px 14px; flex-shrink: 0; font-size: 12px;
        color: var(--txt);
    }
    .dc-banner-ico { color: #f59e0b; font-size: 14px; }
    .dc-incidents-banner em { color: var(--amber, #f59e0b); font-style: normal; }
    .dc-banner-cta {
        margin-left: auto;
        background: rgba(245,158,11,.18); border: 1px solid rgba(245,158,11,.35);
        color: var(--amber, #f59e0b); font: inherit; font-size: 11px;
        padding: 3px 10px; border-radius: 5px; cursor: pointer;
        transition: background .12s;
    }
    .dc-banner-cta:hover { background: rgba(245,158,11,.30); }
    /* Sprint C D14 — Threshold editor button + modal */
    .dc-thr-btn {
        background: transparent; border: 0; padding: 0;
        color: var(--txt3); font-size: 11px; cursor: pointer;
        margin-left: 4px; opacity: 0.6; transition: opacity .12s, color .12s;
    }
    .dc-thr-btn:hover { opacity: 1; color: var(--acc, #10b981); }
    .dc-thr-modal {
        position: fixed; inset: 0;
        background: rgba(0,0,0,.40);
        display: flex; align-items: center; justify-content: center;
        z-index: 8500;
    }
    .dc-thr-modal-inner {
        background: rgba(20, 24, 36, 0.98);
        border: 1px solid rgba(255,255,255,.10);
        border-radius: 8px;
        padding: 14px 16px;
        min-width: 280px;
        box-shadow: 0 8px 32px -8px rgba(0,0,0,.55);
    }
    .dc-thr-modal-hdr {
        display: flex; align-items: center; gap: 8px;
        font-size: 12px; margin-bottom: 12px;
        padding-bottom: 8px; border-bottom: 1px solid rgba(255,255,255,.06);
    }
    .dc-thr-host { color: var(--txt2); font-size: 10px; font-family: var(--mono); }
    .dc-thr-x {
        background: transparent; border: 0; color: var(--txt2);
        font-size: 14px; cursor: pointer; margin-left: auto;
    }
    .dc-thr-row {
        display: flex; align-items: center; gap: 8px;
        margin-bottom: 8px; font-size: 11px;
    }
    .dc-thr-row label { color: var(--txt2); min-width: 50px; }
    .dc-thr-row input {
        background: rgba(0,0,0,.30); border: 1px solid rgba(255,255,255,.08);
        color: var(--txt); font-family: var(--mono); font-size: 12px;
        padding: 4px 8px; border-radius: 4px; width: 80px; outline: none;
    }
    .dc-thr-row input:focus { border-color: var(--acc, #10b981); }
    .dc-thr-default { font-size: 10px; color: #64748b; }
    .dc-thr-actions { display: flex; gap: 8px; margin-top: 10px; padding-top: 10px;
        border-top: 1px solid rgba(255,255,255,.04); }
    .dc-thr-save, .dc-thr-reset {
        background: rgba(16,185,129,.18); border: 1px solid rgba(16,185,129,.30);
        color: var(--acc, #10b981); font: inherit; font-size: 11px;
        padding: 4px 12px; border-radius: 5px; cursor: pointer;
    }
    .dc-thr-save:hover { background: rgba(16,185,129,.28); }
    .dc-thr-reset {
        background: rgba(255,255,255,.04); border-color: rgba(255,255,255,.10);
        color: var(--txt2); margin-left: auto;
    }
    .dc-thr-reset:hover { background: rgba(255,255,255,.08); color: var(--txt); }
    /* Sprint C D18 — "new" badge next to top process names */
    .dc-pid-new-badge {
        display: inline-block;
        font-size: 9px; font-weight: 700; letter-spacing: .3px;
        padding: 1px 6px; border-radius: 8px;
        background: rgba(16,185,129,.18); color: var(--acc, #10b981);
        margin-left: 6px; vertical-align: middle;
        cursor: help;
    }
    /* v1.4.25 — @keyframes alert-glow, .alert-item, .alert-item-ico,
       .alert-dismiss, .alert-badge-btn moved to dashboard-alerts.css. */

    /* ── Skeleton loaders ──────────────────────────── */
    @keyframes sk-shimmer{0%{background-position:200% 0;}100%{background-position:-200% 0;}}
    .dash-skeleton{padding:16px;display:flex;flex-direction:column;gap:16px;}
    .sk-card{background:var(--bg3);border:1px solid var(--bdr);border-radius:10px;padding:16px;display:flex;flex-direction:column;gap:10px;}
    .sk-lbl{height:10px;width:40px;border-radius:4px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s infinite;}
    .sk-val{height:28px;width:70px;border-radius:6px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s infinite;}
    .sk-bar{height:6px;width:100%;border-radius:3px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s infinite;}
    .sk-sub{height:9px;width:80px;border-radius:4px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s .1s infinite;}
    .sk-sub.short{width:50px;}
    .sk-section{background:var(--bg3);border:1px solid var(--bdr);border-radius:10px;padding:16px;display:flex;flex-direction:column;gap:8px;}
    .sk-row{height:11px;border-radius:4px;background:linear-gradient(90deg,var(--bg4) 25%,rgba(255,255,255,.04) 50%,var(--bg4) 75%);background-size:200% 100%;animation:sk-shimmer 1.5s infinite;}
    .sk-row.short{width:60%;}

    /* ── Light mode overrides ─────────────────────── */
    :global(:root.light) .view-wrap{background:var(--bg);}
    :global(:root.light) .view-hdr{background:rgba(224,230,238,.8);border-bottom-color:var(--bdr);}
    :global(:root.light) .view-loading{color:var(--txt3);}
    :global(:root.light) .dash-card{background:#fff;border-color:var(--bdr);}
    :global(:root.light) .dc-label{color:var(--txt3);}
    :global(:root.light) .dc-sub{color:var(--txt2);}
    :global(:root.light) .ds-title{color:var(--txt2);}
    :global(:root.light) .disk-size{color:var(--txt2);}
    :global(:root.light) .proc-table th{color:var(--txt2);background:var(--bg3);}
    /* v1.4.25 — :root.light .alert-dismiss override moved to dashboard-alerts.css. */

    /* ── Anomaly detection badge ──────────────────────────────────────── */
    .anomaly-badge {
        display:inline-flex; align-items:center; gap:3px;
        margin-left:6px; padding:1px 6px;
        font-size:9px; font-weight:600; letter-spacing:.3px;
        border-radius:8px;
        background:rgba(245,158,11,0.12);
        color:#fbbf24;
        border:1px solid rgba(245,158,11,0.30);
        animation: anomaly-pulse 2.4s ease-in-out infinite;
    }
    .anomaly-badge.extreme {
        background:rgba(239,68,68,0.14);
        color:#f87171;
        border-color:rgba(239,68,68,0.40);
    }
    .anomaly-card {
        position:relative;
        box-shadow: 0 0 0 1px rgba(245,158,11,0.18) inset;
    }
    .anomaly-card:has(.anomaly-badge.extreme) {
        box-shadow: 0 0 0 1px rgba(239,68,68,0.30) inset;
    }
    .anomaly-section {
        box-shadow: 0 0 0 1px rgba(245,158,11,0.18) inset;
    }
    .anomaly-section:has(.anomaly-badge.extreme) {
        box-shadow: 0 0 0 1px rgba(239,68,68,0.30) inset;
    }
    @keyframes anomaly-pulse {
        0%, 100% { opacity: 0.85; }
        50%      { opacity: 1; }
    }

    /* ── D-Proc — sortable headers, self-highlight, right-click menu ── */
    .proc-th{cursor:pointer;user-select:none;transition:color .12s;}
    .proc-th:hover{color:var(--acc);}
    .proc-th-num{text-align:left;}
    .proc-row{transition:background .12s;}
    .proc-row:hover{background:rgba(255,255,255,.035);}
    .proc-self{background:rgba(16,185,129,.06);}
    .proc-self-tag{font-size:8.5px;font-weight:700;color:var(--acc);background:rgba(16,185,129,.12);border:1px solid rgba(16,185,129,.25);border-radius:6px;padding:0 4px;margin-left:4px;letter-spacing:.3px;vertical-align:middle;}
    .proc-menu-backdrop{position:fixed;inset:0;z-index:9998;background:transparent;border:0;padding:0;cursor:default;}
    .proc-menu{position:fixed;z-index:9999;min-width:210px;background:var(--bg3,#161b22);border:1px solid var(--bdr);border-radius:8px;padding:4px;box-shadow:0 10px 34px -8px rgba(0,0,0,.6);}
    .proc-menu-hdr{font-size:10px;color:var(--txt2);padding:5px 8px 6px;border-bottom:1px solid rgba(255,255,255,.06);margin-bottom:3px;font-family:var(--mono);white-space:nowrap;overflow:hidden;text-overflow:ellipsis;max-width:230px;}
    .proc-menu-item{display:block;width:100%;text-align:left;background:none;border:0;color:var(--txt);font:inherit;font-size:12px;padding:6px 9px;border-radius:5px;cursor:pointer;transition:background .1s;}
    .proc-menu-item:hover{background:rgba(255,255,255,.07);}
    .proc-menu-danger{color:#ff6b6b;}
    .proc-menu-danger:hover{background:rgba(239,68,68,.14);}

    /* ── disk low-space marker ── */
    .disk-low-tag{font-size:8.5px;font-weight:700;color:var(--red,#ef4444);background:rgba(239,68,68,.12);border:1px solid rgba(239,68,68,.28);border-radius:6px;padding:0 4px;margin-left:5px;vertical-align:middle;}

    /* ── D-Login — failed-logins drill-down ── */
    .dash-card-clickable{cursor:pointer;}
    .dash-card-clickable:hover{border-color:rgba(16,185,129,.3);}
    .fl-drill{color:var(--acc);}
    .fl-modal-backdrop{position:fixed;inset:0;z-index:9998;background:rgba(0,0,0,.45);border:0;padding:0;cursor:default;}
    .fl-modal{position:fixed;z-index:9999;top:50%;left:50%;transform:translate(-50%,-50%);width:min(760px,92vw);max-height:78vh;display:flex;flex-direction:column;background:var(--bg3,#161b22);border:1px solid var(--bdr);border-radius:12px;box-shadow:0 16px 48px -10px rgba(0,0,0,.65);overflow:hidden;}
    .fl-modal-hdr{display:flex;align-items:center;justify-content:space-between;padding:11px 16px;border-bottom:1px solid var(--bdr);font-size:13px;}
    .fl-modal-x{background:none;border:0;color:var(--txt2);font-size:14px;cursor:pointer;padding:2px 6px;border-radius:5px;}
    .fl-modal-x:hover{background:rgba(239,68,68,.15);color:#ef4444;}
    .fl-modal-empty{padding:32px 16px;text-align:center;color:var(--txt2);font-size:12px;}
    .fl-modal-body{overflow-y:auto;}
    .fl-table{width:100%;border-collapse:collapse;font-size:11.5px;}
    .fl-table th{position:sticky;top:0;background:var(--bg4);color:#7a9ab5;padding:7px 12px;text-align:left;font-size:10px;font-weight:700;text-transform:uppercase;letter-spacing:.3px;}
    .fl-table td{padding:6px 12px;border-bottom:1px solid rgba(26,32,48,.4);font-family:var(--mono);color:var(--txt);white-space:nowrap;}
    .fl-table .fl-ip{color:var(--amber,#f59e0b);}
</style>
